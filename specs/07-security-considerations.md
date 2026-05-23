# Security Considerations

## Transport Security

All Control Plane connections MUST use TLS 1.2 or higher. TLS 1.3 is RECOMMENDED. WebSocket connections MUST use `wss://`. TCP connections MUST use TLS wrapping.

Clients MUST validate the server certificate according to the rules of the deployment environment. Servers accepting WebSocket Control Plane connections from browsers SHOULD validate the HTTP `Origin` header before completing the WebSocket upgrade.

WebRTC Pipeline connections benefit from DTLS-SRTP as mandated by the WebRTC specification and require no additional transport security.

## Authentication

Axon draft version 0.1 defines two interoperable authentication schemes:

| Scheme | `SESSION_OFFER.auth_scheme` | Client Behavior |
| ------ | --------------------------- | --------------- |
| No authentication | `"none"` or absent | Client omits `auth_response`. |
| Bearer token | `"bearer"` | Client sends the token as a string in `SESSION_ACCEPT.auth_response`. |

Servers that require bearer authentication MUST set `auth_scheme` to `"bearer"` in `SESSION_OFFER` and MUST validate `SESSION_ACCEPT.auth_response` before accepting the session. If validation fails, the server MUST send `ERROR` with code `ERR_AUTH_FAILED`, set `fatal` to `true`, and close the Control Plane connection.

Axon also provides `auth_challenge` / `auth_response` fields for private challenge-response schemes. Challenge-response schemes are deployment-specific in draft version 0.1 and are not interoperable unless both endpoints are configured out of band.

Session resumption MUST authenticate as the same principal as the original session. A server MUST reject resumption with `ERR_AUTH_FAILED` if the reconnecting principal differs, even if the requested `session_id` exists.

## Authorization

Channel-level and method-level authorization is the responsibility of the application layer. Axon does not define an authorization model.

## Replay Protection

Frame IDs are monotonically increasing per sender and per session. Because the Control Plane and Pipeline can deliver frames out of order relative to one another, receivers MUST use a sliding replay window of recently accepted Frame IDs rather than a single "last accepted" value. Receivers MUST reject duplicate Frame IDs that are still inside the replay window.

Receivers MUST track at least 4096 recent Frame IDs per sender per session, spanning all carriers (Control Plane and Pipeline). Deployments with high fan-out or high latency SHOULD use a larger replay window.

A duplicate `Frame ID` inside the replay window or a zero `Frame ID` is a protocol error. The receiver MUST send `ERROR` with code `ERR_INVALID_FRAME`, `fatal` set to `true`, and `ref_frame_id` set to the rejected frame ID when possible. The receiver then closes the affected carrier according to the fatality rules in [Error Handling and Reconnection](06-error-handling-and-reconnection.md#error-fatality-rules).

Authentication tokens and challenge responses MUST be treated as secrets. Implementations MUST NOT write them to normal application logs.
