# Connection Lifecycle and Handshake

## Overview

Session establishment follows four phases:

```text
Client                                Server
  |                                     |
  |---- [1] Control Plane Connect ---->|
  |---- [2] HELLO -------------------->|
  |<--- [3] Session Offer -------------|
  |---- [4] Session Accept ----------->|
  |                                     |
  |  +-- [5] Pipeline Negotiation ---+  |
  |  |   (WebRTC signaling via CP)   |  |
  |  +-------------------------------+  |
  |                                     |
  |<== Pipeline established ==========>|
  |                                     |
  |  [Normal operation: RPC/Event/Media]|
```

## Session State Machine

Implementations MUST enforce the following session states for draft version 0.1. A frame received in a state where that frame is not legal MUST be rejected with `ERROR` code `ERR_INVALID_FRAME`. The sender MUST set `fatal` according to the error fatality rules in [Error Handling and Reconnection](06-error-handling-and-reconnection.md#error-fatality-rules).

| State | Entered When | Legal Inbound Frames | Exit Condition |
| ----- | ------------ | -------------------- | -------------- |
| `AWAIT_HELLO` | Server accepts a Control Plane transport connection. | `HELLO` | Valid `HELLO` received; server sends `SESSION_OFFER`. |
| `AWAIT_SESSION_OFFER` | Client sends `HELLO`. | `SESSION_OFFER`, `ERROR`, `GOODBYE` | Valid `SESSION_OFFER` received; client sends `SESSION_ACCEPT`. |
| `AWAIT_SESSION_ACCEPT` | Server sends `SESSION_OFFER`. | `SESSION_ACCEPT`, `ERROR`, `GOODBYE` | Valid `SESSION_ACCEPT` received and authentication succeeds. |
| `NEGOTIATING_PIPELINE` | Session accept succeeds. | `PIPELINE_OFFER`, `PIPELINE_ANSWER`, `ICE_CANDIDATE`, `PIPELINE_READY`, `PING`, `PONG`, `ERROR`, `GOODBYE` | Required WebRTC Data Channels are open and server sends `PIPELINE_READY`. |
| `AWAIT_SESSION_READY` | Client receives `PIPELINE_READY`. | `SESSION_READY`, `ICE_CANDIDATE`, `PING`, `PONG`, `ERROR`, `GOODBYE` | Client receives valid `SESSION_READY`. |
| `READY` | Both peers have observed `SESSION_READY`. | `RPC_REQUEST`, `RPC_RESPONSE`, `RPC_ERROR`, `EVENT`, `EVENT_ACK`, `PING`, `PONG`, `ERROR`, `GOODBYE`, `PIPELINE_OFFER`, `PIPELINE_ANSWER`, `ICE_CANDIDATE` | Teardown or transport loss. |
| `RECOVERING_PIPELINE` | Pipeline is lost while the Control Plane remains connected. | Same as `READY`, plus `PIPELINE_READY`; RPC and Event frames MUST be carried on the Control Plane. | Pipeline recovers and the session returns to `READY`, or the session is closed. |
| `CLOSING` | Peer sends or receives `GOODBYE`, or sends fatal `ERROR`. | `GOODBYE`, `ERROR` | Both transports are closed. |

Application RPC and Event traffic MUST NOT be sent before `SESSION_READY`. The only exception is protocol-defined signaling required to establish or recover the Pipeline.

The legal inbound frames above are additionally constrained by the frame direction rules in [Appendix A](09-appendices.md#appendix-a-frame-type-quick-reference). For example, during initial Pipeline negotiation the client sends `PIPELINE_OFFER` and the server sends `PIPELINE_ANSWER`.

## Phase 1: Control Plane Connection

The client opens a TCP or WebSocket connection to the configured Axon endpoint defined in [Control Plane Endpoint Binding](02-architecture.md#control-plane-endpoint-binding). This connection uses standard transport handshake procedures, such as a TCP three-way handshake or WebSocket HTTP Upgrade.

Upon successful transport connection, the client MUST send a `HELLO` frame within 5 seconds. If the timeout expires, the server MUST send `ERROR` with code `ERR_TIMEOUT` and `fatal` set to `true` when possible, then close the Control Plane connection.

The server MUST enter `AWAIT_HELLO` immediately after the Control Plane transport is established. The client MUST enter `AWAIT_SESSION_OFFER` immediately after sending `HELLO`.

## Phase 2: Session Offer

Upon receiving a valid `HELLO`, the server responds with a `SESSION_OFFER` frame containing:

- `session_id`: A server-generated, globally unique session identifier. UUID v4 is RECOMMENDED.
- `server_version`: The Axon protocol version selected by the server. In draft version 0.1 this MUST be `1`.
- `selected_encoding`: The structured payload encoding selected by the server. In draft version 0.1 this MUST be `"msgpack"`.
- `supported_transports`: Array of Pipeline transports the server supports. In draft version 0.1 this MUST be `["webrtc"]`.
- `capabilities`: Server feature flags.
- `auth_scheme`: Authentication scheme selected by the server. Defaults to `"none"` when absent.
- `auth_challenge`: Optional opaque bytes for challenge-response authentication.

If `HELLO.resume_session_id` is present, the server MUST treat the handshake as a resumption request for that exact session. The server MUST NOT silently create a new session in response to a resumption request. It MUST either:

- send `SESSION_OFFER.session_id` equal to `HELLO.resume_session_id` and continue the resumed handshake; or
- send fatal `ERROR` with code `ERR_SESSION_EXPIRED` when resumable state is unavailable; or
- send fatal `ERROR` with code `ERR_AUTH_FAILED` when the reconnecting principal does not match the original session principal.

If a client sent `HELLO.resume_session_id` and receives `SESSION_OFFER.session_id` with any other value, the client MUST send `ERROR` with code `ERR_INVALID_FRAME`, set `fatal` to `true`, and close the Control Plane.

If the server cannot select a compatible protocol version, it MUST send `ERROR` with code `ERR_PROTOCOL_VERSION`, set `fatal` to `true`, and close the Control Plane connection. If the server cannot select a compatible payload encoding, such as when `HELLO.supported_encodings` does not include `"msgpack"`, it MUST send `ERROR` with code `ERR_INVALID_FRAME`, set `fatal` to `true`, and close the Control Plane connection.

The server MUST send either `SESSION_OFFER` or a fatal `ERROR` within 5 seconds after receiving a syntactically valid `HELLO`. If the client does not receive `SESSION_OFFER` or `ERROR` within 5 seconds after sending `HELLO`, the client MUST send `ERROR` with code `ERR_TIMEOUT` and `fatal` set to `true` when possible, close the Control Plane, and treat this as Control Plane loss.

After sending `SESSION_OFFER`, the server MUST enter `AWAIT_SESSION_ACCEPT`.

## Phase 3: Session Accept

The client MUST respond with a `SESSION_ACCEPT` frame containing:

- `session_id`: Echo of the server-provided session ID.
- `client_version`: The Axon protocol version selected by the client. In draft version 0.1 this MUST be `1`.
- `selected_transport`: The Pipeline transport the client wishes to use. In draft version 0.1 this MUST be `"webrtc"`.
- `capabilities`: Client feature flags.
- `auth_response`: Response to `auth_challenge` if present, or bearer token when `auth_scheme` is `"bearer"`.
- `metadata`: Optional key-value map for application-defined session metadata, such as user agent or app version.

If authentication fails, the server MUST send `ERROR` with code `ERR_AUTH_FAILED`, set `fatal` to `true`, and close the Control Plane connection.

The client MUST send either `SESSION_ACCEPT` or a fatal `ERROR` within 5 seconds after receiving a syntactically valid `SESSION_OFFER`. If the server does not receive `SESSION_ACCEPT` or `ERROR` within 5 seconds after sending `SESSION_OFFER`, it MUST send `ERROR` with code `ERR_TIMEOUT` and `fatal` set to `true` when possible, then close the Control Plane.

After accepting `SESSION_ACCEPT`, both endpoints enter `NEGOTIATING_PIPELINE`.

## Phase 4: Pipeline Negotiation

In draft version 0.1 the selected Pipeline transport is WebRTC. The session MUST negotiate a peer connection via SDP exchange and ICE, carried as signaling messages over the established Control Plane.

### SDP Exchange

```text
Client                                Server
  |---- PIPELINE_OFFER (SDP offer) --->|
  |<--- PIPELINE_ANSWER (SDP answer) --|
```

The `PIPELINE_OFFER` frame carries a standard WebRTC SDP offer. The server MUST respond with a `PIPELINE_ANSWER` frame carrying the SDP answer.

After entering `NEGOTIATING_PIPELINE`, the client MUST send `PIPELINE_OFFER` within 5 seconds. If the server does not receive a valid `PIPELINE_OFFER` before this deadline, it MUST send `ERROR` with code `ERR_TIMEOUT`, set `fatal` to `true`, and close the session.

After receiving a valid `PIPELINE_OFFER`, the server MUST send either a valid `PIPELINE_ANSWER` or an `ERROR` with code `ERR_PIPELINE_FAILED` within 5 seconds. An `ERR_PIPELINE_FAILED` sent before `SESSION_READY` MUST set `fatal` to `true` and close the session. If the client does not receive `PIPELINE_ANSWER` or `ERROR` within 5 seconds after sending `PIPELINE_OFFER`, it MUST send `ERROR` with code `ERR_PIPELINE_FAILED`, set `fatal` to `true`, and close the session.

If the client receives an invalid `PIPELINE_ANSWER`, it MUST send `ERROR` with code `ERR_PIPELINE_FAILED`, set `fatal` to `true`, and close the session.

### ICE Candidate Exchange

Both peers MUST exchange ICE candidates via `ICE_CANDIDATE` frames over the Control Plane as they become available. Trickle ICE SHOULD be used.

The initial Pipeline MUST become ready within 30 seconds. For the server, this deadline starts when it accepts `SESSION_ACCEPT`. For the client, this deadline starts when it sends `SESSION_ACCEPT`. The Pipeline is ready when the WebRTC peer connection is connected and all required Axon Data Channels are open. If this deadline expires, or if the WebRTC peer connection enters `failed` or `closed` before `SESSION_READY`, the endpoint detecting the failure MUST send `ERROR` with code `ERR_PIPELINE_FAILED`, set `fatal` to `true`, and close the session. If both endpoints detect the failure, either endpoint MAY send the fatal `ERROR`; receiving a duplicate close after entering `CLOSING` MUST be ignored.

### Data Channel Setup

The client MUST create the required Axon Data Channels before creating the SDP offer, so the Data Channel descriptions are included in the initial WebRTC negotiation. The server MUST accept these channels only when their labels and reliability parameters match this specification.

Draft version 0.1 defines the following Data Channels:

| Channel Name | Required When | Ordered | Reliability | WebRTC `protocol` | Purpose |
| ------------ | ------------- | ------- | ----------- | ------------------ | ------- |
| `axon.rpc` | Negotiated capabilities include `CAP_RPC` | Yes | Reliable; `maxRetransmits` unset and `maxPacketLifeTime` unset | `axon.v1` | RPC request/response |
| `axon.events` | Negotiated capabilities include `CAP_EVENTS` | No | Partially reliable; `maxRetransmits = 3` and `maxPacketLifeTime` unset | `axon.v1` | Event streams |

The channels are bidirectional. The client MUST create each required Data Channel with WebRTC negotiated mode disabled, allowing the SCTP stream ID to be assigned by WebRTC. The client MUST NOT create `axon.rpc` or `axon.events` unless the corresponding capability is in the client/server capability intersection. If a peer opens an Axon-reserved Data Channel whose capability was not negotiated, the receiver MUST close that Data Channel; this does not by itself fail the session.

If a required Data Channel is missing, has the wrong label, has the wrong WebRTC `protocol`, uses different reliability parameters, or fails to open before the Pipeline readiness deadline, Pipeline negotiation fails with `ERR_PIPELINE_FAILED`.

Either peer MAY send RPC or Event frames on the corresponding channel once the session reaches `READY` and the corresponding capability is negotiated. If a peer receives an RPC or Event frame for a capability that was not negotiated, it MUST send `ERROR` with code `ERR_CHANNEL_CLOSED` and `fatal` set to `false`.

Additional application-defined channels MAY be opened with names prefixed `axon.app.`. Such channels MUST NOT carry Axon frames unless a future extension defines their semantics.

Once the required Axon Data Channels are open, the server MUST send `PIPELINE_READY` over the Control Plane. The `data_channels` array MUST list the established Axon Data Channels that correspond to negotiated capabilities, sorted in ascending lexicographic order. If no Data Channel capability is negotiated, the server MUST send an empty `data_channels` array. The client MUST then enter `AWAIT_SESSION_READY`.

### Media Track Setup

Media tracks are negotiated as standard WebRTC `RTCPeerConnection` media tracks. Media capabilities indicate support for media; they do not require an active media track by themselves. The SDP offer/answer MUST include media descriptions only for active media tracks requested by the application.

## Session Ready

Once both the Control Plane handshake and Pipeline establishment are complete, the server MUST send a `SESSION_READY` frame over the Control Plane. `SESSION_READY` MUST be sent after `PIPELINE_READY`. The session is then fully operational.

After sending `SESSION_READY`, the server enters `READY`. After receiving a valid `SESSION_READY`, the client enters `READY`.

The server MUST send `SESSION_READY` within 1 second after sending `PIPELINE_READY`. If the client does not receive `SESSION_READY` within 5 seconds after receiving `PIPELINE_READY`, it MUST send `ERROR` with code `ERR_TIMEOUT` and `fatal` set to `true` when possible, close the Control Plane, and treat the session as failed.

## Session Teardown

Either peer MAY initiate graceful teardown by sending a `GOODBYE` frame with an optional reason string. The receiving peer MUST echo a `GOODBYE` within 1 second and then close both connections. Ungraceful teardown, such as connection drop, is handled by reconnection procedures.

After sending or receiving `GOODBYE`, an endpoint enters `CLOSING`. Endpoints SHOULD allow up to 5 seconds for the peer's `GOODBYE` before closing transports locally.
