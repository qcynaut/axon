//! Server-side RPC idempotency cache types.

use rmpv::Value as MsgpackValue;

/// Cached responder state for one idempotency key.
#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    /// RPC method name.
    pub method: String,
    /// Hash of the MessagePack-encoded params.
    pub params_hash: u64,
    /// Cached terminal result, or `None` while the first request is in flight.
    pub result: Option<IdempotencyResult>,
    /// Completion timestamp for retention expiry.
    pub completed_at_ms: Option<u64>,
}

/// Cached terminal RPC result for an idempotent request.
#[derive(Debug, Clone)]
pub enum IdempotencyResult {
    /// Successful terminal result.
    Success(MsgpackValue),
    /// Terminal RPC error.
    Error {
        /// Error code.
        code: i32,
        /// Human-readable error description.
        message: String,
        /// Optional structured error data.
        data: Option<MsgpackValue>,
    },
}
