# Implementation Notes

## Server Implementation Checklist

A full conformant Axon server MUST:

- [ ] Encode and decode the 16-byte Axon frame header using network byte order.
- [ ] Require header `Version == 0x1` and handshake version fields equal to `1`.
- [ ] Validate frame flags, payload lengths, and required payload schema fields.
- [ ] Reject zero `Frame ID` values and duplicate `Frame ID` values still inside the replay window.
- [ ] Support MessagePack structured payloads.
- [ ] Enforce the session state machine and reject frames that are illegal in the current state.
- [ ] Accept TLS-wrapped TCP connections with Axon framing and TLS ALPN `axon/1`.
- [ ] Accept WebSocket connections on `/axon/v1` with `Sec-WebSocket-Protocol: axon.v1`.
- [ ] Require exactly one complete Axon frame per WebSocket binary message.
- [ ] Support WebRTC peer connection establishment via signaling on the Control Plane.
- [ ] Accept required `axon.rpc` and `axon.events` Data Channels according to the negotiated capabilities, labels, `protocol`, and reliability parameters.
- [ ] Support Opus audio when advertising `CAP_MEDIA_AUDIO` and VP8 video when advertising `CAP_MEDIA_VIDEO`.
- [ ] Validate that RPC frames are received on `axon.rpc` or the Control Plane fallback, and Event frames are received on `axon.events` or the Control Plane fallback.
- [ ] Enforce handshake, SDP answer, and Pipeline readiness deadlines.
- [ ] Apply the required `ERROR.fatal` value for each protocol error condition.
- [ ] Handle Control Plane fallback for RPC and Event traffic.
- [ ] Deduplicate retried RPC requests that include `idempotency_key`.
- [ ] Enforce Event `ACK_REQ` acknowledgement, retransmission, and duplicate-delivery rules.
- [ ] Track at least 4096 recent `Frame ID` values per sender for replay protection.
- [ ] Respond to `PING` frames with `PONG` within 10 seconds.
- [ ] Implement session resumption on Control Plane reconnect.
- [ ] Continue per-session `Frame ID` counters across successful resumption.

A constrained server MAY omit one Control Plane transport as described in [Architecture Overview](02-architecture.md), but all other applicable checklist items remain required.

## Client Implementation Checklist

A conformant Axon client MUST:

- [ ] Encode and decode the 16-byte Axon frame header using network byte order.
- [ ] Require header `Version == 0x1` and handshake version fields equal to `1`.
- [ ] Validate frame flags, payload lengths, and required payload schema fields.
- [ ] Reject zero `Frame ID` values and duplicate `Frame ID` values still inside the replay window.
- [ ] Support MessagePack structured payloads.
- [ ] Enforce the session state machine and reject frames that are illegal in the current state.
- [ ] Implement at least one Control Plane transport: TCP and/or WebSocket.
- [ ] Use TLS ALPN `axon/1` for TCP, or WebSocket path `/axon/v1` with `Sec-WebSocket-Protocol: axon.v1`.
- [ ] Implement the WebRTC Pipeline transport.
- [ ] Create the required `axon.rpc` and `axon.events` Data Channels before generating the SDP offer, according to negotiated capabilities and required reliability parameters.
- [ ] Support Opus audio when advertising `CAP_MEDIA_AUDIO` and VP8 video when advertising `CAP_MEDIA_VIDEO`.
- [ ] Send exactly one complete Axon frame per WebSocket or Data Channel binary message.
- [ ] Select a single active Control Plane transport and a single active Pipeline transport per session, negotiated against the server's `supported_transports` during the handshake.
- [ ] Perform session handshake within 5 seconds of transport connection.
- [ ] Enforce handshake, SDP answer, and Pipeline readiness deadlines.
- [ ] Apply the required `ERROR.fatal` value for each protocol error condition.
- [ ] Fall back to Control Plane when Pipeline is unavailable.
- [ ] Apply deterministic RPC retry behavior for non-streaming `idempotency_key` requests and fail non-retryable in-flight RPCs locally.
- [ ] Enforce Event `ACK_REQ` acknowledgement, retransmission, and duplicate-delivery rules.
- [ ] Implement exponential backoff for reconnection.
- [ ] Attempt session resumption with `HELLO.resume_session_id` after Control Plane loss, then establish a fresh session after `ERR_SESSION_EXPIRED`.
- [ ] Migrate RPC/Event traffic back to Pipeline upon recovery.
- [ ] Continue per-session `Frame ID` counters across successful resumption.
- [ ] Track keepalive independently for the Control Plane and each open Axon Data Channel.

## Versioning

The 4-bit `Version` field in the frame header allows values 1 through 15. Version `0x0` is reserved and invalid. Draft version 0.1 uses wire version `0x1` only.

Future protocol revisions that are not backward-compatible MUST increment the version number. Endpoints receiving a frame with an unrecognized version MUST send `ERROR` with code `ERR_PROTOCOL_VERSION`, set `fatal` to `true`, and close the affected transport. If the affected transport is the Control Plane, the endpoint MUST close the session.

## Interoperability Testing

Implementations SHOULD include tests that decode and encode the vectors in [Implementation Profile and Test Vectors](10-implementation-profile.md). Implementations SHOULD also include negative tests for:

- Unsupported header versions.
- Reserved flags.
- Zero `Frame ID` values.
- Duplicate `Frame ID` values still inside the replay window.
- Payloads larger than 16 MiB.
- Non-binary WebSocket and Data Channel messages.
- Multiple Axon frames in one WebSocket or Data Channel message.
- Frames sent in the wrong session state.
- RPC frames on `axon.events` and Event frames on `axon.rpc`.
- Missing, mislabeled, or incorrectly configured required Axon Data Channels.
- Handshake, SDP answer, Pipeline readiness, and `SESSION_READY` timeouts.
- RPC or Event frames sent when the corresponding capability was not negotiated.
- Resumption requests answered with a different `session_id`.
- Stale or unknown `EVENT_ACK` frames, which must be silently discarded.
