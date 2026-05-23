# Message Framing and Wire Format

## Frame Structure

All Axon frames share a common binary header:

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-----------------------------------------------------------------+
|  Version (4) |  Type (12)  |         Flags (16)                 |
+-----------------------------------------------------------------+
|                      Frame ID (32)                              |
+-----------------------------------------------------------------+
|                    Correlation ID (32)                          |
+-----------------------------------------------------------------+
|                     Payload Length (32)                         |
+-----------------------------------------------------------------+
|                    Payload (variable)                           |
+-----------------------------------------------------------------+
```

| Field | Size | Description |
| ----- | ---- | ----------- |
| `Version` | 4 bits | Axon protocol version. Current: `0x1`. |
| `Type` | 12 bits | Frame type identifier. The frame type implicitly identifies the logical channel, so no separate channel ID field is needed. |
| `Flags` | 16 bits | Type-specific flags. |
| `Frame ID` | 32 bits | Monotonically increasing per-sender frame counter. Used for deduplication and replay protection. |
| `Correlation ID` | 32 bits | Links a response frame to its originating request. For `RPC_REQUEST`, the sender assigns a unique ID; the peer MUST echo this value in the corresponding `RPC_RESPONSE` or `RPC_ERROR`. For `EVENT_ACK`, echoes the `Frame ID` of the acknowledged `EVENT`. MUST be `0x00000000` for frames that are not part of a request/response pair. |
| `Payload Length` | 32 bits | Length of the payload in bytes. Maximum: 16 MiB (16,777,216 bytes). |
| `Payload` | Variable | Frame-type-specific content. |

Total fixed header size: 16 bytes.

## Header Encoding

All integer fields in the fixed header MUST be encoded in network byte order (big-endian).

The first 16-bit header word packs `Version` and `Type` as follows:

- Bits 15 through 12 contain the 4-bit `Version`.
- Bits 11 through 0 contain the 12-bit `Type`.

For example, a draft version 0.1 `HELLO` frame has first word `0x1001`: version `0x1`, type `0x001`.

TCP streams carry a continuous sequence of Axon frames. Receivers MUST parse the 16-byte fixed header first, then read exactly `Payload Length` bytes before parsing the next frame.

WebSocket connections MUST use binary messages. After WebSocket message reassembly, each binary message MUST contain exactly one complete Axon frame. A receiver MUST reject text messages, incomplete binary messages, or binary messages containing more than one Axon frame with `ERR_INVALID_FRAME` and `fatal` set to `true`.

WebRTC Data Channels MUST use binary messages. After SCTP message reassembly, each Data Channel message MUST contain exactly one complete Axon frame, including the 16-byte Axon header. A receiver MUST reject text messages, incomplete binary messages, or binary messages containing more than one Axon frame with `ERR_INVALID_FRAME` and `fatal` set to `true`.

Frames sent on `axon.rpc` MUST be `RPC_REQUEST`, `RPC_RESPONSE`, `RPC_ERROR`, `PING`, `PONG`, or `ERROR`. Frames sent on `axon.events` MUST be `EVENT`, `EVENT_ACK`, `PING`, `PONG`, or `ERROR`. Control signaling frames such as `HELLO`, `SESSION_OFFER`, `SESSION_ACCEPT`, `PIPELINE_OFFER`, `PIPELINE_ANSWER`, `ICE_CANDIDATE`, `PIPELINE_READY`, `SESSION_READY`, and `GOODBYE` MUST be sent on the Control Plane.

If `Payload Length` exceeds 16 MiB, the receiver MUST send `ERROR` with code `ERR_RESOURCE_LIMIT`, set `fatal` to `true`, and close the transport connection. Receivers SHOULD enforce this limit before allocating a payload buffer.

Senders SHOULD also respect the effective maximum message size of the active WebRTC Data Channel. If an Axon frame cannot be sent because it exceeds that transport's message size, the sender MUST fail the operation with `ERR_RESOURCE_LIMIT` rather than fragmenting the structured payload at the Axon layer.

## Protocol Version Negotiation

Draft version 0.1 uses wire version `0x1`. All frames in a conformant draft version 0.1 session MUST set the header `Version` field to `0x1`.

In draft version 0.1, `HELLO.client_version`, `SESSION_OFFER.server_version`, and `SESSION_ACCEPT.client_version` MUST all be `1`. A draft version 0.1 endpoint receiving any other value MUST send `ERROR` with code `ERR_PROTOCOL_VERSION`, set `fatal` to `true`, and close the Control Plane connection.

Future versions MAY define negotiation across multiple wire versions. Until such a revision exists, implementations MUST NOT infer compatibility from a higher version number.

## Frame ID Assignment

Each sender MUST assign a non-zero `Frame ID` to every frame it sends. The `Frame ID` counter is per sender and per session, starts at `0x00000001`, increments by one for each frame, wraps after `0xFFFFFFFF`, and skips `0x00000000`.

Because a session can use both the Control Plane and Pipeline, frames can arrive out of order across transports. Receivers MUST track a sliding window of recently seen `Frame ID` values for duplicate detection instead of requiring strict arrival order across planes.

A receiver MUST reject a frame whose `Frame ID` is `0x00000000`. A receiver MUST also reject a frame whose `Frame ID` is already present in the receiver's replay window for that sender and session. Both cases use `ERROR` code `ERR_INVALID_FRAME`, set `ref_frame_id` to the rejected frame ID when possible, and set `fatal` to `true` for the affected frame carrier. Receivers MUST NOT reject a frame solely because its `Frame ID` is lower than a previously received frame ID if that value is not currently present in the replay window.

## Correlation ID Assignment

The sender of `RPC_REQUEST` MUST assign a Correlation ID that is unique among all currently in-flight requests on that session.

A monotonically increasing 32-bit counter is the RECOMMENDED strategy. The counter wraps at `0xFFFFFFFF` and skips `0x00000000`. The counter is per-sender and per-session; it does not need to be globally unique.

If an `RPC_RESPONSE` or `RPC_ERROR` arrives with a Correlation ID that does not match any pending request, the receiver MUST silently discard the frame and MAY emit a warning log.

## Frame Types

| Code | Name | Plane | Description |
| ---- | ---- | ----- | ----------- |
| `0x001` | `HELLO` | Control | Client greeting |
| `0x002` | `SESSION_OFFER` | Control | Server session proposal |
| `0x003` | `SESSION_ACCEPT` | Control | Client session confirmation |
| `0x004` | `SESSION_READY` | Control | Server confirms session operational |
| `0x005` | `GOODBYE` | Control | Graceful teardown |
| `0x010` | `PIPELINE_OFFER` | Control | WebRTC SDP offer |
| `0x011` | `PIPELINE_ANSWER` | Control | WebRTC SDP answer |
| `0x012` | `ICE_CANDIDATE` | Control | WebRTC ICE candidate |
| `0x013` | `PIPELINE_READY` | Control | Pipeline transport confirmed |
| `0x020` | `RPC_REQUEST` | Pipeline / Control | Invoke a remote procedure |
| `0x021` | `RPC_RESPONSE` | Pipeline / Control | Return value from RPC |
| `0x022` | `RPC_ERROR` | Pipeline / Control | RPC-level error |
| `0x030` | `EVENT` | Pipeline / Control | Publish an event |
| `0x031` | `EVENT_ACK` | Pipeline / Control | Acknowledge an event |
| `0x040` | `PING` | Either | Keepalive probe |
| `0x041` | `PONG` | Either | Keepalive response |
| `0x0F0` | `ERROR` | Either | Protocol-level error |

Frame types `0x100` through `0xFFF` are reserved for future use. Application-defined frame types are not permitted in this version.

## Flags

| Bit | Name | Applicable Types | Description |
| --- | ---- | ---------------- | ----------- |
| 0 | `FIN` | `RPC_RESPONSE`, `RPC_ERROR` | Final frame in an RPC response stream. |
| 1 | `ACK_REQ` | `EVENT` | Receiver MUST send `EVENT_ACK`. |
| 2 | `COMPRESSED` | Reserved | Reserved for a future compression extension. MUST be zero in draft version 0.1. |
| 3 | `ENCRYPTED` | Reserved | Reserved for a future application-layer encryption extension. MUST be zero in draft version 0.1. |
| 4-15 | Reserved | None | MUST be zero. |

Senders MUST NOT set a flag that is not applicable to the frame type being sent. Receivers MUST reject frames with unsupported or reserved flags by sending `ERROR` with code `ERR_INVALID_FRAME` and `fatal` set to `true`.

## Message Boundaries and Fragmentation

Draft version 0.1 does not support splitting a single structured payload across multiple Axon frames. Each frame MUST contain one complete payload.

The `FIN` flag is not a payload-fragmentation marker in draft version 0.1. It marks the terminal frame of an RPC response stream.

## Capabilities Field

The `capabilities` field in `SESSION_OFFER` and `SESSION_ACCEPT` is a 64-bit bitmask:

| Bit | Name | Description |
| --- | ---- | ----------- |
| 0 | `CAP_RPC` | RPC channel support |
| 1 | `CAP_EVENTS` | Event channel support |
| 2 | `CAP_MEDIA_AUDIO` | Audio media track support |
| 3 | `CAP_MEDIA_VIDEO` | Video media track support |
| 4-63 | Reserved | MUST be zero in draft version 0.1 |

Both endpoints MUST negotiate capabilities to their intersection. A server MUST NOT require a capability the client does not advertise.

The negotiated capability intersection MUST contain at least one of `CAP_RPC`, `CAP_EVENTS`, `CAP_MEDIA_AUDIO`, or `CAP_MEDIA_VIDEO`. If the intersection is empty, the server MUST send `ERROR` with code `ERR_CAPABILITY_MISMATCH`, set `fatal` to `true`, and close the session.

An endpoint MUST NOT send RPC frames unless `CAP_RPC` is present in `SESSION_READY.capabilities`. An endpoint MUST NOT send Event frames unless `CAP_EVENTS` is present in `SESSION_READY.capabilities`. If a receiver gets a frame for an unnegotiated channel capability after `SESSION_READY`, it MUST send `ERROR` with code `ERR_CHANNEL_CLOSED` and `fatal` set to `false`.

## Payload Encoding

All structured payloads such as handshake frames, RPC, and events MUST be encoded as MessagePack in interoperable draft version 0.1 implementations.

`HELLO` and `SESSION_OFFER` payloads MUST be encoded as MessagePack so that encoding negotiation can bootstrap reliably. The client advertises supported encodings in `HELLO.supported_encodings`. The server selects one encoding in `SESSION_OFFER.selected_encoding`. For interoperable draft version 0.1 sessions, `selected_encoding` MUST be `"msgpack"`.

Other encodings are reserved for future or private extensions and MUST NOT be selected for a conformant draft version 0.1 session.

Structured payloads MUST be maps with string keys. When a structured frame has no fields to send, its payload MUST be an encoded empty map. Receivers MUST ignore unknown fields. Receivers MUST reject frames that are missing required fields or contain fields with invalid types. Control frame schema errors MUST be rejected by sending `ERROR` with code `ERR_INVALID_FRAME` and `fatal` set according to [Error Fatality Rules](06-error-handling-and-reconnection.md#error-fatality-rules). RPC frame schema errors that can be associated with one request follow the `RPC_ERROR` rule in [Channel Semantics](05-channel-semantics.md#rpc-channel).

Integer fields marked as `uint8`, `uint16`, `uint32`, or `uint64` MUST be encoded as non-negative MessagePack integers within the named range. Integer fields marked as `int32` or `int64` MUST be encoded as MessagePack integers within the named signed range. Receivers MUST reject floating-point values for integer fields.

Media track payloads are raw codec data, such as Opus, VP8, VP9, H.264, or AV1, and are not subject to this encoding requirement.

## Control Payload Schemas

Optional fields default to the value stated in their description, if any. Optional fields with no stated default are absent by default and MUST NOT be treated as present with a null value. A sender MAY explicitly encode null only for fields whose type allows `null`.

### `HELLO`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `client_version` | uint8 | Yes | Axon protocol version selected by the client. In draft version 0.1 this MUST be `1`. |
| `supported_encodings` | array<string> | Yes | Encodings supported by the client. MUST include `"msgpack"`. |
| `resume_session_id` | string | No | Previous session ID the client wants to resume. |
| `metadata` | map<string, any> | No | Application-defined metadata. |

### `SESSION_OFFER`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `session_id` | string | Yes | Server-generated session identifier, or the resumed session ID. |
| `server_version` | uint8 | Yes | Axon protocol version selected by the server. In draft version 0.1 this MUST be `1`. |
| `selected_encoding` | string | Yes | Encoding selected from `HELLO.supported_encodings`. MUST be `"msgpack"` for conformant draft version 0.1 sessions. |
| `supported_transports` | array<string> | Yes | Pipeline transports supported by the server. In draft version 0.1 this MUST be `["webrtc"]`. |
| `capabilities` | uint64 | Yes | Server capability bitmask. |
| `auth_scheme` | string | No | Authentication scheme selected by the server. Defaults to `"none"`. Standard values for draft version 0.1 are `"none"` and `"bearer"`. |
| `auth_challenge` | bin | No | Opaque challenge bytes for application-defined authentication. |

### `SESSION_ACCEPT`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `session_id` | string | Yes | Echo of `SESSION_OFFER.session_id`. |
| `client_version` | uint8 | Yes | Axon protocol version selected by the client. MUST match `SESSION_OFFER.server_version`. |
| `selected_transport` | string | Yes | Selected Pipeline transport. In draft version 0.1 this MUST be `"webrtc"`. |
| `capabilities` | uint64 | Yes | Client capability bitmask. |
| `auth_response` | bin or string | No | Response to `auth_challenge`, or bearer token when `auth_scheme` is `"bearer"`. Required when the server selected an authentication scheme other than `"none"`. |
| `metadata` | map<string, any> | No | Application-defined metadata. |

### `PIPELINE_OFFER`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `sdp_type` | string | Yes | MUST be `"offer"`. |
| `sdp` | string | Yes | WebRTC SDP offer. |

### `PIPELINE_ANSWER`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `sdp_type` | string | Yes | MUST be `"answer"`. |
| `sdp` | string | Yes | WebRTC SDP answer. |

### `ICE_CANDIDATE`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `candidate` | string | Yes | ICE candidate string. |
| `sdp_mid` | string | No | SDP media stream identification. |
| `sdp_mline_index` | uint16 | No | SDP media line index. |
| `username_fragment` | string | No | ICE username fragment. |

An `ICE_CANDIDATE` with an empty `candidate` string indicates end-of-candidates for the referenced media section.

### `PIPELINE_READY`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `transport` | string | Yes | MUST be `"webrtc"` in draft version 0.1. |
| `data_channels` | array<string> | Yes | Established Axon Data Channels for negotiated capabilities, sorted in ascending lexicographic order. Empty when no Data Channel capability is negotiated. |
| `media_tracks` | array<string> | No | Established WebRTC `MediaStreamTrack.id` values, sorted in ascending lexicographic order. Omitted or empty when no media tracks are established. |

### `SESSION_READY`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `session_id` | string | Yes | Active session ID. |
| `selected_transport` | string | Yes | Active Pipeline transport. |
| `capabilities` | uint64 | Yes | Negotiated capability intersection. |
| `resumed` | bool | Yes | Whether this session resumed previous server-side state. |

### `GOODBYE`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `code` | uint16 | No | Application-defined close code. Defaults to `0`; `0` means normal closure. |
| `reason` | string | No | Human-readable close reason. |

### `PING` and `PONG`

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `nonce` | bin | No | Opaque bytes. `PONG` MUST echo this value exactly when present. |
| `timestamp_ms` | int64 | No | Sender wall-clock timestamp as Unix epoch milliseconds. |
