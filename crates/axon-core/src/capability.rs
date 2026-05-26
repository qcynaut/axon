//! Session capability negotiation.
//!
//! The `capabilities` field in `SESSION_OFFER` and `SESSION_ACCEPT` is a
//! 64-bit bitmask.  Both endpoints MUST compute their intersection; if the
//! intersection is empty the server MUST reject with `ERR_CAPABILITY_MISMATCH`
//! (spec §4, "Capabilities Field").

use bitflags::bitflags;

use crate::error::AxonError;

bitflags! {
    /// 64-bit capability bitmask (spec §4).
    ///
    /// Bits 4–63 are reserved and MUST be zero in draft version 0.1.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Capabilities: u64 {
        /// RPC channel support (`CAP_RPC`, bit 0).
        const RPC         = 1 << 0;
        /// Event channel support (`CAP_EVENTS`, bit 1).
        const EVENTS      = 1 << 1;
        /// Audio media track support (`CAP_MEDIA_AUDIO`, bit 2).
        const MEDIA_AUDIO = 1 << 2;
        /// Video media track support (`CAP_MEDIA_VIDEO`, bit 3).
        const MEDIA_VIDEO = 1 << 3;
    }
}

impl Capabilities {
    /// Compute the capability intersection of `self` and `other`.
    ///
    /// Returns `Err(ERR_CAPABILITY_MISMATCH)` when the intersection is empty,
    /// as required by spec §4.
    pub fn intersect(self, other: Self) -> Result<Self, AxonError> {
        let intersection = self & other;
        if intersection.is_empty() {
            Err(AxonError::capability_mismatch(
                "negotiated capability intersection is empty",
            ))
        } else {
            Ok(intersection)
        }
    }

    /// Whether the `axon.rpc` Data Channel is required by these capabilities.
    pub fn requires_rpc_channel(self) -> bool {
        self.contains(Self::RPC)
    }

    /// Whether the `axon.events` Data Channel is required by these capabilities.
    pub fn requires_events_channel(self) -> bool {
        self.contains(Self::EVENTS)
    }

    /// Whether any media track is required by these capabilities.
    pub fn requires_media(self) -> bool {
        self.intersects(Self::MEDIA_AUDIO | Self::MEDIA_VIDEO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_non_empty() {
        let a = Capabilities::RPC | Capabilities::EVENTS;
        let b = Capabilities::RPC | Capabilities::MEDIA_AUDIO;
        assert_eq!(a.intersect(b).unwrap(), Capabilities::RPC);
    }

    #[test]
    fn intersect_empty_is_error() {
        let a = Capabilities::RPC;
        let b = Capabilities::EVENTS;
        assert!(a.intersect(b).is_err());
    }

    #[test]
    fn requires_channels() {
        let caps = Capabilities::RPC | Capabilities::EVENTS;
        assert!(caps.requires_rpc_channel());
        assert!(caps.requires_events_channel());
        assert!(!caps.requires_media());
    }
}
