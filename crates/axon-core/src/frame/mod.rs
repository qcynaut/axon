//! Wire frame types, header, flags, and the [`Frame`] envelope.
//!
//! Every Axon frame has a 16-byte fixed header followed by a variable-length
//! MessagePack payload.  This module defines the in-memory representations;
//! the [`crate::codec`] module handles encoding/decoding.

pub mod payload;

use bitflags::bitflags;

use crate::error::AxonError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Axon wire protocol version used in the 4-bit `Version` header field.
pub const WIRE_VERSION: u8 = 0x1;

/// Axon application protocol version sent in handshake payloads.
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum allowed payload length (16 MiB).
pub const MAX_PAYLOAD_LEN: u32 = 16 * 1024 * 1024;

/// Fixed-size Axon frame header in bytes.
pub const HEADER_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Frame type codes (spec §4, "Frame Types")
// ---------------------------------------------------------------------------

/// Type codes for all defined Axon frame types.
pub mod ty {
    /// Client greeting (Control Plane, C→S).
    pub const HELLO: u16 = 0x001;
    /// Server session proposal (Control Plane, S→C).
    pub const SESSION_OFFER: u16 = 0x002;
    /// Client session confirmation (Control Plane, C→S).
    pub const SESSION_ACCEPT: u16 = 0x003;
    /// Server confirms session is operational (Control Plane, S→C).
    pub const SESSION_READY: u16 = 0x004;
    /// Graceful teardown (Control Plane, both).
    pub const GOODBYE: u16 = 0x005;
    /// WebRTC SDP offer (Control Plane, C→S).
    pub const PIPELINE_OFFER: u16 = 0x010;
    /// WebRTC SDP answer (Control Plane, S→C).
    pub const PIPELINE_ANSWER: u16 = 0x011;
    /// WebRTC ICE candidate (Control Plane, both).
    pub const ICE_CANDIDATE: u16 = 0x012;
    /// Pipeline transport confirmed (Control Plane, S→C).
    pub const PIPELINE_READY: u16 = 0x013;
    /// Invoke a remote procedure (Pipeline/Control, both).
    pub const RPC_REQUEST: u16 = 0x020;
    /// Return value from RPC (Pipeline/Control, both).
    pub const RPC_RESPONSE: u16 = 0x021;
    /// RPC-level error (Pipeline/Control, both).
    pub const RPC_ERROR: u16 = 0x022;
    /// Publish an event (Pipeline/Control, both).
    pub const EVENT: u16 = 0x030;
    /// Acknowledge an event (Pipeline/Control, both).
    pub const EVENT_ACK: u16 = 0x031;
    /// Keepalive probe (either plane, both).
    pub const PING: u16 = 0x040;
    /// Keepalive response (either plane, both).
    pub const PONG: u16 = 0x041;
    /// Protocol-level error (either plane, both).
    pub const ERROR: u16 = 0x0F0;
}

// ---------------------------------------------------------------------------
// Flags (spec §4, "Flags")
// ---------------------------------------------------------------------------

bitflags! {
    /// 16-bit frame flags.
    ///
    /// Senders MUST NOT set a flag that is not applicable to the frame type.
    /// Bits 2–3 are reserved for future compression/encryption extensions and
    /// MUST be zero in draft version 0.1.  Bits 4–15 are reserved and MUST be
    /// zero.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Flags: u16 {
        /// Final frame in an RPC response stream.
        ///
        /// Applicable to: `RPC_RESPONSE`, `RPC_ERROR`.
        const FIN        = 1 << 0;
        /// Receiver MUST send `EVENT_ACK`.
        ///
        /// Applicable to: `EVENT`.
        const ACK_REQ    = 1 << 1;
        /// Reserved — compression extension (MUST be 0 in v0.1).
        const COMPRESSED = 1 << 2;
        /// Reserved — encryption extension (MUST be 0 in v0.1).
        const ENCRYPTED  = 1 << 3;
    }
}

impl Flags {
    /// Validate that only flags applicable to `frame_type` are set.
    ///
    /// Returns `Err(ERR_INVALID_FRAME)` on violation.
    pub fn validate_for(self, frame_type: u16) -> Result<(), AxonError> {
        // Reserved bits 2–15 must always be zero in draft 0.1.
        let reserved = self & (Self::COMPRESSED | Self::ENCRYPTED | Self::from_bits_retain(0xFFF0));
        if !reserved.is_empty() {
            return Err(AxonError::invalid_frame(
                "reserved flag bits must be zero in draft version 0.1",
                true,
            ));
        }

        // FIN is only valid on RPC_RESPONSE and RPC_ERROR.
        if self.contains(Self::FIN) && !matches!(frame_type, ty::RPC_RESPONSE | ty::RPC_ERROR) {
            return Err(AxonError::invalid_frame(
                "FIN flag is only valid on RPC_RESPONSE and RPC_ERROR",
                true,
            ));
        }

        // ACK_REQ is only valid on EVENT.
        if self.contains(Self::ACK_REQ) && frame_type != ty::EVENT {
            return Err(AxonError::invalid_frame(
                "ACK_REQ flag is only valid on EVENT",
                true,
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Frame header
// ---------------------------------------------------------------------------

/// Decoded 16-byte Axon frame header.
///
/// Wire layout (big-endian):
/// ```text
///  0               1               2               3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +───────────────────────────────────────────────────────────────────+
/// │ Version(4) │ Type(12)      │ Flags(16)                           │
/// +───────────────────────────────────────────────────────────────────+
/// │ Frame ID (32)                                                     │
/// +───────────────────────────────────────────────────────────────────+
/// │ Correlation ID (32)                                               │
/// +───────────────────────────────────────────────────────────────────+
/// │ Payload Length (32)                                               │
/// +───────────────────────────────────────────────────────────────────+
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// 4-bit wire version.  MUST be [`WIRE_VERSION`] (`0x1`).
    pub version: u8,
    /// 12-bit frame type code.  See [`ty`].
    pub frame_type: u16,
    /// 16-bit type-specific flags.
    pub flags: Flags,
    /// Monotonically increasing per-sender frame counter.
    ///
    /// Starts at `0x00000001`, wraps after `0xFFFFFFFF`, skips `0x00000000`.
    pub frame_id: u32,
    /// Links a response to its originating request.
    ///
    /// `0x00000000` for frames that are not part of a request/response pair.
    pub correlation_id: u32,
    /// Byte length of the payload that follows the fixed header.
    pub payload_len: u32,
}

impl FrameHeader {
    /// Encode the header into `dst`.
    ///
    /// `dst` must have at least [`HEADER_SIZE`] bytes available.
    pub fn encode(&self, dst: &mut [u8]) {
        debug_assert!(dst.len() >= HEADER_SIZE);
        let first_word: u16 = (u16::from(self.version) << 12) | (self.frame_type & 0x0FFF);
        dst[0..2].copy_from_slice(&first_word.to_be_bytes());
        dst[2..4].copy_from_slice(&self.flags.bits().to_be_bytes());
        dst[4..8].copy_from_slice(&self.frame_id.to_be_bytes());
        dst[8..12].copy_from_slice(&self.correlation_id.to_be_bytes());
        dst[12..16].copy_from_slice(&self.payload_len.to_be_bytes());
    }

    /// Decode a header from exactly [`HEADER_SIZE`] bytes.
    pub fn decode(src: &[u8]) -> Result<Self, AxonError> {
        if src.len() < HEADER_SIZE {
            return Err(AxonError::invalid_frame("incomplete frame header", true));
        }
        let first_word = u16::from_be_bytes([src[0], src[1]]);
        let version = (first_word >> 12) as u8;
        let frame_type = first_word & 0x0FFF;
        let flags_raw = u16::from_be_bytes([src[2], src[3]]);
        let flags = Flags::from_bits_retain(flags_raw);
        let frame_id = u32::from_be_bytes([src[4], src[5], src[6], src[7]]);
        let correlation_id = u32::from_be_bytes([src[8], src[9], src[10], src[11]]);
        let payload_len = u32::from_be_bytes([src[12], src[13], src[14], src[15]]);
        Ok(Self {
            version,
            frame_type,
            flags,
            frame_id,
            correlation_id,
            payload_len,
        })
    }
}

// ---------------------------------------------------------------------------
// Frame envelope
// ---------------------------------------------------------------------------

/// A fully parsed Axon frame: header + typed payload.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Decoded 16-byte header.
    pub header: FrameHeader,
    /// Typed payload.
    pub payload: Payload,
}

impl Frame {
    /// Construct a frame; `correlation_id` defaults to `0` when not relevant.
    pub fn new(
        frame_type: u16,
        flags: Flags,
        frame_id: u32,
        correlation_id: u32,
        payload: Payload,
    ) -> Self {
        Self {
            header: FrameHeader {
                version: WIRE_VERSION,
                frame_type,
                flags,
                frame_id,
                correlation_id,
                payload_len: 0, // filled in during encode
            },
            payload,
        }
    }
}

/// Typed payload variants, one per frame type.
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub enum Payload {
    Hello(payload::HelloPayload),
    SessionOffer(payload::SessionOfferPayload),
    SessionAccept(payload::SessionAcceptPayload),
    SessionReady(payload::SessionReadyPayload),
    Goodbye(payload::GoodbyePayload),
    PipelineOffer(payload::PipelineOfferPayload),
    PipelineAnswer(payload::PipelineAnswerPayload),
    IceCandidate(payload::IceCandidatePayload),
    PipelineReady(payload::PipelineReadyPayload),
    RpcRequest(payload::RpcRequestPayload),
    RpcResponse(payload::RpcResponsePayload),
    RpcError(payload::RpcErrorPayload),
    Event(payload::EventPayload),
    /// `EVENT_ACK` carries an empty map payload; the semantic data is in
    /// the frame header's `correlation_id`.
    EventAck,
    Ping(payload::PingPayload),
    Pong(payload::PongPayload),
    Error(payload::ErrorPayload),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let hdr = FrameHeader {
            version: WIRE_VERSION,
            frame_type: ty::HELLO,
            flags: Flags::empty(),
            frame_id: 1,
            correlation_id: 0,
            payload_len: 46,
        };
        let mut buf = [0u8; HEADER_SIZE];
        hdr.encode(&mut buf);
        let decoded = FrameHeader::decode(&buf).unwrap();
        assert_eq!(hdr, decoded);
    }

    #[test]
    fn hello_header_matches_spec_vector() {
        // Vector 1 from spec §10: first 4 bytes of full frame = 10 01 00 00
        let hdr = FrameHeader {
            version: 0x1,
            frame_type: ty::HELLO,
            flags: Flags::empty(),
            frame_id: 1,
            correlation_id: 0,
            payload_len: 46,
        };
        let mut buf = [0u8; HEADER_SIZE];
        hdr.encode(&mut buf);
        assert_eq!(buf[0], 0x10);
        assert_eq!(buf[1], 0x01);
        assert_eq!(buf[2], 0x00);
        assert_eq!(buf[3], 0x00);
    }

    #[test]
    fn fin_flag_valid_on_rpc_response() {
        assert!(Flags::FIN.validate_for(ty::RPC_RESPONSE).is_ok());
        assert!(Flags::FIN.validate_for(ty::RPC_ERROR).is_ok());
    }

    #[test]
    fn fin_flag_invalid_on_hello() {
        assert!(Flags::FIN.validate_for(ty::HELLO).is_err());
    }

    #[test]
    fn ack_req_valid_only_on_event() {
        assert!(Flags::ACK_REQ.validate_for(ty::EVENT).is_ok());
        assert!(Flags::ACK_REQ.validate_for(ty::RPC_REQUEST).is_err());
    }

    #[test]
    fn reserved_flags_rejected() {
        let bad = Flags::from_bits_retain(0x0004); // COMPRESSED bit
        assert!(bad.validate_for(ty::HELLO).is_err());
    }
}
