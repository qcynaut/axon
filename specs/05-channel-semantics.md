# Channel Semantics

Draft version 0.1 defines fixed channel classes: RPC, Event, and Media. There is no numeric channel ID in the common frame header. The frame type determines the channel class.

Application-defined WebRTC Data Channels prefixed with `axon.app.` MAY be opened, but their payloads are outside the Axon frame semantics unless a future extension defines otherwise.

## RPC Channel

The RPC channel enables request/response interactions where the caller blocks logically pending a response. Multiple RPC calls MAY be in-flight simultaneously, each distinguished by a unique `Correlation ID` set by the requester in the frame header.

Unless a payload table states otherwise, missing required fields or fields with invalid types MUST be rejected with `RPC_ERROR` code `ERR_INVALID_FRAME` when the error is specific to one RPC, or protocol `ERROR` code `ERR_INVALID_FRAME` when the receiver cannot associate the error with one RPC.

### Request Frame

`RPC_REQUEST` payload fields:

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `method` | string | Yes | Fully-qualified method name, such as `"user.getProfile"`. MUST be non-empty and SHOULD be limited to 1024 UTF-8 bytes. |
| `params` | any or null | No | Method parameters. Defaults to null. |
| `timeout_ms` | uint32 | No | Client-side timeout in milliseconds. Defaults to `0`; `0` means no timeout. |
| `expects_stream` | bool | No | If `true`, the caller allows multiple response frames. Defaults to `false`. |
| `idempotency_key` | string | No | Application-generated key used to deduplicate safe retries. SHOULD be limited to 256 UTF-8 bytes. |

`RPC_REQUEST` MUST use a non-zero `Correlation ID` that is unique among the sender's currently in-flight RPC calls for the session.

If `timeout_ms` is non-zero, the timeout is measured from the first time the requester writes the `RPC_REQUEST` to an Axon carrier. Retries caused by failover or resumption do not reset this deadline.

In draft version 0.1, `idempotency_key` is defined only for non-streaming RPCs. A receiver MUST reject an `RPC_REQUEST` that sets both `expects_stream` to `true` and `idempotency_key` by sending `RPC_ERROR` with code `ERR_INVALID_FRAME`, the same `Correlation ID`, and the `FIN` flag set.

For non-streaming RPCs that include `idempotency_key`, the responder MUST maintain a deduplication table scoped to the session, caller principal, `method`, and `idempotency_key`. If a duplicate request arrives with the same key, method, and byte-equivalent encoded `params`, the responder MUST NOT execute the method a second time. If the first execution is still in flight, the duplicate request updates the preferred response carrier to the carrier on which the duplicate was received. If the first execution has completed, the responder MUST send the cached terminal `RPC_RESPONSE` or `RPC_ERROR` again with the retried request's `Correlation ID`. If the same `idempotency_key` is reused with a different `method` or non-equivalent `params`, the responder MUST send `RPC_ERROR` with code `ERR_INVALID_FRAME` and the `FIN` flag set.

Responders MUST retain completed idempotency records while the session is active and for at least 60 seconds after the terminal response, including across successful session resumption. If a responder cannot retain the required idempotency state, it MUST fail new idempotent RPC requests with `RPC_ERROR` code `ERR_RESOURCE_LIMIT` rather than risk duplicate execution.

### Response Frame

The `Correlation ID` in the frame header MUST match the `Correlation ID` of the corresponding `RPC_REQUEST` frame. The receiver uses this to demultiplex the response to the correct pending call.

`RPC_RESPONSE` payload fields:

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `result` | any or null | Yes | Return value. |

For non-streaming RPCs, the responder MUST send exactly one `RPC_RESPONSE` with the `FIN` flag set, or exactly one `RPC_ERROR` with the `FIN` flag set.

The responder MUST send all `RPC_RESPONSE` or `RPC_ERROR` frames for an RPC on the carrier that carried the request, unless that carrier becomes unavailable. If the request was retried with the same `Correlation ID` after failover, the responder MUST use the carrier of the most recent accepted retry unless that carrier becomes unavailable. When the selected carrier is unavailable, the responder MUST send remaining response frames on the currently available RPC carrier: the Control Plane during Pipeline recovery, or the Pipeline once recovered.

### RPC Error Frame

`RPC_ERROR` payload fields:

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `code` | int32 | Yes | Application-defined or Axon-defined error code. |
| `message` | string | Yes | Human-readable error description. |
| `data` | any or null | No | Optional structured error detail. Defaults to null. |

Protocol error code names defined in [Error Handling and Reconnection](06-error-handling-and-reconnection.md) MAY be reused in `RPC_ERROR` when the RPC error is generated by Axon itself rather than by application logic.

### Streaming RPC

A single RPC call MAY return multiple `RPC_RESPONSE` frames, representing a server-streaming RPC. All response frames MUST carry the same `Correlation ID` as the originating `RPC_REQUEST`. The final frame MUST set the `FIN` flag.

The responder MUST NOT send multiple `RPC_RESPONSE` frames unless the request payload set `expects_stream` to `true`.

The requester MAY cancel a streaming RPC by sending an `RPC_ERROR` frame with the same `Correlation ID`, code `ERR_CANCELLED`, and the `FIN` flag set.

After an endpoint sends or receives an `RPC_RESPONSE` or `RPC_ERROR` with the `FIN` flag set, the corresponding `Correlation ID` is no longer in flight and MAY be reused by the original requester after wraparound.

Axon does not guarantee ordering between independent RPC calls. Applications that require ordering MUST express that dependency at the application layer, such as by awaiting one response before sending the next request.

### RPC Transport Selection

When the Pipeline Data Channel is available, RPC traffic MUST be sent over it. If the Pipeline is unavailable, RPC traffic MUST fall back to the Control Plane. During Control Plane fallback, the receiver distinguishes RPC frames from Event frames by the frame type code in the Axon header; no separate channel identifier is needed because the Control Plane carries both frame types on a single connection.

Implementations MUST transparently send new RPC traffic on the Pipeline once it recovers, without requiring the application layer to retry. In-flight RPC behavior during failover is defined in [Error Handling and Reconnection](06-error-handling-and-reconnection.md).

## Event Channel

The Event channel delivers asynchronous messages from a publisher to one or more subscribers. Events are identified by a topic string. Events are fire-and-forget unless `ACK_REQ` is set.

The WebRTC Event Data Channel is unordered in the default profile. Therefore, Axon does not guarantee receive order for events sent on the Pipeline. The `seq` field is the authoritative ordering signal per publisher and topic.

Subscription management is application-defined. Applications MAY use RPC methods, static configuration, or application-specific events to manage subscriptions.

### Event Frame

`EVENT` payload fields:

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `topic` | string | Yes | Hierarchical topic identifier, such as `"chat.message"` or `"sensor.reading"`. |
| `payload` | any or null | Yes | Event data. |
| `seq` | uint64 | Yes | Monotonically increasing sequence number per topic per publisher. |
| `timestamp_ms` | int64 | Yes | Publisher wall-clock timestamp as Unix epoch milliseconds. |

### Acknowledgment

When `ACK_REQ` is set on an `EVENT` frame, the receiver MUST respond with an `EVENT_ACK` frame referencing the same `Frame ID`.

`EVENT_ACK` MUST set `Correlation ID` to the `Frame ID` of the acknowledged `EVENT`. The `EVENT_ACK` payload MUST be an empty map.

The sender of an `EVENT` with `ACK_REQ` set MUST track the event as pending until it receives an `EVENT_ACK` whose `Correlation ID` equals the original `EVENT.Frame ID`. If no matching `EVENT_ACK` is received within 5 seconds, the sender MUST retransmit the event. A retransmission MUST use a new `Frame ID` and the same `topic`, `payload`, `seq`, and `timestamp_ms`. The sender MUST make at most three retransmissions after the original send, using delays of 5 seconds, 10 seconds, and 20 seconds. If no matching `EVENT_ACK` is received after the final retransmission deadline, the sender MUST stop retransmitting, keep the session open, and report event delivery failure to the application layer.

If an `EVENT_ACK` references an unknown, expired, or already acknowledged `EVENT.Frame ID`, the receiver of the `EVENT_ACK` MUST silently discard it and MUST NOT send `ERROR`.

Receivers MUST deduplicate events by publisher identity, topic, and `seq`. For draft version 0.1, publisher identity is the authenticated peer endpoint within the session. A duplicate event MUST NOT be delivered to the application more than once. If the duplicate `EVENT` has `ACK_REQ` set, the receiver MUST still send `EVENT_ACK` for that duplicate frame's `Frame ID`.

If a receiver detects a sequence gap, it MAY notify the application layer, but draft version 0.1 does not define automatic event replay beyond the `ACK_REQ` retransmission behavior above. Receivers MAY buffer events briefly to deliver them to the application in ascending `seq` order, but they MUST NOT block delivery indefinitely waiting for a missing sequence number.

### Topic Naming

Topics MUST be UTF-8 strings. Topic segments MUST be separated by `.`. Empty segments are invalid. Topics beginning with `axon.` are reserved for protocol use.

### Event Transport Selection

Events follow the same transport selection rules as RPC. Events fall back to the Control Plane when the Pipeline is unavailable.

## Media Channel

The Media channel carries real-time audio and video. It is exclusively transported via WebRTC Media Tracks. There is no Control Plane fallback for media.

### Track Negotiation

Media tracks are negotiated during Pipeline setup via standard WebRTC SDP negotiation. In draft version 0.1, the client is the offerer for both initial Pipeline negotiation and later Pipeline renegotiation. The server MUST NOT send `PIPELINE_OFFER`.

Renegotiation, such as adding or removing tracks, MAY be performed after session establishment using a new client-to-server `PIPELINE_OFFER` followed by server-to-client `PIPELINE_ANSWER` over the Control Plane. After successful renegotiation, the server MUST send `PIPELINE_READY` with the currently established `data_channels` and `media_tracks`. `SESSION_READY` MUST NOT be resent for media-only or Pipeline-only renegotiation after the session is already `READY`.

If media negotiation fails before the initial `SESSION_READY`, the endpoint detecting failure MUST send `ERROR` with code `ERR_PIPELINE_FAILED`, set `fatal` to `true`, and close the session. If media renegotiation fails after the session is already `READY`, the endpoint detecting failure MUST send `ERROR` with code `ERR_PIPELINE_FAILED`, set `fatal` to `false`, keep the existing Pipeline state unchanged when possible, and continue the session.

### Track Metadata

Applications MAY associate metadata with a media track via a companion `EVENT` on the topic `axon.media.track.<track_id>`, carrying fields such as codec, sample rate, and channel count.

### Supported Codecs

Codec negotiation is handled by SDP, but capability advertisement has the following interoperability requirements:

- An endpoint that advertises `CAP_MEDIA_AUDIO` MUST be able to negotiate, send, and receive Opus audio over WebRTC RTP.
- An endpoint that advertises `CAP_MEDIA_VIDEO` MUST be able to negotiate, send, and receive VP8 video over WebRTC RTP.
- VP9, H.264 Constrained Baseline, and AV1 are OPTIONAL in draft version 0.1.

If an SDP offer or answer requests an active audio or video track but the peers have no common codec satisfying the advertised media capability, Pipeline negotiation fails with `ERR_PIPELINE_FAILED`.

`PIPELINE_READY.media_tracks`, when present, MUST contain the WebRTC `MediaStreamTrack.id` values for active media tracks established by the most recent successful negotiation. The array MUST be sorted in ascending lexicographic order. If no media tracks are established, the sender MUST either omit `media_tracks` or send an empty array; receivers MUST treat both forms as equivalent.
