//! Pure frame encoding and decoding (no I/O).
//!
//! [`encode_frame`] serializes a [`Frame`] into a byte buffer.
//! [`decode_frame`] parses a complete Axon frame from a byte slice, returning
//! the frame and the number of bytes consumed.
//!
//! Both functions are `#[inline]`-able and have no async or I/O dependencies —
//! callers drive all I/O and call into these functions.

use bytes::{BufMut, BytesMut};

use crate::{
    error::AxonError,
    frame::{
        Frame, FrameHeader, HEADER_SIZE, MAX_PAYLOAD_LEN, Payload, WIRE_VERSION,
        payload::{
            ErrorPayload, EventAckEmptyMap, GoodbyePayload, HelloPayload, IceCandidatePayload,
            PingPayload, PipelineAnswerPayload, PipelineOfferPayload, PipelineReadyPayload,
            PongPayload, RpcErrorPayload, RpcRequestPayload, RpcResponsePayload,
            SessionAcceptPayload, SessionOfferPayload, SessionReadyPayload,
        },
        ty,
    },
};

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encode `frame` into `buf`.
///
/// The function serializes the payload to MessagePack first, then prepends the
/// 16-byte header with the correct `payload_len` field.
///
/// # Errors
///
/// Returns `ERR_RESOURCE_LIMIT` when the serialized payload exceeds 16 MiB.
/// Returns `ERR_INVALID_FRAME` when the payload cannot be serialized.
pub fn encode_frame(frame: &Frame, buf: &mut BytesMut) -> Result<(), AxonError> {
    let payload_bytes = encode_payload(&frame.payload)?;

    if payload_bytes.len() > MAX_PAYLOAD_LEN as usize {
        return Err(AxonError::resource_limit("encoded payload exceeds 16 MiB"));
    }

    let header = FrameHeader {
        payload_len: payload_bytes.len() as u32,
        ..frame.header
    };

    let mut header_bytes = [0u8; HEADER_SIZE];
    header.encode(&mut header_bytes);

    buf.put_slice(&header_bytes);
    buf.put_slice(&payload_bytes);

    Ok(())
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Attempt to decode one complete Axon frame from `src`.
///
/// Returns `(frame, bytes_consumed)` on success.  The caller MUST advance its
/// read cursor by `bytes_consumed`.
///
/// Returns `Err` if the buffer contains a complete-but-invalid frame.  Returns
/// `Ok(None)` is not returned — callers should check `src.len()` before
/// calling.  At minimum `src` must contain `HEADER_SIZE` bytes; the full frame
/// requires `HEADER_SIZE + payload_len` bytes.
///
/// # Errors
///
/// - `ERR_INVALID_FRAME` — malformed header, unknown frame type, invalid payload, zero `frame_id`,
///   unsupported header version, or invalid flags.
/// - `ERR_RESOURCE_LIMIT` — `payload_len` exceeds 16 MiB.
pub fn decode_frame(src: &[u8]) -> Result<(Frame, usize), AxonError> {
    if src.len() < HEADER_SIZE {
        return Err(AxonError::invalid_frame("incomplete frame header", true));
    }

    let header = FrameHeader::decode(&src[..HEADER_SIZE])?;
    validate_header(&header)?;

    let total = HEADER_SIZE + header.payload_len as usize;
    if src.len() < total {
        return Err(AxonError::invalid_frame("incomplete frame payload", true));
    }

    let payload_bytes = &src[HEADER_SIZE..total];
    let payload = decode_payload(header.frame_type, payload_bytes)?;

    Ok((Frame { header, payload }, total))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn validate_header(hdr: &FrameHeader) -> Result<(), AxonError> {
    if hdr.version != WIRE_VERSION {
        return Err(AxonError::protocol_version(format!(
            "unsupported wire version 0x{:X}; expected 0x{WIRE_VERSION:X}",
            hdr.version
        )));
    }
    if hdr.frame_id == 0 {
        return Err(AxonError::invalid_frame("frame_id must not be zero", true));
    }
    if hdr.payload_len > MAX_PAYLOAD_LEN {
        return Err(AxonError::resource_limit(format!(
            "payload_len {} exceeds the 16 MiB limit",
            hdr.payload_len
        )));
    }
    hdr.flags.validate_for(hdr.frame_type)?;
    Ok(())
}

/// Serialize a [`Payload`] to MessagePack with string-keyed maps.
fn encode_payload(payload: &Payload) -> Result<Vec<u8>, AxonError> {
    let bytes = match payload {
        Payload::Hello(p) => msgpack_encode(p),
        Payload::SessionOffer(p) => msgpack_encode(p),
        Payload::SessionAccept(p) => msgpack_encode(p),
        Payload::SessionReady(p) => msgpack_encode(p),
        Payload::Goodbye(p) => msgpack_encode(p),
        Payload::PipelineOffer(p) => msgpack_encode(p),
        Payload::PipelineAnswer(p) => msgpack_encode(p),
        Payload::IceCandidate(p) => msgpack_encode(p),
        Payload::PipelineReady(p) => msgpack_encode(p),
        Payload::RpcRequest(p) => msgpack_encode(p),
        Payload::RpcResponse(p) => msgpack_encode(p),
        Payload::RpcError(p) => msgpack_encode(p),
        Payload::Event(p) => msgpack_encode(p),
        Payload::EventAck => msgpack_encode(&EventAckEmptyMap),
        Payload::Ping(p) => msgpack_encode(p),
        Payload::Pong(p) => msgpack_encode(p),
        Payload::Error(p) => msgpack_encode(p),
    };
    bytes.map_err(|e| AxonError::invalid_frame(format!("payload encode error: {e}"), true))
}

fn msgpack_encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec_named(value)
}

/// Decode a MessagePack payload into the correct [`Payload`] variant.
fn decode_payload(frame_type: u16, bytes: &[u8]) -> Result<Payload, AxonError> {
    macro_rules! de {
        ($T:ty, $variant:ident) => {
            msgpack_decode::<$T>(bytes)
                .map(Payload::$variant)
                .map_err(|e| AxonError::invalid_frame(format!("payload decode error: {e}"), true))
        };
    }

    match frame_type {
        ty::HELLO => de!(HelloPayload, Hello),
        ty::SESSION_OFFER => de!(SessionOfferPayload, SessionOffer),
        ty::SESSION_ACCEPT => de!(SessionAcceptPayload, SessionAccept),
        ty::SESSION_READY => de!(SessionReadyPayload, SessionReady),
        ty::GOODBYE => de!(GoodbyePayload, Goodbye),
        ty::PIPELINE_OFFER => de!(PipelineOfferPayload, PipelineOffer),
        ty::PIPELINE_ANSWER => de!(PipelineAnswerPayload, PipelineAnswer),
        ty::ICE_CANDIDATE => de!(IceCandidatePayload, IceCandidate),
        ty::PIPELINE_READY => de!(PipelineReadyPayload, PipelineReady),
        ty::RPC_REQUEST => de!(RpcRequestPayload, RpcRequest),
        ty::RPC_RESPONSE => de!(RpcResponsePayload, RpcResponse),
        ty::RPC_ERROR => de!(RpcErrorPayload, RpcError),
        ty::EVENT => de!(crate::frame::payload::EventPayload, Event),
        ty::EVENT_ACK => {
            // Payload must be an empty map; we accept and discard it.
            msgpack_decode::<EventAckEmptyMap>(bytes)
                .map(|_| Payload::EventAck)
                .map_err(|e| {
                    AxonError::invalid_frame(format!("EVENT_ACK payload error: {e}"), true)
                })
        }
        ty::PING => de!(PingPayload, Ping),
        ty::PONG => de!(PongPayload, Pong),
        ty::ERROR => de!(ErrorPayload, Error),
        other => Err(AxonError::invalid_frame(
            format!("unknown frame type 0x{other:03X}"),
            true,
        )),
    }
}

fn msgpack_decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

// ---------------------------------------------------------------------------
// Tests — spec §10 test vectors
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode the full-frame hex from spec §10 and re-encode; assert
    /// round-trip byte equality.
    fn assert_vector_round_trip(hex: &str) {
        let bytes = hex::decode(hex.replace(' ', "")).expect("invalid hex");
        let (frame, consumed) = decode_frame(&bytes).expect("decode failed");
        assert_eq!(consumed, bytes.len(), "did not consume all bytes");

        let mut out = BytesMut::new();
        encode_frame(&frame, &mut out).expect("encode failed");
        assert_eq!(out.as_ref(), bytes.as_slice(), "round-trip mismatch");
    }

    #[test]
    fn vector_1_hello() {
        // Full frame hex from spec §10, Vector 1
        assert_vector_round_trip(
            "1001000000000001000000000000002e\
             82ae636c69656e745f76657273696f6e\
             01b3737570706f727465645f656e636f\
             64696e677391a76d73677061636b",
        );
    }

    #[test]
    fn vector_2_session_offer() {
        assert_vector_round_trip(
            "1002000000000001000000000000009886aa73657373696f6e5f6964d924303030303030\
             30302d303030302d343030302d383030302d303030303030303030303031ae736572766572\
             5f76657273696f6e01b173656c65637465645f656e636f64696e67a76d73677061636bb473\
             7570706f727465645f7472616e73706f72747391a6776562727463ac6361706162696c6974\
             69657303ab617574685f736368656d65a46e6f6e65",
        );
    }

    #[test]
    fn vector_3_ping_empty() {
        // Vector 3: PING with empty payload ({})
        assert_vector_round_trip("1040000000000002000000000000000180");
    }

    #[test]
    fn vector_4_rpc_request() {
        assert_vector_round_trip(
            "1020000000000003000000010000003c\
             84a66d6574686f64a9726f6f6d2e6a6f\
             696ea6706172616d7381a2696401aa74\
             696d656f75745f6d73cd1388ae657870\
             656374735f73747265616dc2",
        );
    }

    #[test]
    fn vector_5_rpc_response_fin() {
        assert_vector_round_trip(
            "1021000100000001000000010000001a\
             81a6726573756c7481a76d656d626572\
             7392a3616461a36c696e",
        );
    }

    #[test]
    fn vector_6_session_accept() {
        assert_vector_round_trip(
            "1003000000000002000000000000006a84aa73657373696f6e5f6964d924303030303030\
             30302d303030302d343030302d383030302d303030303030303030303031ae636c69656e74\
             5f76657273696f6e01b273656c65637465645f7472616e73706f7274a6776562727463ac63\
             61706162696c697469657303",
        );
    }

    #[test]
    fn vector_7_session_ready() {
        assert_vector_round_trip(
            "1004000000000004000000000000006384aa73657373696f6e5f6964d924303030303030\
             30302d303030302d343030302d383030302d303030303030303030303031b273656c656374\
             65645f7472616e73706f7274a6776562727463ac6361706162696c697469657303a7726573\
             756d6564c2",
        );
    }

    #[test]
    fn vector_8_error() {
        assert_vector_round_trip(
            "10f0000000000005000000000000002483a4636f646504a76d657373\
             616765ad696e76616c6964206672616d65a5666174616cc3",
        );
    }

    #[test]
    fn reject_zero_frame_id() {
        // Build a valid HELLO frame then zero out frame_id bytes (4..8).
        let hex = "1001000000000001000000000000002e\
                   82ae636c69656e745f76657273696f6e\
                   01b3737570706f727465645f656e636f\
                   64696e677391a76d73677061636b";
        let mut bytes = hex::decode(hex.replace(' ', "")).unwrap();
        bytes[4..8].copy_from_slice(&[0, 0, 0, 0]);
        assert!(decode_frame(&bytes).is_err());
    }

    #[test]
    fn reject_wrong_version() {
        let hex = "1001000000000001000000000000002e\
                   82ae636c69656e745f76657273696f6e\
                   01b3737570706f727465645f656e636f\
                   64696e677391a76d73677061636b";
        let mut bytes = hex::decode(hex.replace(' ', "")).unwrap();
        // Overwrite version nibble: set version to 0x2
        bytes[0] = (bytes[0] & 0x0F) | 0x20;
        assert!(decode_frame(&bytes).is_err());
    }

    #[test]
    fn reject_payload_too_large() {
        let hex = "1001000000000001000000000000002e\
                   82ae636c69656e745f76657273696f6e\
                   01b3737570706f727465645f656e636f\
                   64696e677391a76d73677061636b";
        let mut bytes = hex::decode(hex.replace(' ', "")).unwrap();
        // Set payload_len to MAX + 1 (bytes 12..16)
        let big: u32 = MAX_PAYLOAD_LEN + 1;
        bytes[12..16].copy_from_slice(&big.to_be_bytes());
        let err = decode_frame(&bytes).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::ResourceLimit);
    }
}
