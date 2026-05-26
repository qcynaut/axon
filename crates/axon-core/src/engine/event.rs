//! Event deduplication and retransmission state.

use rmpv::Value as MsgpackValue;

use super::CarrierId;

/// Maximum number of inbound events remembered for duplicate suppression.
pub(crate) const EVENT_DEDUP_LIMIT: usize = 8192;

/// Sender-side state for an `EVENT` that requested acknowledgement.
#[derive(Debug, Clone)]
pub struct PendingAckEvent {
    /// Event topic.
    pub topic: String,
    /// Event payload.
    pub payload: MsgpackValue,
    /// Event sequence number.
    pub seq: u64,
    /// Publisher timestamp.
    pub timestamp_ms: i64,
    /// Carrier used for retransmission.
    pub carrier: CarrierId,
    /// Frame ID assigned to the original send.
    pub original_frame_id: u32,
    /// Number of retransmissions already attempted.
    pub send_count: u8,
    /// Wall-clock time for the next retry.
    pub next_retry_ms: u64,
}
