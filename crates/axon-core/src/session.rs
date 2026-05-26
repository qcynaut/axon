//! Session state machine and configuration.
//!
//! [`SessionState`] encodes the eight states from spec §3 ("Session State
//! Machine").  [`SessionConfig`] holds all per-session timeout values.
//! [`SessionInfo`] carries the post-handshake session parameters.

use crate::{capability::Capabilities, frame::ty};

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// The eight session states defined by the Axon protocol (spec §3).
///
/// Implementations MUST enforce this machine and reject any frame that is not
/// in the "Legal Inbound Frames" set for the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Server has accepted a Control Plane transport connection.
    ///
    /// Legal inbound: `HELLO`.
    AwaitHello,

    /// Client has sent `HELLO` and is waiting for the server's offer.
    ///
    /// Legal inbound: `SESSION_OFFER`, `ERROR`, `GOODBYE`.
    AwaitSessionOffer,

    /// Server has sent `SESSION_OFFER` and is waiting for the client's accept.
    ///
    /// Legal inbound: `SESSION_ACCEPT`, `ERROR`, `GOODBYE`.
    AwaitSessionAccept,

    /// Both peers are performing WebRTC signaling over the Control Plane.
    ///
    /// Legal inbound: `PIPELINE_OFFER`, `PIPELINE_ANSWER`, `ICE_CANDIDATE`,
    /// `PIPELINE_READY`, `PING`, `PONG`, `ERROR`, `GOODBYE`.
    NegotiatingPipeline,

    /// Client has received `PIPELINE_READY` and is waiting for `SESSION_READY`.
    ///
    /// Legal inbound: `SESSION_READY`, `ICE_CANDIDATE`, `PING`, `PONG`,
    /// `ERROR`, `GOODBYE`.
    AwaitSessionReady,

    /// Session is fully operational.
    ///
    /// Legal inbound: `RPC_REQUEST`, `RPC_RESPONSE`, `RPC_ERROR`, `EVENT`,
    /// `EVENT_ACK`, `PING`, `PONG`, `ERROR`, `GOODBYE`, `PIPELINE_OFFER`,
    /// `PIPELINE_ANSWER`, `ICE_CANDIDATE`.
    Ready,

    /// Pipeline has been lost; RPC/Event traffic falls back to the Control
    /// Plane.
    ///
    /// Legal inbound: same as `Ready`, plus `PIPELINE_READY`.
    RecoveringPipeline,

    /// Graceful or ungraceful teardown is in progress.
    ///
    /// Legal inbound: `GOODBYE`, `ERROR`.
    Closing,
}

impl SessionState {
    /// Return `true` when `frame_type` is a legal inbound frame in this state.
    ///
    /// This encodes the "Legal Inbound Frames" column from the spec state
    /// machine table.  Direction (C→S vs S→C) is validated separately by the
    /// engine based on the session role.
    pub fn is_frame_legal(self, frame_type: u16) -> bool {
        match self {
            Self::AwaitHello => frame_type == ty::HELLO,

            Self::AwaitSessionOffer => {
                matches!(frame_type, ty::SESSION_OFFER | ty::ERROR | ty::GOODBYE)
            }

            Self::AwaitSessionAccept => {
                matches!(frame_type, ty::SESSION_ACCEPT | ty::ERROR | ty::GOODBYE)
            }

            Self::NegotiatingPipeline => matches!(
                frame_type,
                ty::PIPELINE_OFFER
                    | ty::PIPELINE_ANSWER
                    | ty::ICE_CANDIDATE
                    | ty::PIPELINE_READY
                    | ty::PING
                    | ty::PONG
                    | ty::ERROR
                    | ty::GOODBYE
            ),

            Self::AwaitSessionReady => matches!(
                frame_type,
                ty::SESSION_READY
                    | ty::ICE_CANDIDATE
                    | ty::PING
                    | ty::PONG
                    | ty::ERROR
                    | ty::GOODBYE
            ),

            Self::Ready => matches!(
                frame_type,
                ty::RPC_REQUEST
                    | ty::RPC_RESPONSE
                    | ty::RPC_ERROR
                    | ty::EVENT
                    | ty::EVENT_ACK
                    | ty::PING
                    | ty::PONG
                    | ty::ERROR
                    | ty::GOODBYE
                    | ty::PIPELINE_OFFER
                    | ty::PIPELINE_ANSWER
                    | ty::ICE_CANDIDATE
            ),

            // Same as Ready plus PIPELINE_READY.
            Self::RecoveringPipeline => {
                Self::Ready.is_frame_legal(frame_type) || frame_type == ty::PIPELINE_READY
            }

            Self::Closing => matches!(frame_type, ty::GOODBYE | ty::ERROR),
        }
    }

    /// Return `true` when this state allows application RPC/Event traffic.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready | Self::RecoveringPipeline)
    }
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// Whether this endpoint is the connecting client or the listening server.
///
/// Role affects which frames an endpoint is allowed to **send** and helps the
/// engine detect illegally directed inbound frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Initiating endpoint.  Starts by sending `HELLO`.
    Client,
    /// Accepting endpoint.  Starts in `AWAIT_HELLO`.
    Server,
}

impl Role {
    /// Return `true` when `frame_type` is a legal **inbound** frame direction
    /// for this role (i.e., the frame is sent by the peer).
    ///
    /// Frames marked "Both" in Appendix A are always permitted.
    pub fn is_inbound_direction_valid(self, frame_type: u16) -> bool {
        match frame_type {
            // Both directions
            ty::GOODBYE
            | ty::ICE_CANDIDATE
            | ty::RPC_REQUEST
            | ty::RPC_RESPONSE
            | ty::RPC_ERROR
            | ty::EVENT
            | ty::EVENT_ACK
            | ty::PING
            | ty::PONG
            | ty::ERROR => true,

            // S→C only — valid inbound for Client
            ty::SESSION_OFFER | ty::SESSION_READY | ty::PIPELINE_ANSWER | ty::PIPELINE_READY => {
                self == Role::Client
            }

            // C→S only — valid inbound for Server
            ty::HELLO | ty::SESSION_ACCEPT | ty::PIPELINE_OFFER => self == Role::Server,

            // Unknown frame type — rejected by decode_payload before here
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Session configuration
// ---------------------------------------------------------------------------

/// Per-session timeout configuration with spec-mandated defaults.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum ms the server waits for `HELLO` after transport connection
    /// (spec §3: 5 s).
    pub hello_timeout_ms: u64,

    /// Maximum ms after `HELLO` for the server to send `SESSION_OFFER` or
    /// `ERROR`; and for the client to receive it (spec §3: 5 s each).
    pub session_offer_timeout_ms: u64,

    /// Maximum ms after `SESSION_OFFER` for the client to send
    /// `SESSION_ACCEPT` (spec §3: 5 s).
    pub session_accept_timeout_ms: u64,

    /// Maximum ms after `SESSION_ACCEPT` for the initial Pipeline to become
    /// ready (spec §3: 30 s).
    pub pipeline_ready_timeout_ms: u64,

    /// Maximum ms after `PIPELINE_READY` for the server to send
    /// `SESSION_READY` (spec §3: 1 s); client waits up to 5 s.
    pub session_ready_server_send_ms: u64,

    /// Maximum ms the client waits for `SESSION_READY` after `PIPELINE_READY`
    /// (spec §3: 5 s).
    pub session_ready_client_wait_ms: u64,

    /// Keepalive idle interval per carrier (spec §6: 30 s).
    pub keepalive_interval_ms: u64,

    /// Maximum ms to wait for `PONG` after sending `PING` (spec §6: 10 s).
    pub pong_timeout_ms: u64,

    /// Initial backoff delay for Control Plane reconnection (spec §6: 500 ms).
    pub reconnect_initial_delay_ms: u64,

    /// Maximum backoff delay for Control Plane reconnection (spec §6: 30 s).
    pub reconnect_max_delay_ms: u64,

    /// Initial backoff delay for Pipeline recovery (spec §6: 1 s).
    pub pipeline_recover_initial_delay_ms: u64,

    /// Maximum backoff delay for Pipeline recovery (spec §6: 30 s).
    pub pipeline_recover_max_delay_ms: u64,

    /// Minimum duration (ms) a server MUST retain resumable session state
    /// after Control Plane loss (spec §6: 60 s).
    pub session_resume_state_ttl_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            hello_timeout_ms: 5_000,
            session_offer_timeout_ms: 5_000,
            session_accept_timeout_ms: 5_000,
            pipeline_ready_timeout_ms: 30_000,
            session_ready_server_send_ms: 1_000,
            session_ready_client_wait_ms: 5_000,
            keepalive_interval_ms: 30_000,
            pong_timeout_ms: 10_000,
            reconnect_initial_delay_ms: 500,
            reconnect_max_delay_ms: 30_000,
            pipeline_recover_initial_delay_ms: 1_000,
            pipeline_recover_max_delay_ms: 30_000,
            session_resume_state_ttl_ms: 60_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Session info
// ---------------------------------------------------------------------------

/// Parameters describing a fully established Axon session.
///
/// Available after the session reaches [`SessionState::Ready`].
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Active session identifier.
    pub session_id: String,
    /// Negotiated capability intersection.
    pub capabilities: Capabilities,
    /// Active Pipeline transport identifier (e.g. `"webrtc"`).
    pub selected_transport: String,
    /// Whether this session resumed previous server-side state.
    pub resumed: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::ty;

    #[test]
    fn await_hello_legal_frames() {
        assert!(SessionState::AwaitHello.is_frame_legal(ty::HELLO));
        assert!(!SessionState::AwaitHello.is_frame_legal(ty::SESSION_OFFER));
        assert!(!SessionState::AwaitHello.is_frame_legal(ty::PING));
    }

    #[test]
    fn negotiating_pipeline_legal_frames() {
        let state = SessionState::NegotiatingPipeline;
        assert!(state.is_frame_legal(ty::PIPELINE_OFFER));
        assert!(state.is_frame_legal(ty::ICE_CANDIDATE));
        assert!(state.is_frame_legal(ty::PING));
        assert!(!state.is_frame_legal(ty::RPC_REQUEST));
        assert!(!state.is_frame_legal(ty::HELLO));
    }

    #[test]
    fn ready_legal_frames() {
        let state = SessionState::Ready;
        assert!(state.is_frame_legal(ty::RPC_REQUEST));
        assert!(state.is_frame_legal(ty::EVENT));
        assert!(state.is_frame_legal(ty::PING));
        assert!(state.is_frame_legal(ty::GOODBYE));
        assert!(!state.is_frame_legal(ty::HELLO));
        assert!(!state.is_frame_legal(ty::SESSION_READY));
    }

    #[test]
    fn recovering_pipeline_has_pipeline_ready() {
        let state = SessionState::RecoveringPipeline;
        assert!(state.is_frame_legal(ty::PIPELINE_READY));
        assert!(state.is_frame_legal(ty::RPC_REQUEST));
    }

    #[test]
    fn closing_legal_frames() {
        let state = SessionState::Closing;
        assert!(state.is_frame_legal(ty::GOODBYE));
        assert!(state.is_frame_legal(ty::ERROR));
        assert!(!state.is_frame_legal(ty::PING));
    }

    #[test]
    fn role_direction_server_only() {
        assert!(Role::Server.is_inbound_direction_valid(ty::HELLO));
        assert!(!Role::Client.is_inbound_direction_valid(ty::HELLO));
    }

    #[test]
    fn role_direction_client_only() {
        assert!(Role::Client.is_inbound_direction_valid(ty::SESSION_OFFER));
        assert!(!Role::Server.is_inbound_direction_valid(ty::SESSION_OFFER));
    }

    #[test]
    fn role_direction_both() {
        assert!(Role::Client.is_inbound_direction_valid(ty::PING));
        assert!(Role::Server.is_inbound_direction_valid(ty::PING));
        assert!(Role::Client.is_inbound_direction_valid(ty::RPC_REQUEST));
        assert!(Role::Server.is_inbound_direction_valid(ty::RPC_REQUEST));
    }
}
