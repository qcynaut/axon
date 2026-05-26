//! Payload structs for every Axon frame type.
//!
//! All structs are `serde`-derived for MessagePack encoding via `rmp-serde`.
//! String-keyed maps are used throughout, per spec §4 ("Payload Encoding").
//! Optional fields are omitted from the encoded map when absent.

use std::collections::BTreeMap;

use rmpv::Value as MsgpackValue;
use serde::{Deserialize, Serialize};

/// Zero-field helper used to encode / decode the mandatory empty-map payload
/// of `EVENT_ACK` frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAckEmptyMap;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// Authentication response sent in `SESSION_ACCEPT`.
///
/// The spec allows either a bearer token string or raw binary bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AuthResponse {
    /// Bearer token (UTF-8 string).
    Bearer(String),
    /// Raw binary credential (MessagePack `bin` type).
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

// ---------------------------------------------------------------------------
// Handshake frames
// ---------------------------------------------------------------------------

/// Payload for `HELLO` (C→S, `0x001`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    /// Axon protocol version selected by the client.  MUST be `1`.
    pub client_version: u8,
    /// Encodings supported by the client.  MUST include `"msgpack"`.
    pub supported_encodings: Vec<String>,
    /// Previous session ID the client wants to resume, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_session_id: Option<String>,
    /// Application-defined metadata (e.g., user-agent, app version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, MsgpackValue>>,
}

/// Payload for `SESSION_OFFER` (S→C, `0x002`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOfferPayload {
    /// Server-generated session identifier.
    pub session_id: String,
    /// Axon protocol version selected by the server.  MUST be `1`.
    pub server_version: u8,
    /// Encoding selected from `HELLO.supported_encodings`.  MUST be `"msgpack"`.
    pub selected_encoding: String,
    /// Pipeline transports the server supports.  MUST be `["webrtc"]` in v0.1.
    pub supported_transports: Vec<String>,
    /// Server capability bitmask (raw `u64`; interpret with [`Capabilities`]).
    ///
    /// [`Capabilities`]: crate::capability::Capabilities
    pub capabilities: u64,
    /// Authentication scheme selected by the server.  Defaults to `"none"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_scheme: Option<String>,
    /// Opaque challenge bytes for challenge-response authentication.
    #[serde(with = "serde_bytes", skip_serializing_if = "Option::is_none", default)]
    pub auth_challenge: Option<Vec<u8>>,
}

/// Payload for `SESSION_ACCEPT` (C→S, `0x003`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAcceptPayload {
    /// Echo of `SESSION_OFFER.session_id`.
    pub session_id: String,
    /// Axon protocol version selected by the client.  MUST match server's.
    pub client_version: u8,
    /// Selected Pipeline transport.  MUST be `"webrtc"` in v0.1.
    pub selected_transport: String,
    /// Client capability bitmask (raw `u64`).
    pub capabilities: u64,
    /// Authentication credential; required when `auth_scheme != "none"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_response: Option<AuthResponse>,
    /// Application-defined session metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, MsgpackValue>>,
}

/// Payload for `SESSION_READY` (S→C, `0x004`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReadyPayload {
    /// Active session ID.
    pub session_id: String,
    /// Active Pipeline transport.
    pub selected_transport: String,
    /// Negotiated capability intersection (raw `u64`).
    pub capabilities: u64,
    /// Whether this session resumed previous server-side state.
    pub resumed: bool,
}

/// Payload for `GOODBYE` (both, `0x005`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoodbyePayload {
    /// Application-defined close code.  `0` means normal closure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    /// Human-readable close reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Pipeline negotiation frames
// ---------------------------------------------------------------------------

/// Payload for `PIPELINE_OFFER` (C→S, `0x010`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOfferPayload {
    /// MUST be `"offer"`.
    pub sdp_type: String,
    /// WebRTC SDP offer string.
    pub sdp: String,
}

/// Payload for `PIPELINE_ANSWER` (S→C, `0x011`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineAnswerPayload {
    /// MUST be `"answer"`.
    pub sdp_type: String,
    /// WebRTC SDP answer string.
    pub sdp: String,
}

/// Payload for `ICE_CANDIDATE` (both, `0x012`).
///
/// An empty `candidate` string signals end-of-candidates for the referenced
/// media section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidatePayload {
    /// ICE candidate string (may be empty to signal end-of-candidates).
    pub candidate: String,
    /// SDP media stream identification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    /// SDP media line index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u16>,
    /// ICE username fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username_fragment: Option<String>,
}

/// Payload for `PIPELINE_READY` (S→C, `0x013`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineReadyPayload {
    /// MUST be `"webrtc"` in draft version 0.1.
    pub transport: String,
    /// Established Axon Data Channels in ascending lexicographic order.
    pub data_channels: Vec<String>,
    /// Established `MediaStreamTrack.id` values in ascending lexicographic order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_tracks: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// RPC frames
// ---------------------------------------------------------------------------

/// Payload for `RPC_REQUEST` (both, `0x020`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequestPayload {
    /// Fully-qualified method name, e.g. `"user.getProfile"`.
    pub method: String,
    /// Method parameters.  `None` indicates no parameters (treated as null).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<MsgpackValue>,
    /// Client-side timeout in milliseconds.  `0` means no timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    /// If `true`, the caller allows multiple `RPC_RESPONSE` frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expects_stream: Option<bool>,
    /// Application-generated key for safe retry deduplication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Payload for `RPC_RESPONSE` (both, `0x021`).
///
/// The `FIN` flag in the frame header marks the terminal frame in a streaming
/// response.  Non-streaming responses MUST have `FIN` set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponsePayload {
    /// Return value.  `Value::Nil` represents an explicit null return.
    pub result: MsgpackValue,
}

/// Payload for `RPC_ERROR` (both, `0x022`).
///
/// The `FIN` flag MUST be set.  `code` may be an Axon error code or an
/// application-defined code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorPayload {
    /// Error code (signed 32-bit, as required by spec §5).
    pub code: i32,
    /// Human-readable error description.
    pub message: String,
    /// Optional structured error detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<MsgpackValue>,
}

// ---------------------------------------------------------------------------
// Event frames
// ---------------------------------------------------------------------------

/// Payload for `EVENT` (both, `0x030`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPayload {
    /// Hierarchical topic, e.g. `"chat.message"`.  Segments separated by `.`.
    pub topic: String,
    /// Event data.  `Value::Nil` for an explicit null payload.
    pub payload: MsgpackValue,
    /// Monotonically increasing sequence number per topic per publisher.
    pub seq: u64,
    /// Publisher wall-clock timestamp as Unix epoch milliseconds.
    pub timestamp_ms: i64,
}

// `EVENT_ACK` carries an empty map payload; it is represented as the
// `Payload::EventAck` unit variant in the Frame enum.

// ---------------------------------------------------------------------------
// Keepalive frames
// ---------------------------------------------------------------------------

/// Payload for `PING` (either plane, both, `0x040`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PingPayload {
    /// Opaque bytes echoed in the corresponding `PONG`.
    #[serde(with = "serde_bytes", skip_serializing_if = "Option::is_none", default)]
    pub nonce: Option<Vec<u8>>,
    /// Sender wall-clock timestamp as Unix epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<i64>,
}

/// Payload for `PONG` (either plane, both, `0x041`).
///
/// MUST echo `PingPayload.nonce` exactly when present.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PongPayload {
    /// Echoed nonce from the corresponding `PING`.
    #[serde(with = "serde_bytes", skip_serializing_if = "Option::is_none", default)]
    pub nonce: Option<Vec<u8>>,
    /// Sender wall-clock timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<i64>,
}

// ---------------------------------------------------------------------------
// Protocol error frame
// ---------------------------------------------------------------------------

/// Payload for `ERROR` (either plane, both, `0x0F0`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Protocol error code (unsigned 16-bit on the wire).
    pub code: u16,
    /// Human-readable description.
    pub message: String,
    /// If `true`, the sender closes the affected transport after this frame.
    pub fatal: bool,
    /// Frame ID that triggered this error, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_frame_id: Option<u32>,
}
