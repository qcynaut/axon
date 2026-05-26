//! Sans-I/O Axon protocol engine.
//!
//! [`AxonEngine`] is a pure state machine: it owns all protocol state and
//! exposes two entry points:
//!
//! - [`process_inbound`] — call with each decoded inbound [`Frame`] and the [`CarrierId`] it
//!   arrived on.  Returns a list of [`EngineAction`]s for the caller to execute (send frames,
//!   deliver events, invoke callbacks).
//! - [`tick`] — call periodically with the current wall-clock time to drive keepalive probes and
//!   deadline enforcement.
//!
//! The engine never performs I/O.  The caller is responsible for encoding
//! outgoing frames (via [`crate::codec::encode_frame`]) and writing them to
//! the appropriate transport.
//!
//! [`process_inbound`]: AxonEngine::process_inbound
//! [`tick`]: AxonEngine::tick

pub mod action;
pub mod event;
pub mod idempotency;
pub mod resumable;
pub mod traits;

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
};

pub use action::{CarrierId, EngineAction, PendingRpc};
pub use event::PendingAckEvent;
pub use idempotency::{IdempotencyRecord, IdempotencyResult};
pub use resumable::ResumableState;
pub use traits::{AxonClientSession, AxonServerHandler, ControlPlane, PipelineDataChannel};

use crate::{
    capability::Capabilities,
    error::{AxonError, ErrorCode},
    frame::{
        Flags, Frame, PROTOCOL_VERSION, Payload,
        payload::{
            ErrorPayload, EventPayload, GoodbyePayload, HelloPayload, IceCandidatePayload,
            PingPayload, PipelineAnswerPayload, PipelineOfferPayload, PipelineReadyPayload,
            PongPayload, RpcErrorPayload, RpcRequestPayload, RpcResponsePayload,
            SessionAcceptPayload, SessionOfferPayload, SessionReadyPayload,
        },
        ty,
    },
    session::{Role, SessionConfig, SessionInfo, SessionState},
};

/// Maximum number of recent `Frame ID` values tracked per sender for replay
/// protection (spec §8: at least 4096).
const REPLAY_WINDOW_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Engine configuration
// ---------------------------------------------------------------------------

/// Configuration supplied at engine construction time.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Whether this endpoint is a client or server.
    pub role: Role,
    /// Capabilities this endpoint wishes to advertise.
    pub capabilities: Capabilities,
    /// Per-session timeout values.
    pub session_config: SessionConfig,
    /// Server only: authentication scheme to advertise (`"none"` or `"bearer"`).
    pub auth_scheme: String,
    /// Server only: Pipeline transports to advertise (MUST be `["webrtc"]`).
    pub supported_transports: Vec<String>,
}

impl EngineConfig {
    /// Default client configuration with RPC + Events capabilities.
    pub fn client() -> Self {
        Self {
            role: Role::Client,
            capabilities: Capabilities::RPC | Capabilities::EVENTS,
            session_config: SessionConfig::default(),
            auth_scheme: String::new(),
            supported_transports: Vec::new(),
        }
    }

    /// Default server configuration with RPC + Events capabilities.
    pub fn server() -> Self {
        Self {
            role: Role::Server,
            capabilities: Capabilities::RPC | Capabilities::EVENTS,
            session_config: SessionConfig::default(),
            auth_scheme: String::from("none"),
            supported_transports: vec![String::from("webrtc")],
        }
    }
}

// ---------------------------------------------------------------------------
// Options structs for builder methods
// ---------------------------------------------------------------------------

/// Options for building an outbound `RPC_REQUEST` frame.
///
/// Passed to [`AxonEngine::build_rpc_request`] to avoid exceeding the
/// function-argument count threshold.
#[derive(Debug, Clone, Default)]
pub struct RpcRequestOptions {
    /// Method parameters; `None` means no parameters (treated as null).
    pub params: Option<rmpv::Value>,
    /// Client-side timeout in milliseconds; `0` means no timeout.
    pub timeout_ms: u32,
    /// Whether the caller expects a server-streaming (multi-response) reply.
    pub expects_stream: bool,
    /// Idempotency key for safe retry deduplication.
    pub idempotency_key: Option<String>,
}

/// Options for building an outbound `ICE_CANDIDATE` frame.
#[derive(Debug, Clone, Default)]
pub struct IceCandidateOptions {
    /// SDP media stream identification tag.
    pub sdp_mid: Option<String>,
    /// SDP media line index.
    pub sdp_mline_index: Option<u16>,
    /// ICE username fragment.
    pub username_fragment: Option<String>,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The Sans-I/O Axon protocol engine.
///
/// Create one engine per session.  Drive it by calling [`process_inbound`]
/// for each received frame and [`tick`] at regular intervals.
///
/// [`process_inbound`]: Self::process_inbound
pub struct AxonEngine {
    /// Engine configuration (role, capabilities, timeouts).
    config: EngineConfig,
    /// Current session state.
    state: SessionState,
    /// Monotonically increasing outbound frame counter (skips 0).
    frame_id_counter: u32,
    /// Monotonically increasing outbound RPC correlation counter (skips 0).
    correlation_id_counter: u32,
    /// Sliding window of recently seen inbound Frame IDs for replay protection.
    replay_window: VecDeque<u32>,
    /// In-flight RPC calls keyed by correlation ID.
    pending_rpc: BTreeMap<u32, PendingRpc>,
    /// Server-side idempotency records keyed by application idempotency key.
    idempotency_cache: BTreeMap<String, IdempotencyRecord>,
    /// Maps in-flight correlation IDs to idempotency keys.
    idempotency_by_correlation: BTreeMap<u32, String>,
    /// Sender-side events waiting for acknowledgement.
    pending_ack_events: BTreeMap<u32, PendingAckEvent>,
    /// Tracks inbound `(topic, seq)` events already delivered.
    event_dedup: HashSet<(String, u64)>,
    /// FIFO order for bounded event dedup eviction.
    event_dedup_order: VecDeque<(String, u64)>,
    /// Negotiated capability intersection (set after SESSION_READY).
    capabilities: Option<Capabilities>,
    /// Active session identifier.
    session_id: Option<String>,
    /// Session ID the client requested to resume.
    pending_resume_session_id: Option<String>,
    /// Last wall-clock time (ms) a frame was sent on each carrier.
    last_sent_ms: BTreeMap<CarrierId, u64>,
    /// Last wall-clock time (ms) a frame was received on each carrier.
    last_recv_ms: BTreeMap<CarrierId, u64>,
    /// Pending PING nonce per carrier (waiting for PONG).
    pending_ping: BTreeMap<CarrierId, (Vec<u8>, u64)>,
    /// Time the current state was entered, for deadline enforcement.
    state_entered_ms: u64,
    /// Epoch-ms at engine creation; used as the baseline for tick().
    created_ms: u64,
}

impl AxonEngine {
    /// Create a new engine with `config` and record the creation time as
    /// `now_ms` (wall-clock milliseconds, e.g. Unix epoch).
    pub fn new(config: EngineConfig, now_ms: u64) -> Self {
        let initial_state = match config.role {
            Role::Server => SessionState::AwaitHello,
            Role::Client => SessionState::AwaitSessionOffer,
        };
        Self {
            config,
            state: initial_state,
            frame_id_counter: 0,
            correlation_id_counter: 0,
            replay_window: VecDeque::with_capacity(REPLAY_WINDOW_SIZE),
            pending_rpc: BTreeMap::new(),
            idempotency_cache: BTreeMap::new(),
            idempotency_by_correlation: BTreeMap::new(),
            pending_ack_events: BTreeMap::new(),
            event_dedup: HashSet::new(),
            event_dedup_order: VecDeque::new(),
            capabilities: None,
            session_id: None,
            pending_resume_session_id: None,
            last_sent_ms: BTreeMap::new(),
            last_recv_ms: BTreeMap::new(),
            pending_ping: BTreeMap::new(),
            state_entered_ms: now_ms,
            created_ms: now_ms,
        }
    }

    /// Create an engine from state preserved across Control Plane resumption.
    pub fn new_resumed(config: EngineConfig, state: ResumableState, now_ms: u64) -> Self {
        let session_id = state.session_id;
        let capabilities = state.capabilities;
        Self {
            config,
            state: SessionState::Ready,
            frame_id_counter: state.frame_id_counter,
            correlation_id_counter: state.correlation_id_counter,
            replay_window: state.replay_window,
            pending_rpc: state.pending_rpc,
            idempotency_cache: state.idempotency_cache,
            idempotency_by_correlation: BTreeMap::new(),
            pending_ack_events: state.pending_ack_events,
            event_dedup: HashSet::new(),
            event_dedup_order: VecDeque::new(),
            capabilities: Some(capabilities),
            session_id: Some(session_id),
            pending_resume_session_id: None,
            last_sent_ms: BTreeMap::new(),
            last_recv_ms: BTreeMap::new(),
            pending_ping: BTreeMap::new(),
            state_entered_ms: now_ms,
            created_ms: now_ms,
        }
    }

    // -----------------------------------------------------------------------
    // Public read accessors
    // -----------------------------------------------------------------------

    /// Current session state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Active session ID (available after `SESSION_OFFER` is exchanged).
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Negotiated capabilities (available after `SESSION_READY`).
    pub fn capabilities(&self) -> Option<Capabilities> {
        self.capabilities
    }

    /// Export protocol state that must survive session resumption.
    pub fn export_resumable_state(&self) -> Option<ResumableState> {
        let session_id = self.session_id.clone()?;
        let capabilities = self.capabilities?;
        if !self.state.is_ready() {
            return None;
        }
        Some(ResumableState {
            session_id,
            capabilities,
            frame_id_counter: self.frame_id_counter,
            correlation_id_counter: self.correlation_id_counter,
            replay_window: self.replay_window.clone(),
            pending_rpc: self.pending_rpc.clone(),
            idempotency_cache: self.idempotency_cache.clone(),
            pending_ack_events: self.pending_ack_events.clone(),
        })
    }

    // -----------------------------------------------------------------------
    // Frame ID / Correlation ID allocation
    // -----------------------------------------------------------------------

    fn next_frame_id(&mut self) -> u32 {
        self.frame_id_counter = self.frame_id_counter.wrapping_add(1);
        if self.frame_id_counter == 0 {
            self.frame_id_counter = 1;
        }
        self.frame_id_counter
    }

    fn next_correlation_id(&mut self) -> u32 {
        self.correlation_id_counter = self.correlation_id_counter.wrapping_add(1);
        if self.correlation_id_counter == 0 {
            self.correlation_id_counter = 1;
        }
        self.correlation_id_counter
    }

    // -----------------------------------------------------------------------
    // Frame building helpers
    // -----------------------------------------------------------------------

    fn build_frame(&mut self, frame_type: u16, flags: Flags, corr: u32, payload: Payload) -> Frame {
        let id = self.next_frame_id();
        Frame::new(frame_type, flags, id, corr, payload)
    }

    fn build_error_frame(&mut self, err: &AxonError) -> Frame {
        let payload = Payload::Error(ErrorPayload {
            code: err.code.as_u16(),
            message: err.message.clone(),
            fatal: err.fatal,
            ref_frame_id: err.ref_frame_id,
        });
        self.build_frame(ty::ERROR, Flags::empty(), 0, payload)
    }

    fn build_pong(&mut self, ping: &PingPayload) -> Frame {
        let payload = Payload::Pong(PongPayload {
            nonce: ping.nonce.clone(),
            timestamp_ms: None,
        });
        self.build_frame(ty::PONG, Flags::empty(), 0, payload)
    }

    fn build_event_ack(&mut self, event_frame_id: u32) -> Frame {
        let id = self.next_frame_id();
        Frame::new(
            ty::EVENT_ACK,
            Flags::empty(),
            id,
            event_frame_id,
            Payload::EventAck,
        )
    }

    // -----------------------------------------------------------------------
    // Replay-window management
    // -----------------------------------------------------------------------

    fn check_and_record_frame_id(&mut self, frame_id: u32) -> Result<(), AxonError> {
        if frame_id == 0 {
            return Err(AxonError::invalid_frame("frame_id must not be zero", true));
        }
        if self.replay_window.contains(&frame_id) {
            return Err(
                AxonError::invalid_frame("duplicate frame_id inside replay window", true)
                    .with_ref_frame_id(frame_id),
            );
        }
        if self.replay_window.len() >= REPLAY_WINDOW_SIZE {
            self.replay_window.pop_front();
        }
        self.replay_window.push_back(frame_id);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // State transition
    // -----------------------------------------------------------------------

    fn transition(&mut self, new_state: SessionState, now_ms: u64) -> EngineAction {
        self.state = new_state;
        self.state_entered_ms = now_ms;
        EngineAction::StateTransition(new_state)
    }

    // -----------------------------------------------------------------------
    // Fatal error helpers (return SendFrame + Close)
    // -----------------------------------------------------------------------

    fn fatal_control_error(&mut self, err: AxonError) -> Vec<EngineAction> {
        let frame = self.build_error_frame(&err);
        self.state = SessionState::Closing;
        vec![
            EngineAction::SendFrame {
                frame,
                carrier: CarrierId::ControlPlane,
            },
            EngineAction::ProtocolError(err),
            EngineAction::Close,
        ]
    }

    fn non_fatal_error(&mut self, err: AxonError, carrier: CarrierId) -> Vec<EngineAction> {
        let frame = self.build_error_frame(&err);
        vec![
            EngineAction::SendFrame { frame, carrier },
            EngineAction::ProtocolError(err),
        ]
    }

    // -----------------------------------------------------------------------
    // Primary entry point: process one inbound frame
    // -----------------------------------------------------------------------

    /// Process one decoded inbound `frame` received on `carrier`.
    ///
    /// Returns the list of actions the caller must execute in order.
    pub fn process_inbound(
        &mut self,
        frame: Frame,
        carrier: CarrierId,
        now_ms: u64,
    ) -> Vec<EngineAction> {
        // 1. Validate header version (already done by decode_frame, but verify state-machine
        //    expectations explicitly).
        if let Err(e) = self.validate_inbound_frame(&frame, carrier) {
            if e.fatal {
                return self.fatal_control_error(e);
            }
            return self.non_fatal_error(e, carrier);
        }

        // 2. Record frame_id for replay protection.
        if let Err(e) = self.check_and_record_frame_id(frame.header.frame_id) {
            return self.fatal_control_error(e);
        }

        // 3. Update last-received timestamp.
        self.last_recv_ms.insert(carrier, now_ms);

        // 4. Dispatch to state handler.
        match self.state {
            SessionState::AwaitHello => self.handle_await_hello(frame, now_ms),
            SessionState::AwaitSessionOffer => self.handle_await_session_offer(frame, now_ms),
            SessionState::AwaitSessionAccept => self.handle_await_session_accept(frame, now_ms),
            SessionState::NegotiatingPipeline => {
                self.handle_negotiating_pipeline(frame, carrier, now_ms)
            }
            SessionState::AwaitSessionReady => self.handle_await_session_ready(frame, now_ms),
            SessionState::Ready | SessionState::RecoveringPipeline => {
                self.handle_ready(frame, carrier, now_ms)
            }
            SessionState::Closing => self.handle_closing(frame),
        }
    }

    // -----------------------------------------------------------------------
    // State handlers
    // -----------------------------------------------------------------------

    fn validate_inbound_frame(&self, frame: &Frame, _carrier: CarrierId) -> Result<(), AxonError> {
        let ft = frame.header.frame_type;

        if !self.state.is_frame_legal(ft) {
            return Err(AxonError::invalid_frame(
                format!(
                    "frame type 0x{ft:03X} is not legal in state {:?}",
                    self.state
                ),
                true,
            ));
        }
        if !self.config.role.is_inbound_direction_valid(ft) {
            return Err(AxonError::invalid_frame(
                format!(
                    "frame type 0x{ft:03X} direction is invalid for role {:?}",
                    self.config.role
                ),
                true,
            ));
        }
        Ok(())
    }

    fn handle_await_hello(&mut self, frame: Frame, now_ms: u64) -> Vec<EngineAction> {
        let Payload::Hello(hello) = frame.payload else {
            return self
                .fatal_control_error(AxonError::invalid_frame("expected HELLO payload", true));
        };

        if hello.client_version != PROTOCOL_VERSION {
            let e = AxonError::protocol_version(format!(
                "client_version {} is not supported; expected {}",
                hello.client_version, PROTOCOL_VERSION
            ));
            return self.fatal_control_error(e);
        }
        if !hello.supported_encodings.iter().any(|e| e == "msgpack") {
            let e = AxonError::invalid_frame(
                "HELLO.supported_encodings must include \"msgpack\"",
                true,
            );
            return self.fatal_control_error(e);
        }

        let transition = self.transition(SessionState::AwaitSessionAccept, now_ms);
        vec![
            transition,
            EngineAction::HelloReceived {
                client_version: hello.client_version,
                supported_encodings: hello.supported_encodings,
                resume_session_id: hello.resume_session_id,
            },
        ]
    }

    fn handle_await_session_offer(&mut self, frame: Frame, _now_ms: u64) -> Vec<EngineAction> {
        match frame.payload {
            Payload::SessionOffer(offer) => {
                if offer.server_version != PROTOCOL_VERSION {
                    let e = AxonError::protocol_version("server_version mismatch");
                    return self.fatal_control_error(e);
                }
                if offer.selected_encoding != "msgpack" {
                    let e = AxonError::invalid_frame("selected_encoding must be \"msgpack\"", true);
                    return self.fatal_control_error(e);
                }
                if Capabilities::from_bits(offer.capabilities).is_none() {
                    return self.fatal_control_error(AxonError::invalid_frame(
                        "capabilities field has reserved bits set",
                        true,
                    ));
                }
                if let Some(ref expected_id) = self.pending_resume_session_id
                    && offer.session_id != *expected_id
                {
                    return self.fatal_control_error(AxonError::invalid_frame(
                        "SESSION_OFFER.session_id does not match HELLO.resume_session_id",
                        true,
                    ));
                }
                self.session_id = Some(offer.session_id.clone());
                let scheme = offer
                    .auth_scheme
                    .clone()
                    .unwrap_or_else(|| String::from("none"));
                vec![EngineAction::SessionOfferReceived {
                    session_id: offer.session_id,
                    capabilities: offer.capabilities,
                    auth_scheme: scheme,
                }]
            }
            Payload::Error(e) => self.handle_received_error(e),
            Payload::Goodbye(g) => self.handle_received_goodbye(g),
            _ => self.fatal_control_error(AxonError::invalid_frame(
                "unexpected frame in AwaitSessionOffer",
                true,
            )),
        }
    }

    fn handle_await_session_accept(&mut self, frame: Frame, now_ms: u64) -> Vec<EngineAction> {
        match frame.payload {
            Payload::SessionAccept(accept) => {
                if accept.client_version != PROTOCOL_VERSION {
                    let e =
                        AxonError::protocol_version("client_version mismatch in SESSION_ACCEPT");
                    return self.fatal_control_error(e);
                }
                // Verify session_id echo.
                if self.session_id.as_deref() != Some(accept.session_id.as_str()) {
                    let e = AxonError::invalid_frame(
                        "SESSION_ACCEPT.session_id does not match offered session_id",
                        true,
                    );
                    return self.fatal_control_error(e);
                }
                // Compute capability intersection.
                let Some(client_caps) = Capabilities::from_bits(accept.capabilities) else {
                    return self.fatal_control_error(AxonError::invalid_frame(
                        "capabilities field has reserved bits set",
                        true,
                    ));
                };
                let Ok(intersection) = self.config.capabilities.intersect(client_caps) else {
                    let e = AxonError::capability_mismatch(
                        "negotiated capability intersection is empty",
                    );
                    return self.fatal_control_error(e);
                };
                self.capabilities = Some(intersection);

                let scheme = self.config.auth_scheme.clone();
                let transition = self.transition(SessionState::NegotiatingPipeline, now_ms);
                vec![
                    transition,
                    EngineAction::AuthRequired {
                        session_id: accept.session_id,
                        auth_scheme: scheme,
                        auth_response: accept.auth_response,
                        capabilities: intersection.bits(),
                    },
                ]
            }
            Payload::Error(e) => self.handle_received_error(e),
            Payload::Goodbye(g) => self.handle_received_goodbye(g),
            _ => self.fatal_control_error(AxonError::invalid_frame(
                "unexpected frame in AwaitSessionAccept",
                true,
            )),
        }
    }

    fn handle_negotiating_pipeline(
        &mut self,
        frame: Frame,
        carrier: CarrierId,
        now_ms: u64,
    ) -> Vec<EngineAction> {
        match frame.payload {
            Payload::PipelineOffer(offer) => {
                vec![EngineAction::SdpOfferReceived { sdp: offer.sdp }]
            }
            Payload::PipelineAnswer(answer) => {
                vec![EngineAction::SdpAnswerReceived { sdp: answer.sdp }]
            }
            Payload::IceCandidate(ice) => vec![EngineAction::IceCandidateReceived {
                candidate: ice.candidate,
                sdp_mid: ice.sdp_mid,
                sdp_mline_index: ice.sdp_mline_index,
                username_fragment: ice.username_fragment,
            }],
            Payload::PipelineReady(ready) => {
                let transition = self.transition(SessionState::AwaitSessionReady, now_ms);
                vec![
                    transition,
                    EngineAction::PipelineReadyReceived {
                        transport: ready.transport,
                        data_channels: ready.data_channels,
                        media_tracks: ready.media_tracks,
                    },
                ]
            }
            Payload::Ping(ping) => {
                let pong = self.build_pong(&ping);
                vec![EngineAction::SendFrame {
                    frame: pong,
                    carrier,
                }]
            }
            Payload::Pong(_) => {
                self.pending_ping.remove(&carrier);
                vec![]
            }
            Payload::Error(e) => self.handle_received_error(e),
            Payload::Goodbye(g) => self.handle_received_goodbye(g),
            _ => self.fatal_control_error(AxonError::invalid_frame(
                "unexpected frame in NegotiatingPipeline",
                true,
            )),
        }
    }

    fn handle_await_session_ready(&mut self, frame: Frame, now_ms: u64) -> Vec<EngineAction> {
        match frame.payload {
            Payload::SessionReady(ready) => {
                let Some(caps) = Capabilities::from_bits(ready.capabilities) else {
                    return self.fatal_control_error(AxonError::invalid_frame(
                        "capabilities field has reserved bits set",
                        true,
                    ));
                };
                self.capabilities = Some(caps);
                self.session_id = Some(ready.session_id.clone());
                let info = SessionInfo {
                    session_id: ready.session_id,
                    capabilities: caps,
                    selected_transport: ready.selected_transport,
                    resumed: ready.resumed,
                };
                let transition = self.transition(SessionState::Ready, now_ms);
                vec![transition, EngineAction::SessionReady(info)]
            }
            Payload::IceCandidate(_) => vec![], // trickle ICE passthrough
            Payload::Ping(ping) => {
                let pong = self.build_pong(&ping);
                vec![EngineAction::SendFrame {
                    frame: pong,
                    carrier: CarrierId::ControlPlane,
                }]
            }
            Payload::Pong(_) => {
                self.pending_ping.remove(&CarrierId::ControlPlane);
                vec![]
            }
            Payload::Error(e) => self.handle_received_error(e),
            Payload::Goodbye(g) => self.handle_received_goodbye(g),
            _ => self.fatal_control_error(AxonError::invalid_frame(
                "unexpected frame in AwaitSessionReady",
                true,
            )),
        }
    }

    fn handle_ready(&mut self, frame: Frame, carrier: CarrierId, now_ms: u64) -> Vec<EngineAction> {
        match frame.payload {
            Payload::RpcRequest(req) => {
                self.on_rpc_request(req, frame.header.correlation_id, carrier)
            }
            Payload::RpcResponse(resp) => {
                self.on_rpc_response(resp, frame.header.correlation_id, frame.header.flags)
            }
            Payload::RpcError(err) => {
                self.on_rpc_error(err, frame.header.correlation_id, frame.header.flags)
            }
            Payload::Event(event) => {
                self.on_event(event, frame.header.frame_id, frame.header.flags, carrier)
            }
            Payload::EventAck => {
                self.pending_ack_events.remove(&frame.header.correlation_id);
                vec![]
            }
            Payload::Ping(ping) => {
                let pong = self.build_pong(&ping);
                vec![EngineAction::SendFrame {
                    frame: pong,
                    carrier,
                }]
            }
            Payload::Pong(_) => {
                self.pending_ping.remove(&carrier);
                vec![]
            }
            Payload::PipelineReady(ready) => {
                let transition = self.transition(SessionState::Ready, now_ms);
                vec![
                    transition,
                    EngineAction::PipelineRecovered {
                        transport: ready.transport,
                        data_channels: ready.data_channels,
                        media_tracks: ready.media_tracks,
                    },
                ]
            }
            Payload::PipelineOffer(offer) => {
                vec![EngineAction::SdpOfferReceived { sdp: offer.sdp }]
            }
            Payload::PipelineAnswer(answer) => {
                vec![EngineAction::SdpAnswerReceived { sdp: answer.sdp }]
            }
            Payload::IceCandidate(ice) => vec![EngineAction::IceCandidateReceived {
                candidate: ice.candidate,
                sdp_mid: ice.sdp_mid,
                sdp_mline_index: ice.sdp_mline_index,
                username_fragment: ice.username_fragment,
            }],
            Payload::Error(e) => self.handle_received_error(e),
            Payload::Goodbye(g) => self.handle_received_goodbye(g),
            _ => self.non_fatal_error(
                AxonError::invalid_frame("unexpected frame in Ready state", false),
                carrier,
            ),
        }
    }

    fn handle_closing(&mut self, frame: Frame) -> Vec<EngineAction> {
        match frame.payload {
            Payload::Goodbye(_) | Payload::Error(_) => vec![], // absorb duplicate close signals
            _ => vec![],
        }
    }

    // -----------------------------------------------------------------------
    // RPC handlers
    // -----------------------------------------------------------------------

    fn on_rpc_request(
        &mut self,
        req: RpcRequestPayload,
        correlation_id: u32,
        carrier: CarrierId,
    ) -> Vec<EngineAction> {
        // Verify RPC capability is negotiated.
        if !self
            .capabilities
            .map(|c| c.requires_rpc_channel())
            .unwrap_or(false)
        {
            return self.non_fatal_error(
                AxonError::channel_closed("RPC capability not negotiated"),
                carrier,
            );
        }
        // Reject the combination of expects_stream + idempotency_key.
        if req.expects_stream.unwrap_or(false) && req.idempotency_key.is_some() {
            let err_payload = Payload::RpcError(RpcErrorPayload {
                code: ErrorCode::InvalidFrame as i32,
                message: String::from("expects_stream and idempotency_key are mutually exclusive"),
                data: None,
            });
            let frame = self.build_frame(ty::RPC_ERROR, Flags::FIN, correlation_id, err_payload);
            return vec![EngineAction::SendFrame { frame, carrier }];
        }
        if let Some(key) = req.idempotency_key.clone() {
            let params_hash = Self::hash_rpc_params(&req.params);
            if let Some(record) = self.idempotency_cache.get(&key) {
                if record.method != req.method || record.params_hash != params_hash {
                    let payload = Payload::RpcError(RpcErrorPayload {
                        code: ErrorCode::InvalidFrame as i32,
                        message: String::from(
                            "idempotency_key reused with different method or params",
                        ),
                        data: None,
                    });
                    let frame =
                        self.build_frame(ty::RPC_ERROR, Flags::FIN, correlation_id, payload);
                    return vec![EngineAction::SendFrame { frame, carrier }];
                }
                return match record.result.clone() {
                    Some(IdempotencyResult::Success(result)) => {
                        let payload = Payload::RpcResponse(RpcResponsePayload { result });
                        let frame =
                            self.build_frame(ty::RPC_RESPONSE, Flags::FIN, correlation_id, payload);
                        vec![EngineAction::SendFrame { frame, carrier }]
                    }
                    Some(IdempotencyResult::Error {
                        code,
                        message,
                        data,
                    }) => {
                        let payload = Payload::RpcError(RpcErrorPayload {
                            code,
                            message,
                            data,
                        });
                        let frame =
                            self.build_frame(ty::RPC_ERROR, Flags::FIN, correlation_id, payload);
                        vec![EngineAction::SendFrame { frame, carrier }]
                    }
                    None => vec![],
                };
            }
            self.idempotency_cache.insert(
                key.clone(),
                IdempotencyRecord {
                    method: req.method.clone(),
                    params_hash,
                    result: None,
                    completed_at_ms: None,
                },
            );
            self.idempotency_by_correlation.insert(correlation_id, key);
        }
        vec![EngineAction::RpcInvoke {
            correlation_id,
            method: req.method,
            params: req.params,
            timeout_ms: req.timeout_ms.unwrap_or(0),
        }]
    }

    fn hash_rpc_params(params: &Option<rmpv::Value>) -> u64 {
        let encoded = rmp_serde::to_vec_named(params).unwrap_or_default();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        encoded.hash(&mut hasher);
        hasher.finish()
    }

    fn on_rpc_response(
        &mut self,
        resp: RpcResponsePayload,
        correlation_id: u32,
        flags: Flags,
    ) -> Vec<EngineAction> {
        let fin = flags.contains(Flags::FIN);
        if !self.pending_rpc.contains_key(&correlation_id) {
            // Unknown correlation — silently discard per spec.
            return vec![];
        }
        if fin {
            self.pending_rpc.remove(&correlation_id);
        }
        vec![EngineAction::RpcComplete {
            correlation_id,
            result: resp.result,
            fin,
        }]
    }

    fn on_rpc_error(
        &mut self,
        err: RpcErrorPayload,
        correlation_id: u32,
        _flags: Flags,
    ) -> Vec<EngineAction> {
        self.pending_rpc.remove(&correlation_id);
        vec![EngineAction::RpcFailed {
            correlation_id,
            code: err.code,
            message: err.message,
            data: err.data,
        }]
    }

    // -----------------------------------------------------------------------
    // Event handler
    // -----------------------------------------------------------------------

    fn on_event(
        &mut self,
        event: EventPayload,
        frame_id: u32,
        flags: Flags,
        carrier: CarrierId,
    ) -> Vec<EngineAction> {
        if !self
            .capabilities
            .map(|c| c.requires_events_channel())
            .unwrap_or(false)
        {
            return self.non_fatal_error(
                AxonError::channel_closed("Events capability not negotiated"),
                carrier,
            );
        }
        let ack_required = flags.contains(Flags::ACK_REQ);
        let dedup_key = (event.topic.clone(), event.seq);
        let is_duplicate = self.event_dedup.contains(&dedup_key);
        if !is_duplicate {
            if self.event_dedup.len() >= event::EVENT_DEDUP_LIMIT
                && let Some(old) = self.event_dedup_order.pop_front()
            {
                self.event_dedup.remove(&old);
            }
            self.event_dedup.insert(dedup_key.clone());
            self.event_dedup_order.push_back(dedup_key);
        }
        let mut actions = Vec::new();
        if !is_duplicate {
            actions.push(EngineAction::EventReceived {
                topic: event.topic,
                seq: event.seq,
                payload: event.payload,
                timestamp_ms: event.timestamp_ms,
                frame_id,
                ack_required,
            });
        }
        if ack_required {
            let ack = self.build_event_ack(frame_id);
            actions.push(EngineAction::SendFrame {
                frame: ack,
                carrier,
            });
        }
        actions
    }

    // -----------------------------------------------------------------------
    // Received ERROR / GOODBYE helpers
    // -----------------------------------------------------------------------

    fn handle_received_error(&mut self, err: ErrorPayload) -> Vec<EngineAction> {
        let axon_err = AxonError {
            code: ErrorCode::from_u16(err.code).unwrap_or(ErrorCode::Internal),
            message: err.message,
            fatal: err.fatal,
            ref_frame_id: err.ref_frame_id,
        };
        if axon_err.fatal {
            self.state = SessionState::Closing;
            vec![EngineAction::ProtocolError(axon_err), EngineAction::Close]
        } else {
            vec![EngineAction::ProtocolError(axon_err)]
        }
    }

    fn handle_received_goodbye(&mut self, _goodbye: GoodbyePayload) -> Vec<EngineAction> {
        // Echo GOODBYE then close.
        let echo_payload = Payload::Goodbye(GoodbyePayload::default());
        let echo = self.build_frame(ty::GOODBYE, Flags::empty(), 0, echo_payload);
        self.state = SessionState::Closing;
        vec![
            EngineAction::SendFrame {
                frame: echo,
                carrier: CarrierId::ControlPlane,
            },
            EngineAction::Close,
        ]
    }

    // -----------------------------------------------------------------------
    // Application callbacks (called after async auth/RPC completes)
    // -----------------------------------------------------------------------

    /// Called by the server after successful authentication.
    ///
    /// Builds and returns the `SESSION_READY` frame that should be sent to
    /// the client.
    pub fn auth_success(
        &mut self,
        session_id: String,
        selected_transport: String,
        now_ms: u64,
    ) -> Result<(Frame, Vec<EngineAction>), AxonError> {
        let caps = self
            .capabilities
            .ok_or_else(|| AxonError::internal("capabilities not set before auth_success", true))?;
        self.session_id = Some(session_id.clone());
        let payload = Payload::SessionReady(SessionReadyPayload {
            session_id,
            selected_transport,
            capabilities: caps.bits(),
            resumed: false,
        });
        let transition = self.transition(SessionState::Ready, now_ms);
        let frame = self.build_frame(ty::SESSION_READY, Flags::empty(), 0, payload);
        Ok((frame, vec![transition]))
    }

    /// Called by the server after authentication failure.
    ///
    /// Returns a fatal `ERROR` frame to send before closing.
    pub fn auth_failed(&mut self, reason: impl Into<String>) -> Frame {
        let err = AxonError::auth_failed(reason);
        self.state = SessionState::Closing;
        self.build_error_frame(&err)
    }

    /// Called by the server to complete an RPC invocation with a successful
    /// result.  Returns the `RPC_RESPONSE` frame to send.
    pub fn rpc_success(
        &mut self,
        correlation_id: u32,
        result: rmpv::Value,
        _carrier: CarrierId,
    ) -> Frame {
        if let Some(key) = self.idempotency_by_correlation.remove(&correlation_id)
            && let Some(record) = self.idempotency_cache.get_mut(&key)
        {
            record.result = Some(IdempotencyResult::Success(result.clone()));
            record.completed_at_ms = Some(self.created_ms);
        }
        let payload = Payload::RpcResponse(RpcResponsePayload { result });
        self.build_frame(ty::RPC_RESPONSE, Flags::FIN, correlation_id, payload)
    }

    /// Called by the server to fail an RPC invocation.  Returns the
    /// `RPC_ERROR` frame to send.
    pub fn rpc_error(
        &mut self,
        correlation_id: u32,
        code: i32,
        message: impl Into<String>,
        data: Option<rmpv::Value>,
    ) -> Frame {
        let message = message.into();
        if let Some(key) = self.idempotency_by_correlation.remove(&correlation_id)
            && let Some(record) = self.idempotency_cache.get_mut(&key)
        {
            record.result = Some(IdempotencyResult::Error {
                code,
                message: message.clone(),
                data: data.clone(),
            });
            record.completed_at_ms = Some(self.created_ms);
        }
        let payload = Payload::RpcError(RpcErrorPayload {
            code,
            message,
            data,
        });
        self.build_frame(ty::RPC_ERROR, Flags::FIN, correlation_id, payload)
    }

    /// Cancel a streaming in-flight RPC.
    ///
    /// Returns the `RPC_ERROR` frame with `ERR_CANCELLED` and `FIN` to send.
    pub fn cancel_rpc(&mut self, correlation_id: u32) -> Option<Frame> {
        let _rpc = self.pending_rpc.remove(&correlation_id)?;
        let payload = Payload::RpcError(RpcErrorPayload {
            code: ErrorCode::Cancelled as i32,
            message: String::from("cancelled by caller"),
            data: None,
        });
        Some(self.build_frame(ty::RPC_ERROR, Flags::FIN, correlation_id, payload))
    }

    // -----------------------------------------------------------------------
    // Outbound frame builders (client-side helpers)
    // -----------------------------------------------------------------------

    /// Build a `HELLO` frame to start the client handshake.
    ///
    /// After calling this, the client MUST send the frame over the Control
    /// Plane.
    pub fn build_hello(
        &mut self,
        resume_session_id: Option<String>,
        metadata: Option<BTreeMap<String, rmpv::Value>>,
    ) -> Frame {
        self.pending_resume_session_id = resume_session_id.clone();
        let payload = Payload::Hello(HelloPayload {
            client_version: PROTOCOL_VERSION,
            supported_encodings: vec![String::from("msgpack")],
            resume_session_id,
            metadata,
        });
        self.build_frame(ty::HELLO, Flags::empty(), 0, payload)
    }

    /// Build a `SESSION_ACCEPT` frame in response to a `SESSION_OFFER`.
    pub fn build_session_accept(
        &mut self,
        session_id: String,
        auth_response: Option<crate::frame::payload::AuthResponse>,
        metadata: Option<BTreeMap<String, rmpv::Value>>,
        now_ms: u64,
    ) -> Result<Frame, AxonError> {
        if self.state != SessionState::AwaitSessionOffer {
            return Err(AxonError::invalid_frame(
                "cannot send SESSION_ACCEPT before SESSION_OFFER",
                false,
            ));
        }
        let caps = self.config.capabilities;
        let payload = Payload::SessionAccept(SessionAcceptPayload {
            session_id,
            client_version: PROTOCOL_VERSION,
            selected_transport: String::from("webrtc"),
            capabilities: caps.bits(),
            auth_response,
            metadata,
        });
        self.transition(SessionState::NegotiatingPipeline, now_ms);
        Ok(self.build_frame(ty::SESSION_ACCEPT, Flags::empty(), 0, payload))
    }

    /// Build a `SESSION_OFFER` frame (server side).
    pub fn build_session_offer(&mut self, session_id: String) -> Frame {
        let caps = self.config.capabilities;
        let payload = Payload::SessionOffer(SessionOfferPayload {
            session_id: session_id.clone(),
            server_version: PROTOCOL_VERSION,
            selected_encoding: String::from("msgpack"),
            supported_transports: self.config.supported_transports.clone(),
            capabilities: caps.bits(),
            auth_scheme: Some(self.config.auth_scheme.clone()),
            auth_challenge: None,
        });
        self.session_id = Some(session_id);
        self.build_frame(ty::SESSION_OFFER, Flags::empty(), 0, payload)
    }

    /// Build a `PIPELINE_OFFER` frame with the supplied SDP offer string.
    pub fn build_pipeline_offer(&mut self, sdp: String) -> Frame {
        let payload = Payload::PipelineOffer(PipelineOfferPayload {
            sdp_type: String::from("offer"),
            sdp,
        });
        self.build_frame(ty::PIPELINE_OFFER, Flags::empty(), 0, payload)
    }

    /// Build a `PIPELINE_ANSWER` frame with the supplied SDP answer string.
    pub fn build_pipeline_answer(&mut self, sdp: String) -> Frame {
        let payload = Payload::PipelineAnswer(PipelineAnswerPayload {
            sdp_type: String::from("answer"),
            sdp,
        });
        self.build_frame(ty::PIPELINE_ANSWER, Flags::empty(), 0, payload)
    }

    /// Build an `ICE_CANDIDATE` frame.
    pub fn build_ice_candidate(&mut self, candidate: String, opts: IceCandidateOptions) -> Frame {
        let payload = Payload::IceCandidate(IceCandidatePayload {
            candidate,
            sdp_mid: opts.sdp_mid,
            sdp_mline_index: opts.sdp_mline_index,
            username_fragment: opts.username_fragment,
        });
        self.build_frame(ty::ICE_CANDIDATE, Flags::empty(), 0, payload)
    }

    /// Build a `PIPELINE_READY` frame (server side).
    pub fn build_pipeline_ready(
        &mut self,
        data_channels: Vec<String>,
        media_tracks: Option<Vec<String>>,
    ) -> Frame {
        let payload = Payload::PipelineReady(PipelineReadyPayload {
            transport: String::from("webrtc"),
            data_channels,
            media_tracks,
        });
        self.build_frame(ty::PIPELINE_READY, Flags::empty(), 0, payload)
    }

    /// Build a `GOODBYE` frame with an optional close reason.
    pub fn build_goodbye(&mut self, code: Option<u16>, reason: Option<String>) -> Frame {
        let payload = Payload::Goodbye(GoodbyePayload { code, reason });
        self.state = SessionState::Closing;
        self.build_frame(ty::GOODBYE, Flags::empty(), 0, payload)
    }

    /// Build an `RPC_REQUEST` frame and register the pending call.
    ///
    /// Returns `(correlation_id, frame)`.  The correlation ID is needed to
    /// match the response.
    pub fn build_rpc_request(
        &mut self,
        method: String,
        opts: RpcRequestOptions,
        carrier: CarrierId,
    ) -> Result<(u32, Frame), AxonError> {
        if !self.state.is_ready() {
            return Err(AxonError::invalid_frame(
                "cannot send RPC_REQUEST before SESSION_READY",
                false,
            ));
        }
        let corr = self.next_correlation_id();
        self.pending_rpc.insert(
            corr,
            PendingRpc {
                method: method.clone(),
                expects_stream: opts.expects_stream,
                deadline_ms: if opts.timeout_ms > 0 {
                    Some(self.created_ms + u64::from(opts.timeout_ms))
                } else {
                    None
                },
                idempotency_key: opts.idempotency_key.clone(),
                carrier,
            },
        );
        let payload = Payload::RpcRequest(RpcRequestPayload {
            method,
            params: opts.params,
            timeout_ms: if opts.timeout_ms > 0 {
                Some(opts.timeout_ms)
            } else {
                None
            },
            expects_stream: if opts.expects_stream {
                Some(true)
            } else {
                None
            },
            idempotency_key: opts.idempotency_key,
        });
        let frame = self.build_frame(ty::RPC_REQUEST, Flags::empty(), corr, payload);
        Ok((corr, frame))
    }

    /// Build an outbound `EVENT` frame.
    ///
    /// `seq` and `timestamp_ms` are provided by the caller; the application
    /// layer manages per-topic sequence numbering.
    #[allow(clippy::too_many_arguments)]
    pub fn build_event(
        &mut self,
        topic: String,
        payload: rmpv::Value,
        seq: u64,
        timestamp_ms: i64,
        ack_required: bool,
        carrier: CarrierId,
    ) -> Result<Frame, AxonError> {
        if !self.state.is_ready() {
            return Err(AxonError::invalid_frame(
                "cannot send EVENT before SESSION_READY",
                false,
            ));
        }
        if !self
            .capabilities
            .map(|c| c.requires_events_channel())
            .unwrap_or(false)
        {
            return Err(AxonError::channel_closed(
                "Events capability not negotiated",
            ));
        }
        let flags = if ack_required {
            Flags::ACK_REQ
        } else {
            Flags::empty()
        };
        let event_payload = Payload::Event(EventPayload {
            topic: topic.clone(),
            payload: payload.clone(),
            seq,
            timestamp_ms,
        });
        let frame = self.build_frame(ty::EVENT, flags, 0, event_payload);
        if ack_required {
            self.pending_ack_events.insert(
                frame.header.frame_id,
                PendingAckEvent {
                    topic,
                    payload,
                    seq,
                    timestamp_ms,
                    carrier,
                    original_frame_id: frame.header.frame_id,
                    send_count: 0,
                    next_retry_ms: self.created_ms + 5_000,
                },
            );
        }
        Ok(frame)
    }

    /// Inform the engine that a frame was sent on `carrier` at `now_ms`.
    ///
    /// The caller must call this after writing every outbound frame to keep
    /// keepalive timers accurate.
    pub fn record_frame_sent(&mut self, carrier: CarrierId, now_ms: u64) {
        self.last_sent_ms.insert(carrier, now_ms);
    }

    // -----------------------------------------------------------------------
    // Keepalive / tick
    // -----------------------------------------------------------------------

    /// Drive keepalive probes and deadline enforcement.
    ///
    /// Call this at regular intervals (e.g. every second).  `now_ms` is the
    /// current wall-clock time in Unix epoch milliseconds.
    pub fn tick(&mut self, now_ms: u64) -> Vec<EngineAction> {
        let mut actions = Vec::new();
        self.tick_handshake_deadlines(now_ms, &mut actions);
        if self.state != SessionState::Closing {
            self.tick_keepalive(now_ms, &mut actions);
        }
        self.tick_rpc_timeouts(now_ms, &mut actions);
        self.tick_event_retransmissions(now_ms, &mut actions);
        self.tick_idempotency_eviction(now_ms);
        actions
    }

    fn tick_handshake_deadlines(&mut self, now_ms: u64, actions: &mut Vec<EngineAction>) {
        let elapsed = now_ms.saturating_sub(self.state_entered_ms);
        let cfg = &self.config.session_config;
        let timeout = match (self.state, self.config.role) {
            (SessionState::AwaitHello, Role::Server) => Some(cfg.hello_timeout_ms),
            (SessionState::AwaitSessionOffer, Role::Client) => Some(cfg.session_offer_timeout_ms),
            (SessionState::AwaitSessionAccept, Role::Server) => Some(cfg.session_accept_timeout_ms),
            (SessionState::NegotiatingPipeline, _) => Some(cfg.pipeline_ready_timeout_ms),
            (SessionState::AwaitSessionReady, Role::Client) => {
                Some(cfg.session_ready_client_wait_ms)
            }
            _ => None,
        };
        if let Some(deadline_ms) = timeout
            && elapsed >= deadline_ms
        {
            let err = AxonError::timeout(format!(
                "handshake deadline expired in state {:?} ({}ms elapsed, {}ms allowed)",
                self.state, elapsed, deadline_ms
            ));
            actions.extend(self.fatal_control_error(err));
        }
    }

    fn active_carriers(&self) -> Vec<CarrierId> {
        if self.state.is_ready() {
            let mut carriers = vec![CarrierId::ControlPlane];
            if self
                .capabilities
                .map(|c| c.requires_rpc_channel())
                .unwrap_or(false)
            {
                carriers.push(CarrierId::RpcChannel);
            }
            if self
                .capabilities
                .map(|c| c.requires_events_channel())
                .unwrap_or(false)
            {
                carriers.push(CarrierId::EventsChannel);
            }
            carriers
        } else if self.state != SessionState::Closing {
            vec![CarrierId::ControlPlane]
        } else {
            vec![]
        }
    }

    fn tick_keepalive(&mut self, now_ms: u64, actions: &mut Vec<EngineAction>) {
        let interval = self.config.session_config.keepalive_interval_ms;
        let pong_timeout = self.config.session_config.pong_timeout_ms;

        for carrier in self.active_carriers() {
            let last_sent = self
                .last_sent_ms
                .get(&carrier)
                .copied()
                .unwrap_or(self.created_ms);

            // Check for PONG timeout.
            if let Some((_, sent_at)) = self.pending_ping.get(&carrier) {
                if now_ms.saturating_sub(*sent_at) >= pong_timeout {
                    self.pending_ping.remove(&carrier);
                    let err = if carrier == CarrierId::ControlPlane {
                        AxonError::timeout("PONG not received within 10 s on Control Plane")
                    } else {
                        AxonError::pipeline_failed(
                            "PONG not received within 10 s on Pipeline",
                            false,
                        )
                    };
                    actions.push(EngineAction::ProtocolError(err));
                }
                continue;
            }

            // Send PING if idle too long.
            if now_ms.saturating_sub(last_sent) >= interval {
                let nonce: Vec<u8> = (0..8).map(|i| (now_ms >> (i * 8)) as u8).collect();
                let ping_payload = PingPayload {
                    nonce: Some(nonce.clone()),
                    timestamp_ms: Some(now_ms as i64),
                };
                let frame =
                    self.build_frame(ty::PING, Flags::empty(), 0, Payload::Ping(ping_payload));
                self.pending_ping.insert(carrier, (nonce, now_ms));
                self.last_sent_ms.insert(carrier, now_ms);
                actions.push(EngineAction::SendFrame { frame, carrier });
            }
        }
    }

    fn tick_rpc_timeouts(&mut self, now_ms: u64, actions: &mut Vec<EngineAction>) {
        let expired: Vec<u32> = self
            .pending_rpc
            .iter()
            .filter_map(|(&corr, rpc)| rpc.deadline_ms.filter(|&dl| now_ms >= dl).map(|_| corr))
            .collect();

        for corr in expired {
            self.pending_rpc.remove(&corr);
            actions.push(EngineAction::RpcFailed {
                correlation_id: corr,
                code: ErrorCode::Timeout as i32,
                message: String::from("RPC timed out"),
                data: None,
            });
        }
    }

    fn tick_event_retransmissions(&mut self, now_ms: u64, actions: &mut Vec<EngineAction>) {
        let due: Vec<u32> = self
            .pending_ack_events
            .iter()
            .filter_map(|(&frame_id, pending)| {
                (now_ms >= pending.next_retry_ms).then_some(frame_id)
            })
            .collect();

        for frame_id in due {
            let Some(mut pending) = self.pending_ack_events.remove(&frame_id) else {
                continue;
            };
            if pending.send_count >= 3 {
                actions.push(EngineAction::EventDeliveryFailed {
                    topic: pending.topic,
                    seq: pending.seq,
                    original_frame_id: pending.original_frame_id,
                });
                continue;
            }

            pending.send_count += 1;
            let next_delay = match pending.send_count {
                1 => 10_000,
                2 | 3 => 20_000,
                _ => 20_000,
            };
            pending.next_retry_ms = now_ms + next_delay;
            let payload = Payload::Event(EventPayload {
                topic: pending.topic.clone(),
                payload: pending.payload.clone(),
                seq: pending.seq,
                timestamp_ms: pending.timestamp_ms,
            });
            let frame = self.build_frame(ty::EVENT, Flags::ACK_REQ, 0, payload);
            let carrier = pending.carrier;
            self.pending_ack_events
                .insert(frame.header.frame_id, pending);
            actions.push(EngineAction::SendFrame { frame, carrier });
        }
    }

    fn tick_idempotency_eviction(&mut self, now_ms: u64) {
        self.idempotency_cache.retain(|_, record| {
            record
                .completed_at_ms
                .is_none_or(|completed_at| now_ms <= completed_at + 60_000)
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Payload;

    fn make_hello_frame(engine: &mut AxonEngine) -> Frame {
        engine.build_hello(None, None)
    }

    #[test]
    fn client_starts_in_await_session_offer() {
        let engine = AxonEngine::new(EngineConfig::client(), 0);
        assert_eq!(engine.state(), SessionState::AwaitSessionOffer);
    }

    #[test]
    fn server_starts_in_await_hello() {
        let engine = AxonEngine::new(EngineConfig::server(), 0);
        assert_eq!(engine.state(), SessionState::AwaitHello);
    }

    #[test]
    fn server_processes_hello_transitions_to_await_session_accept() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        let mut client = AxonEngine::new(EngineConfig::client(), 0);

        let hello = make_hello_frame(&mut client);
        let actions = server.process_inbound(hello, CarrierId::ControlPlane, 100);

        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::StateTransition(SessionState::AwaitSessionAccept)
        )));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::HelloReceived { .. }))
        );
        assert_eq!(server.state(), SessionState::AwaitSessionAccept);
    }

    #[test]
    fn replay_window_rejects_duplicate_frame_id() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        let mut client = AxonEngine::new(EngineConfig::client(), 0);

        let hello = make_hello_frame(&mut client);
        // Duplicate: same frame object sent twice.
        let hello2 = hello.clone();

        let _ = server.process_inbound(hello, CarrierId::ControlPlane, 0);
        let actions = server.process_inbound(hello2, CarrierId::ControlPlane, 0);

        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));
    }

    #[test]
    fn illegal_frame_in_wrong_state_is_rejected() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        // Server is in AwaitHello; send SESSION_READY (illegal direction + state).
        let frame = Frame::new(
            ty::SESSION_READY,
            Flags::empty(),
            1,
            0,
            Payload::SessionReady(crate::frame::payload::SessionReadyPayload {
                session_id: String::from("x"),
                selected_transport: String::from("webrtc"),
                capabilities: 3,
                resumed: false,
            }),
        );
        let actions = server.process_inbound(frame, CarrierId::ControlPlane, 0);
        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));
    }

    #[test]
    fn frame_id_counter_skips_zero() {
        let mut engine = AxonEngine::new(EngineConfig::client(), 0);
        // Wind counter to max.
        engine.frame_id_counter = u32::MAX;
        let id = engine.next_frame_id();
        assert_eq!(id, 1, "counter must wrap and skip 0");
    }

    #[test]
    fn pipeline_offer_emits_semantic_action() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        server.state = SessionState::NegotiatingPipeline;
        let frame = Frame::new(
            ty::PIPELINE_OFFER,
            Flags::empty(),
            1,
            0,
            Payload::PipelineOffer(PipelineOfferPayload {
                sdp_type: String::from("offer"),
                sdp: String::from("v=0"),
            }),
        );

        let actions = server.process_inbound(frame, CarrierId::ControlPlane, 1);

        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::SdpOfferReceived { sdp } if sdp == "v=0"
        )));
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, EngineAction::SendFrame { .. }))
        );
    }

    #[test]
    fn pipeline_ready_during_negotiation_waits_for_session_ready() {
        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        client.state = SessionState::NegotiatingPipeline;
        let frame = Frame::new(
            ty::PIPELINE_READY,
            Flags::empty(),
            1,
            0,
            Payload::PipelineReady(PipelineReadyPayload {
                transport: String::from("webrtc"),
                data_channels: vec![String::from("axon.events")],
                media_tracks: None,
            }),
        );

        let actions = client.process_inbound(frame, CarrierId::ControlPlane, 1);

        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::StateTransition(SessionState::AwaitSessionReady)
        )));
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::PipelineReadyReceived { transport, .. } if transport == "webrtc"
        )));
    }

    #[test]
    fn pipeline_ready_during_recovery_reports_recovered() {
        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        client.state = SessionState::RecoveringPipeline;
        let frame = Frame::new(
            ty::PIPELINE_READY,
            Flags::empty(),
            1,
            0,
            Payload::PipelineReady(PipelineReadyPayload {
                transport: String::from("webrtc"),
                data_channels: vec![String::from("axon.rpc")],
                media_tracks: Some(vec![String::from("audio")]),
            }),
        );

        let actions = client.process_inbound(frame, CarrierId::ControlPlane, 1);

        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::StateTransition(SessionState::Ready)))
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::PipelineRecovered { transport, .. } if transport == "webrtc"
        )));
    }

    #[test]
    fn auth_success_returns_ready_transition() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        server.capabilities = Some(Capabilities::RPC);

        let (_frame, actions) = server
            .auth_success(String::from("s"), String::from("webrtc"), 10)
            .unwrap();

        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::StateTransition(SessionState::Ready)))
        );
    }

    #[test]
    fn session_offer_does_not_transition_until_accept_is_built() {
        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        let frame = Frame::new(
            ty::SESSION_OFFER,
            Flags::empty(),
            1,
            0,
            Payload::SessionOffer(SessionOfferPayload {
                session_id: String::from("s"),
                server_version: PROTOCOL_VERSION,
                selected_encoding: String::from("msgpack"),
                supported_transports: vec![String::from("webrtc")],
                capabilities: Capabilities::RPC.bits(),
                auth_scheme: Some(String::from("none")),
                auth_challenge: None,
            }),
        );

        let actions = client.process_inbound(frame, CarrierId::ControlPlane, 1);

        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::SessionOfferReceived { .. }))
        );
        assert_eq!(client.state(), SessionState::AwaitSessionOffer);
        let accept = client.build_session_accept(String::from("s"), None, None, 2);
        assert!(accept.is_ok());
        assert_eq!(client.state(), SessionState::NegotiatingPipeline);
    }

    #[test]
    fn resume_session_id_mismatch_is_fatal() {
        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        let _hello = client.build_hello(Some(String::from("abc")), None);
        let frame = Frame::new(
            ty::SESSION_OFFER,
            Flags::empty(),
            2,
            0,
            Payload::SessionOffer(SessionOfferPayload {
                session_id: String::from("xyz"),
                server_version: PROTOCOL_VERSION,
                selected_encoding: String::from("msgpack"),
                supported_transports: vec![String::from("webrtc")],
                capabilities: Capabilities::RPC.bits(),
                auth_scheme: Some(String::from("none")),
                auth_challenge: None,
            }),
        );

        let actions = client.process_inbound(frame, CarrierId::ControlPlane, 1);

        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::SendFrame {
                frame: Frame {
                    payload: Payload::Error(ErrorPayload { code, fatal: true, .. }),
                    ..
                },
                ..
            } if *code == ErrorCode::InvalidFrame.as_u16()
        )));
    }

    #[test]
    fn resume_session_id_match_and_absent_succeed() {
        for resume_id in [Some(String::from("abc")), None] {
            let mut client = AxonEngine::new(EngineConfig::client(), 0);
            let _hello = client.build_hello(resume_id, None);
            let frame = Frame::new(
                ty::SESSION_OFFER,
                Flags::empty(),
                2,
                0,
                Payload::SessionOffer(SessionOfferPayload {
                    session_id: String::from("abc"),
                    server_version: PROTOCOL_VERSION,
                    selected_encoding: String::from("msgpack"),
                    supported_transports: vec![String::from("webrtc")],
                    capabilities: Capabilities::RPC.bits(),
                    auth_scheme: Some(String::from("none")),
                    auth_challenge: None,
                }),
            );

            let actions = client.process_inbound(frame, CarrierId::ControlPlane, 1);

            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, EngineAction::SessionOfferReceived { .. }))
            );
        }
    }

    #[test]
    fn keepalive_runs_during_pipeline_negotiation() {
        let mut config = EngineConfig::client();
        config.session_config.pipeline_ready_timeout_ms = 60_000;
        let mut client = AxonEngine::new(config, 0);
        client.state = SessionState::NegotiatingPipeline;

        let actions = client.tick(31_000);

        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::SendFrame {
                carrier: CarrierId::ControlPlane,
                frame: Frame {
                    payload: Payload::Ping(_),
                    ..
                }
            }
        )));
    }

    #[test]
    fn handshake_deadline_expiry_is_fatal() {
        let mut server = AxonEngine::new(EngineConfig::server(), 0);
        let actions = server.tick(6_000);
        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));

        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        client.state = SessionState::AwaitSessionReady;
        let actions = client.tick(6_000);
        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));

        let mut client = AxonEngine::new(EngineConfig::client(), 0);
        client.state = SessionState::NegotiatingPipeline;
        let actions = client.tick(31_000);
        assert!(actions.iter().any(|a| matches!(a, EngineAction::Close)));
    }

    #[test]
    fn record_frame_sent_suppresses_idle_ping_until_interval_expires() {
        let mut engine = AxonEngine::new(EngineConfig::client(), 0);
        engine.state = SessionState::Ready;
        engine.capabilities = Some(Capabilities::RPC);
        engine.record_frame_sent(CarrierId::ControlPlane, 10_000);

        let actions = engine.tick(39_000);
        assert!(!actions.iter().any(|a| matches!(
            a,
            EngineAction::SendFrame {
                carrier: CarrierId::ControlPlane,
                frame: Frame {
                    payload: Payload::Ping(_),
                    ..
                }
            }
        )));

        let actions = engine.tick(40_000);
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::SendFrame {
                carrier: CarrierId::ControlPlane,
                frame: Frame {
                    payload: Payload::Ping(_),
                    ..
                }
            }
        )));
    }

    #[test]
    fn duplicate_event_is_acknowledged_but_not_delivered_twice() {
        let mut engine = AxonEngine::new(EngineConfig::server(), 0);
        engine.state = SessionState::Ready;
        engine.capabilities = Some(Capabilities::EVENTS);
        let event = EventPayload {
            topic: String::from("topic"),
            payload: rmpv::Value::from(1),
            seq: 7,
            timestamp_ms: 1,
        };
        let first = Frame::new(
            ty::EVENT,
            Flags::ACK_REQ,
            1,
            0,
            Payload::Event(event.clone()),
        );
        let second = Frame::new(ty::EVENT, Flags::ACK_REQ, 2, 0, Payload::Event(event));

        let first_actions = engine.process_inbound(first, CarrierId::EventsChannel, 1);
        let second_actions = engine.process_inbound(second, CarrierId::EventsChannel, 2);

        assert_eq!(
            first_actions
                .iter()
                .filter(|a| matches!(a, EngineAction::EventReceived { .. }))
                .count(),
            1
        );
        assert_eq!(
            second_actions
                .iter()
                .filter(|a| matches!(a, EngineAction::EventReceived { .. }))
                .count(),
            0
        );
        assert!(second_actions.iter().any(|a| matches!(
            a,
            EngineAction::SendFrame {
                frame: Frame {
                    payload: Payload::EventAck,
                    ..
                },
                ..
            }
        )));
    }

    #[test]
    fn ack_required_event_retransmits_then_fails() {
        let mut engine = AxonEngine::new(EngineConfig::client(), 0);
        engine.state = SessionState::Ready;
        engine.capabilities = Some(Capabilities::EVENTS);
        let frame = engine
            .build_event(
                String::from("topic"),
                rmpv::Value::from(1),
                1,
                0,
                true,
                CarrierId::EventsChannel,
            )
            .unwrap();

        let actions = engine.tick(5_000);
        assert!(actions
            .iter()
            .any(|a| matches!(a, EngineAction::SendFrame { frame: f, .. } if f.header.frame_id != frame.header.frame_id)));
        let actions = engine.tick(15_000);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::SendFrame { .. }))
        );
        let actions = engine.tick(35_000);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, EngineAction::SendFrame { .. }))
        );
        let actions = engine.tick(55_000);
        assert!(actions.iter().any(|a| matches!(
            a,
            EngineAction::EventDeliveryFailed {
                topic,
                seq: 1,
                original_frame_id,
            } if topic == "topic" && *original_frame_id == frame.header.frame_id
        )));
    }

    #[test]
    fn cancel_rpc_returns_cancelled_error() {
        let mut engine = AxonEngine::new(EngineConfig::client(), 0);
        engine.state = SessionState::Ready;
        engine.capabilities = Some(Capabilities::RPC);
        let opts = RpcRequestOptions {
            expects_stream: true,
            ..RpcRequestOptions::default()
        };
        let (corr, _frame) = engine
            .build_rpc_request(String::from("stream"), opts, CarrierId::RpcChannel)
            .unwrap();

        let frame = engine.cancel_rpc(corr).unwrap();

        assert!(matches!(
            frame.payload,
            Payload::RpcError(RpcErrorPayload { code, .. })
                if code == ErrorCode::Cancelled as i32
        ));
        assert!(frame.header.flags.contains(Flags::FIN));
    }

    #[test]
    fn duplicate_idempotency_key_is_not_invoked_twice() {
        let mut engine = AxonEngine::new(EngineConfig::server(), 0);
        engine.state = SessionState::Ready;
        engine.capabilities = Some(Capabilities::RPC);
        let payload = RpcRequestPayload {
            method: String::from("m"),
            params: Some(rmpv::Value::from(1)),
            timeout_ms: None,
            expects_stream: None,
            idempotency_key: Some(String::from("k")),
        };
        let first = Frame::new(
            ty::RPC_REQUEST,
            Flags::empty(),
            1,
            10,
            Payload::RpcRequest(payload.clone()),
        );
        let second = Frame::new(
            ty::RPC_REQUEST,
            Flags::empty(),
            2,
            11,
            Payload::RpcRequest(payload),
        );

        let first_actions = engine.process_inbound(first, CarrierId::RpcChannel, 1);
        let second_actions = engine.process_inbound(second, CarrierId::RpcChannel, 2);

        assert!(first_actions.iter().any(|a| matches!(
            a,
            EngineAction::RpcInvoke {
                correlation_id: 10,
                ..
            }
        )));
        assert!(
            !second_actions
                .iter()
                .any(|a| matches!(a, EngineAction::RpcInvoke { .. }))
        );
    }

    #[test]
    fn resumed_engine_continues_frame_counter() {
        let mut engine = AxonEngine::new(EngineConfig::client(), 0);
        engine.state = SessionState::Ready;
        engine.session_id = Some(String::from("s"));
        engine.capabilities = Some(Capabilities::RPC);
        let _goodbye = engine.build_goodbye(None, None);
        engine.state = SessionState::Ready;
        let state = engine.export_resumable_state().unwrap();

        let mut resumed = AxonEngine::new_resumed(EngineConfig::client(), state, 100);
        let frame = resumed.build_goodbye(None, None);

        assert_eq!(frame.header.frame_id, 2);
    }
}
