//! Output actions produced by the [`super::AxonEngine`] state machine.

use rmpv::Value as MsgpackValue;

use crate::{
    error::AxonError,
    frame::Frame,
    session::{SessionInfo, SessionState},
};

/// Identifies which transport carrier a frame was received on or should be
/// sent on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CarrierId {
    /// The Control Plane (TCP or WebSocket).
    ControlPlane,
    /// The `axon.rpc` WebRTC Data Channel.
    RpcChannel,
    /// The `axon.events` WebRTC Data Channel.
    EventsChannel,
}

/// A pending RPC call tracked by the engine.
#[derive(Debug, Clone)]
pub struct PendingRpc {
    /// The method name.
    pub method: String,
    /// `true` when the caller opted into a server-streaming response.
    pub expects_stream: bool,
    /// Optional timeout deadline in wall-clock milliseconds.
    pub deadline_ms: Option<u64>,
    /// Idempotency key, when provided.
    pub idempotency_key: Option<String>,
    /// Carrier on which the request was last written.
    pub carrier: CarrierId,
}

/// An action the engine requires the caller to perform.
///
/// [`AxonEngine::process_inbound`] and [`AxonEngine::tick`] both return
/// `Vec<EngineAction>`.  The caller executes each action in order.
///
/// [`AxonEngine::process_inbound`]: super::AxonEngine::process_inbound
/// [`AxonEngine::tick`]: super::AxonEngine::tick
#[derive(Debug)]
#[non_exhaustive]
pub enum EngineAction {
    /// Send `frame` on `carrier`.
    SendFrame {
        /// The frame to transmit.
        frame: Frame,
        /// Which transport to use.
        carrier: CarrierId,
    },

    /// The session state has changed.
    StateTransition(SessionState),

    // -----------------------------------------------------------------------
    // Server-only actions (role = Server)
    // -----------------------------------------------------------------------
    /// A `HELLO` was received; the caller MUST call
    /// [`AxonEngine::accept_session`] to proceed.
    HelloReceived {
        /// Protocol version advertised by the client.
        client_version: u8,
        /// Encodings supported by the client.
        supported_encodings: Vec<String>,
        /// Session ID the client wishes to resume, if any.
        resume_session_id: Option<String>,
    },

    /// A `SESSION_ACCEPT` was received; the caller MUST authenticate the
    /// credentials and then call [`AxonEngine::auth_success`] or
    /// [`AxonEngine::auth_failed`].
    AuthRequired {
        /// Session ID echoed by the client.
        session_id: String,
        /// Effective authentication scheme.
        auth_scheme: String,
        /// Authentication credential provided by the client.
        auth_response: Option<crate::frame::payload::AuthResponse>,
        /// Raw capability bitmask advertised by the client.
        capabilities: u64,
    },

    /// An RPC call arrived; the caller MUST call [`AxonEngine::rpc_success`]
    /// or [`AxonEngine::rpc_error`] with the matching `correlation_id`.
    RpcInvoke {
        /// Correlation ID for this call.
        correlation_id: u32,
        /// Method name.
        method: String,
        /// Method parameters.
        params: Option<MsgpackValue>,
        /// Client-side timeout, milliseconds (0 = none).
        timeout_ms: u32,
    },

    // -----------------------------------------------------------------------
    // Client-only actions (role = Client)
    // -----------------------------------------------------------------------
    /// A `SESSION_OFFER` was received; the caller SHOULD inspect the offer
    /// then call [`AxonEngine::accept_session`].
    SessionOfferReceived {
        /// The session ID assigned by the server.
        session_id: String,
        /// Server's capability bitmask.
        capabilities: u64,
        /// Authentication scheme selected by the server.
        auth_scheme: String,
    },

    /// An RPC response completed successfully.
    RpcComplete {
        /// Matching correlation ID.
        correlation_id: u32,
        /// Return value.
        result: MsgpackValue,
        /// `true` when this is the final frame in a streaming response.
        fin: bool,
    },

    /// An in-flight RPC failed.
    RpcFailed {
        /// Matching correlation ID.
        correlation_id: u32,
        /// Application or protocol error code.
        code: i32,
        /// Human-readable description.
        message: String,
        /// Optional structured detail.
        data: Option<MsgpackValue>,
    },

    /// Peer sent a WebRTC SDP offer; caller must feed it to the WebRTC stack.
    SdpOfferReceived {
        /// SDP offer body.
        sdp: String,
    },

    /// Peer sent a WebRTC SDP answer; caller must apply it to the WebRTC stack.
    SdpAnswerReceived {
        /// SDP answer body.
        sdp: String,
    },

    /// Peer sent an ICE candidate; caller must add it to the WebRTC peer connection.
    IceCandidateReceived {
        /// ICE candidate string.
        candidate: String,
        /// SDP media stream identification.
        sdp_mid: Option<String>,
        /// SDP media line index.
        sdp_mline_index: Option<u16>,
        /// ICE username fragment.
        username_fragment: Option<String>,
    },

    /// Server sent `PIPELINE_READY`; caller must verify data channels and
    /// prepare for `SESSION_READY`.
    PipelineReadyReceived {
        /// Active transport, `"webrtc"` in draft version 0.1.
        transport: String,
        /// Established Axon Data Channels.
        data_channels: Vec<String>,
        /// Established media tracks, when present.
        media_tracks: Option<Vec<String>>,
    },

    /// Pipeline has been recovered/renegotiated; caller must switch traffic
    /// back to Pipeline.
    PipelineRecovered {
        /// Active transport, `"webrtc"` in draft version 0.1.
        transport: String,
        /// Established Axon Data Channels.
        data_channels: Vec<String>,
        /// Established media tracks, when present.
        media_tracks: Option<Vec<String>>,
    },

    // -----------------------------------------------------------------------
    // Both roles
    // -----------------------------------------------------------------------
    /// An event was received and MUST be delivered to the application.
    EventReceived {
        /// Publisher topic.
        topic: String,
        /// Sequence number (monotonically increasing per publisher+topic).
        seq: u64,
        /// Event data.
        payload: MsgpackValue,
        /// Publisher wall-clock timestamp (Unix epoch ms).
        timestamp_ms: i64,
        /// `Frame ID` of the `EVENT` frame — used to generate `EVENT_ACK`
        /// when the `ACK_REQ` flag was set.
        frame_id: u32,
        /// Whether the sender requires an acknowledgement.
        ack_required: bool,
    },

    /// All retransmissions were exhausted for an `EVENT` with `ACK_REQ`.
    /// The session remains open.
    EventDeliveryFailed {
        /// Event topic.
        topic: String,
        /// Event sequence number.
        seq: u64,
        /// Frame ID of the original `EVENT`.
        original_frame_id: u32,
    },

    /// The session is fully established and ready for application traffic.
    SessionReady(SessionInfo),

    /// A fatal or non-fatal protocol error was detected.  When `fatal` is
    /// `true` the engine has already queued a `SendFrame(ERROR, ...)` action;
    /// the caller MUST close the indicated carrier after draining the frame.
    ProtocolError(AxonError),

    /// The session is closing.  The caller MUST close all transports.
    Close,
}
