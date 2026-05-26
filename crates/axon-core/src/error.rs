//! Protocol-level error types.
//!
//! All Axon protocol errors are represented by [`AxonError`], which carries an
//! [`ErrorCode`], a human-readable message, a `fatal` flag (mirrors
//! `ERROR.fatal` on the wire), and an optional reference frame ID.
//!
//! See Axon spec §6 for the full error model and fatality rules.

use thiserror::Error;

/// Wire error codes defined by the Axon protocol (spec §6).
///
/// Codes `0x0100`–`0x7FFF` are reserved for future extensions.
/// Codes `0x8000`–`0xFFFF` are reserved for private deployment-specific errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ErrorCode {
    /// Incompatible protocol version (`0x0001`).
    ProtocolVersion = 0x0001,
    /// Authentication failed (`0x0002`).
    AuthFailed = 0x0002,
    /// Expected frame not received within the deadline (`0x0003`).
    Timeout = 0x0003,
    /// Malformed or unrecognized frame received (`0x0004`).
    InvalidFrame = 0x0004,
    /// Negotiated capability intersection is empty (`0x0005`).
    CapabilityMismatch = 0x0005,
    /// Pipeline negotiation or connection failed (`0x0006`).
    PipelineFailed = 0x0006,
    /// Referenced channel is closed or was not negotiated (`0x0007`).
    ChannelClosed = 0x0007,
    /// Payload or resource limit exceeded (`0x0008`).
    ResourceLimit = 0x0008,
    /// In-flight RPC was cancelled by the caller (`0x0009`).
    ///
    /// **MUST NOT** be sent in a protocol `ERROR` frame; only valid inside
    /// an `RPC_ERROR` payload.
    Cancelled = 0x0009,
    /// Previous session state is no longer available for resumption (`0x000A`).
    SessionExpired = 0x000A,
    /// Unspecified internal error (`0x00FF`).
    Internal = 0x00FF,
}

impl ErrorCode {
    /// Convert a raw `u16` to an [`ErrorCode`].
    ///
    /// Returns `None` for unrecognized codes.
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0001 => Some(Self::ProtocolVersion),
            0x0002 => Some(Self::AuthFailed),
            0x0003 => Some(Self::Timeout),
            0x0004 => Some(Self::InvalidFrame),
            0x0005 => Some(Self::CapabilityMismatch),
            0x0006 => Some(Self::PipelineFailed),
            0x0007 => Some(Self::ChannelClosed),
            0x0008 => Some(Self::ResourceLimit),
            0x0009 => Some(Self::Cancelled),
            0x000A => Some(Self::SessionExpired),
            0x00FF => Some(Self::Internal),
            _ => None,
        }
    }

    /// Return the wire representation of this code.
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// A protocol-level error.
///
/// `fatal` mirrors the `ERROR.fatal` wire field: when `true`, the sender
/// closes the affected transport immediately after writing the frame.
///
/// An `ERROR` sent on the **Control Plane** with `fatal = true` closes the
/// entire session. An `ERROR` sent on a **Pipeline Data Channel** with
/// `fatal = true` closes only the Pipeline; the session enters
/// `RECOVERING_PIPELINE` if it had already reached `READY`.
#[derive(Debug, Clone, Error)]
#[error("[{code:?}] {message}")]
pub struct AxonError {
    /// The protocol error code.
    pub code: ErrorCode,
    /// Human-readable description.
    pub message: String,
    /// Whether this error closes the affected transport.
    pub fatal: bool,
    /// Frame ID that triggered this error, when applicable.
    pub ref_frame_id: Option<u32>,
}

impl AxonError {
    /// Construct an [`AxonError`] with the given code, message, and fatality.
    pub fn new(code: ErrorCode, message: impl Into<String>, fatal: bool) -> Self {
        Self {
            code,
            message: message.into(),
            fatal,
            ref_frame_id: None,
        }
    }

    /// Attach the frame ID that triggered this error.
    #[must_use]
    pub fn with_ref_frame_id(mut self, id: u32) -> Self {
        self.ref_frame_id = Some(id);
        self
    }

    // ------------------------------------------------------------------
    // Convenience constructors — fatality matches the spec fatality table.
    // ------------------------------------------------------------------

    /// `ERR_PROTOCOL_VERSION` — always fatal.
    pub fn protocol_version(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ProtocolVersion, message, true)
    }

    /// `ERR_AUTH_FAILED` — always fatal.
    pub fn auth_failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::AuthFailed, message, true)
    }

    /// `ERR_TIMEOUT` — always fatal (before `SESSION_READY`).
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, message, true)
    }

    /// `ERR_INVALID_FRAME` — fatality depends on context.
    pub fn invalid_frame(message: impl Into<String>, fatal: bool) -> Self {
        Self::new(ErrorCode::InvalidFrame, message, fatal)
    }

    /// `ERR_CAPABILITY_MISMATCH` — always fatal.
    pub fn capability_mismatch(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CapabilityMismatch, message, true)
    }

    /// `ERR_PIPELINE_FAILED` — fatal before `SESSION_READY`, non-fatal after.
    pub fn pipeline_failed(message: impl Into<String>, fatal: bool) -> Self {
        Self::new(ErrorCode::PipelineFailed, message, fatal)
    }

    /// `ERR_CHANNEL_CLOSED` — never fatal (session remains open).
    pub fn channel_closed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ChannelClosed, message, false)
    }

    /// `ERR_RESOURCE_LIMIT` — always fatal.
    pub fn resource_limit(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ResourceLimit, message, true)
    }

    /// `ERR_CANCELLED` — non-fatal; only valid inside `RPC_ERROR`.
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Cancelled, message, false)
    }

    /// `ERR_SESSION_EXPIRED` — always fatal.
    pub fn session_expired(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::SessionExpired, message, true)
    }

    /// `ERR_INTERNAL` — fatality depends on whether continued operation is safe.
    pub fn internal(message: impl Into<String>, fatal: bool) -> Self {
        Self::new(ErrorCode::Internal, message, fatal)
    }
}
