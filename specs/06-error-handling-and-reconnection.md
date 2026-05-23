# Error Handling and Reconnection

## Protocol Error Frame

Protocol-level errors, distinct from RPC-level errors, are communicated via the `ERROR` frame.

`ERROR` payload fields:

| Field | Type | Required | Description |
| ----- | ---- | -------- | ----------- |
| `code` | uint16 | Yes | Error code. |
| `message` | string | Yes | Human-readable description. |
| `fatal` | bool | Yes | If `true`, the sender will close the affected transport immediately after this frame. |
| `ref_frame_id` | uint32 | No | Frame ID that triggered this error, if applicable. Defaults to `0`. |

If `fatal` is `false`, the receiver MAY continue using the session after applying any frame-specific recovery behavior. If `fatal` is `true`, the receiver MUST treat the affected transport as closing.

An `ERROR` frame sent on the Control Plane with `fatal` set to `true` closes the entire Axon session. The sender MUST close the Control Plane after writing the `ERROR` frame and MUST close any associated Pipeline. The receiver MUST enter `CLOSING` and close the Pipeline immediately.

An `ERROR` frame sent on a WebRTC Data Channel with `fatal` set to `true` closes the Pipeline. The receiver MUST treat the Pipeline as lost. If the Control Plane is still connected and the session had reached `READY`, both endpoints follow [Pipeline Recovery](#pipeline-recovery). If the session had not reached `READY`, the session fails.

## Error Fatality Rules

Unless a frame-specific rule states otherwise, endpoints MUST use the following fatality rules:

| Condition | Code | `fatal` | Required behavior |
| --------- | ---- | ------- | ----------------- |
| Unsupported header version or incompatible handshake version | `ERR_PROTOCOL_VERSION` | `true` | Close the Control Plane and session. |
| Authentication failure | `ERR_AUTH_FAILED` | `true` | Close the Control Plane and session. |
| Required Control Plane handshake frame deadline expires before `SESSION_READY` | `ERR_TIMEOUT` | `true` | Close the Control Plane and session. |
| Malformed header, invalid MessagePack, unsupported or reserved flags, invalid payload schema for a control frame, unknown frame type, illegal frame direction, zero `Frame ID`, duplicate `Frame ID` inside the replay window, or frame received in an illegal session state | `ERR_INVALID_FRAME` | `true` | Close the affected transport. If the affected transport is the Control Plane, close the session. |
| RPC request payload is invalid but can be associated with one `RPC_REQUEST` | `ERR_INVALID_FRAME` in `RPC_ERROR` | N/A | Send `RPC_ERROR` with matching `Correlation ID` and `FIN`; keep the session open. |
| Negotiated capability intersection is empty | `ERR_CAPABILITY_MISMATCH` | `true` | Close the Control Plane and session. |
| SDP answer deadline expires, initial Pipeline readiness deadline expires, or initial Pipeline setup otherwise fails before `SESSION_READY` | `ERR_PIPELINE_FAILED` | `true` | Close the Control Plane and session. |
| Pipeline is lost or recovery attempt fails after `SESSION_READY` while the Control Plane remains connected | `ERR_PIPELINE_FAILED` | `false` | Enter or remain in `RECOVERING_PIPELINE`; keep using Control Plane fallback. |
| Frame is for a channel capability that was not negotiated, or for a channel that is currently unavailable after `SESSION_READY` | `ERR_CHANNEL_CLOSED` | `false` | Keep the session open; sender must stop using that channel until it is available and negotiated. |
| Inbound payload length exceeds 16 MiB or a receiver cannot process the frame without exceeding resource limits | `ERR_RESOURCE_LIMIT` | `true` | Close the affected transport. If the affected transport is the Control Plane, close the session. |
| Requested session state is unavailable for resumption | `ERR_SESSION_EXPIRED` | `true` | Close the Control Plane. The client MUST establish a fresh session on a new Control Plane connection. |
| Internal error before `SESSION_READY` | `ERR_INTERNAL` | `true` | Close the Control Plane and session. |
| Internal error after `SESSION_READY` where continued operation is unsafe | `ERR_INTERNAL` | `true` | Close the affected transport; close the session if the affected transport is the Control Plane. |
| Internal error after `SESSION_READY` where continued operation is safe | `ERR_INTERNAL` | `false` | Keep the session open. |

If an endpoint receives a second fatal `ERROR` or `GOODBYE` after it has entered `CLOSING`, it MUST ignore the duplicate close signal and continue closing transports.

## Error Codes

| Code | Name | Description |
| ---- | ---- | ----------- |
| `0x0001` | `ERR_PROTOCOL_VERSION` | Incompatible protocol version. |
| `0x0002` | `ERR_AUTH_FAILED` | Authentication failed. |
| `0x0003` | `ERR_TIMEOUT` | Expected frame not received in time. |
| `0x0004` | `ERR_INVALID_FRAME` | Malformed or unrecognized frame received. |
| `0x0005` | `ERR_CAPABILITY_MISMATCH` | Required capability not supported by peer. |
| `0x0006` | `ERR_PIPELINE_FAILED` | Pipeline negotiation or connection failed. |
| `0x0007` | `ERR_CHANNEL_CLOSED` | Referenced channel is no longer open. |
| `0x0008` | `ERR_RESOURCE_LIMIT` | Server-side resource limit reached. |
| `0x0009` | `ERR_CANCELLED` | In-flight RPC was cancelled by the caller. Used in `RPC_ERROR` with a matching Correlation ID; MUST NOT be sent in protocol `ERROR`. |
| `0x000A` | `ERR_SESSION_EXPIRED` | Previous session state is no longer available for resumption. |
| `0x00FF` | `ERR_INTERNAL` | Unspecified internal error. |

Codes `0x0100` through `0x7FFF` are reserved for future Axon protocol extensions. Codes `0x8000` through `0xFFFF` are reserved for private deployment-specific protocol errors and MUST NOT be used in interoperable specifications.

## Control Plane Reconnection

If the Control Plane connection is lost, the client MUST attempt reconnection using exponential backoff:

- Initial delay: 500 ms.
- Multiplier: 2x.
- Maximum delay: 30 s.
- Jitter: +/-25% of computed delay is RECOMMENDED.

When the Control Plane is lost, endpoints MUST treat the Pipeline as unavailable for session management. A resumed session MUST negotiate a fresh WebRTC Pipeline before returning to `READY`; servers SHOULD close or discard any old WebRTC peer connection associated with the interrupted Control Plane.

Upon reconnection, the client MUST perform a new handshake. To resume a previous session, the client MUST send the previous session ID in `HELLO.resume_session_id` on the first reconnect attempt for that lost session. A client MUST NOT send application RPC, Event, or media traffic until the resumed handshake reaches `SESSION_READY`.

A full draft version 0.1 server MUST implement session resumption. If the server still holds resumable state for `HELLO.resume_session_id` and the reconnecting client authenticates as the same principal, the server MUST resume that session. The resumed session uses the same `session_id`, and `SESSION_READY.resumed` MUST be `true`.

If `HELLO.resume_session_id` is present and the server cannot resume that exact session because the state is expired, evicted, unknown, or incomplete, the server MUST send `ERROR` with code `ERR_SESSION_EXPIRED`, set `fatal` to `true`, and close the Control Plane. If the reconnecting principal differs from the original session principal, the server MUST instead send `ERROR` with code `ERR_AUTH_FAILED`, set `fatal` to `true`, and close the Control Plane. A server MUST NOT answer a resumption request by issuing a different `session_id`.

After receiving `ERR_SESSION_EXPIRED` for a resumption request, the client MUST discard the old session state, MUST NOT retry that same `resume_session_id`, and MUST establish a fresh session on a new Control Plane connection.

Servers MUST retain resumable session state for at least 60 seconds after Control Plane loss. If a server evicts resumable state earlier because of a hard resource limit, it MUST reject later resumption for that session with `ERR_SESSION_EXPIRED`; it MUST NOT resume with partial protocol state.

### Resumable State

When a session is resumed, the following protocol state MUST be retained:

- `session_id`.
- Authenticated principal or equivalent security context.
- Negotiated capability intersection from the previous session.
- Recent `Frame ID` replay windows for each sender.
- Sender-side `Frame ID` counters, which MUST continue rather than restart at `0x00000001`.
- In-flight RPC correlation table, including retry eligibility and `idempotency_key` values.
- Event retransmission state for unacknowledged events sent with `ACK_REQ`.

The following state is not resumed and MUST be re-established:

- Control Plane transport connection.
- WebRTC peer connection, Data Channels, ICE state, and media tracks.
- Keepalive timers.

Application-defined subscription state MAY be retained only if the application layer explicitly binds it to the Axon session. If subscription state is not retained, the application MUST resubscribe after `SESSION_READY`.

If the server cannot preserve all required protocol state, it MUST reject resumption with `ERR_SESSION_EXPIRED`, set `fatal` to `true`, close the Control Plane, and force a fresh session.

## Pipeline Recovery

If the Pipeline connection is lost but the Control Plane remains intact:

1. RPC and Event traffic MUST immediately fall back to the Control Plane.
2. Media channels are suspended.
3. The session enters `RECOVERING_PIPELINE`.
4. The client MUST reattempt Pipeline negotiation after a delay of 1 second, with exponential backoff up to 30 seconds.
5. Recovery MUST use a fresh `PIPELINE_OFFER` / `PIPELINE_ANSWER` exchange unless both endpoints already support WebRTC ICE restart by out-of-band configuration. A conformant draft version 0.1 implementation MUST support the fresh offer/answer recovery path.
6. The server MUST send `PIPELINE_READY` after the required Data Channels are open again.
7. `SESSION_READY` MUST NOT be resent for Pipeline-only recovery.
8. Once the Pipeline recovers, traffic MUST migrate back from fallback to the Pipeline automatically.

Each Pipeline recovery attempt uses the same SDP answer timeout and 30 second Pipeline readiness deadline as the initial Pipeline negotiation. For the client, the 30 second recovery deadline starts when it sends the recovery `PIPELINE_OFFER`. For the server, the 30 second recovery deadline starts when it receives a valid recovery `PIPELINE_OFFER`. If a recovery attempt fails, the endpoint detecting failure MUST send `ERROR` with code `ERR_PIPELINE_FAILED` and `fatal` set to `false` on the Control Plane, remain in `RECOVERING_PIPELINE`, and continue retrying according to the backoff schedule until the session is closed.

## In-Flight Message Handling During Failover

When this section says that a requester fails a local RPC operation with an error code, that error is reported to the local application API. The requester MUST NOT send a protocol `ERROR` frame solely because it failed a local in-flight RPC operation during failover.

- **RPC not yet written**: RPC requests that have not yet been written to the failed carrier MUST be sent over the current RPC carrier: the Control Plane during Pipeline recovery, or the newly negotiated carrier after successful Control Plane resumption.
- **RPC already written without retry eligibility**: RPC requests already written to the failed carrier but not completed MUST NOT be retried automatically unless the original request was a non-streaming request with `idempotency_key`. The requester MUST fail the local RPC operation with `ERR_CANCELLED`, release matching state for the `Correlation ID`, MUST NOT intentionally reuse that `Correlation ID` until normal counter wraparound, and silently discard any later response for that correlation as an unknown response.
- **RPC already written with retry eligibility**: A non-streaming RPC request that included `idempotency_key` and has not completed MUST be retried after failover if its `timeout_ms` deadline has not expired. The retry MUST use a new `Frame ID`, the same `Correlation ID`, and a byte-for-byte identical `RPC_REQUEST` payload. If the request timed out before a retry can be written, the requester MUST fail the local RPC operation with `ERR_TIMEOUT`, release matching state for the `Correlation ID`, MUST NOT retry it, and MUST NOT intentionally reuse that `Correlation ID` until normal counter wraparound. Servers MUST deduplicate idempotent retries according to [RPC Channel](05-channel-semantics.md#rpc-channel).
- **Streaming RPC**: Streaming RPCs MUST NOT be retried automatically after the request has been written to a failed carrier, even if the request payload incorrectly included `idempotency_key`. The requester MUST fail the local streaming operation with `ERR_CANCELLED`.
- **Events**: Events with `ACK_REQ` that have not been acknowledged MUST be retransmitted immediately on the current Event carrier after failover, counting that send as the next retransmission attempt under the Event retransmission rules in [Event Channel](05-channel-semantics.md#event-channel). Events without `ACK_REQ` MAY be lost during failover.
- **Media**: Media is not retransmitted; loss during failover is expected and acceptable.

## Keepalive

Keepalive is tracked independently for each Axon frame carrier:

- The Control Plane connection.
- The `axon.rpc` Data Channel, when open.
- The `axon.events` Data Channel, when open.

Media tracks are excluded from Axon keepalive because WebRTC handles media transport liveness.

For each carrier, both endpoints MUST send a `PING` frame on that same carrier if no Axon frame has been sent on that carrier for 30 seconds. The peer MUST respond with a `PONG` frame on the same carrier within 10 seconds. Any Axon frame sent on that carrier resets the sender's 30 second idle timer.

Failure to receive a `PONG` on the Control Plane MUST be treated as Control Plane loss and triggers [Control Plane Reconnection](#control-plane-reconnection). Failure to receive a `PONG` on either Axon Data Channel MUST be treated as Pipeline loss and triggers [Pipeline Recovery](#pipeline-recovery). A `PING` sent on one carrier MUST NOT be answered on a different carrier.

The `PING` payload MAY contain an opaque nonce echoed in the `PONG` for round-trip time measurement.
