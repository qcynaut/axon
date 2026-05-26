//! `axon-core` — protocol types, codec, and Sans-I/O engine for the Axon protocol.
//!
//! # What this crate provides
//!
//! - **Wire types** ([`frame`]) — header, frame-type constants, flags, and every payload struct
//!   defined in the spec.
//! - **Codec** ([`codec`]) — pure `encode_frame` / `decode_frame` with no I/O.
//! - **Errors** ([`error`]) — [`AxonError`] with per-code fatality helpers.
//! - **Capabilities** ([`capability`]) — 64-bit bitmask + intersection logic.
//! - **Session state machine** ([`session`]) — the eight protocol states and all timeout defaults.
//! - **Sans-I/O engine** ([`engine`]) — [`AxonEngine`] drives the full handshake and
//!   application-traffic state machine; returns [`EngineAction`]s for the caller to act on.
//! - **Async trait contracts** ([`engine::traits`]) — [`ControlPlane`], [`PipelineDataChannel`],
//!   [`AxonServerHandler`], [`AxonClientSession`].
//!
//! # Design: Sans-I/O
//!
//! `axon-core` never performs I/O.  Callers feed decoded frames in via
//! [`AxonEngine::process_inbound`], receive [`EngineAction`]s back, and
//! execute those actions (send frames, invoke callbacks, etc.).  This makes
//! the engine testable without a network and usable with any async runtime.

pub mod capability;
pub mod codec;
pub mod engine;
pub mod error;
pub mod frame;
pub mod session;

// ---------------------------------------------------------------------------
// Convenience re-exports
// ---------------------------------------------------------------------------

pub use capability::Capabilities;
pub use codec::{decode_frame, encode_frame};
pub use engine::{
    AxonEngine, EngineConfig, IceCandidateOptions, IdempotencyRecord, IdempotencyResult,
    PendingAckEvent, ResumableState, RpcRequestOptions,
    action::{CarrierId, EngineAction, PendingRpc},
    traits::{AxonClientSession, AxonServerHandler, ControlPlane, PipelineDataChannel},
};
pub use error::{AxonError, ErrorCode};
pub use frame::{
    Flags, Frame, FrameHeader, HEADER_SIZE, MAX_PAYLOAD_LEN, PROTOCOL_VERSION, Payload,
    WIRE_VERSION, ty,
};
pub use session::{Role, SessionConfig, SessionInfo, SessionState};
