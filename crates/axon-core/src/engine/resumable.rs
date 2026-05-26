//! Exportable protocol state for session resumption.

use std::collections::{BTreeMap, VecDeque};

use super::{PendingRpc, event::PendingAckEvent, idempotency::IdempotencyRecord};
use crate::capability::Capabilities;

/// Protocol state that must be preserved across session resumption.
#[derive(Debug, Clone)]
pub struct ResumableState {
    /// Active session ID.
    pub session_id: String,
    /// Negotiated capabilities.
    pub capabilities: Capabilities,
    /// Outbound frame counter.
    pub frame_id_counter: u32,
    /// Outbound RPC correlation counter.
    pub correlation_id_counter: u32,
    /// Recent inbound frame IDs for replay protection.
    pub replay_window: VecDeque<u32>,
    /// In-flight RPC calls.
    pub pending_rpc: BTreeMap<u32, PendingRpc>,
    /// Server-side idempotency cache.
    pub idempotency_cache: BTreeMap<String, IdempotencyRecord>,
    /// Sender-side events waiting for acknowledgement.
    pub pending_ack_events: BTreeMap<u32, PendingAckEvent>,
}
