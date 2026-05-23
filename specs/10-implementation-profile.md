# Implementation Profile and Test Vectors

This document defines the draft version 0.1 interoperable implementation profile. It is intended to remove ambiguity for the first client and server implementations.

## Required Profile

A draft version 0.1 implementation MUST use:

- Header wire version `0x1`.
- MessagePack for all structured payloads.
- A maximum Axon payload length of 16 MiB.
- TCP Control Plane endpoint binding with TLS ALPN `axon/1`, when TCP is implemented.
- WebSocket Control Plane endpoint binding at `/axon/v1` with `Sec-WebSocket-Protocol: axon.v1`, when WebSocket is implemented.
- Exactly one complete Axon frame per WebSocket binary message.
- Exactly one complete Axon frame per WebRTC Data Channel binary message.
- WebRTC Data Channels named `axon.rpc` and `axon.events` with the required reliability parameters when their corresponding capabilities are negotiated.
- WebRTC Media Tracks for active media tracks negotiated in SDP.
- Opus audio support when advertising `CAP_MEDIA_AUDIO`.
- VP8 video support when advertising `CAP_MEDIA_VIDEO`.
- The session state machine defined in [Connection Lifecycle and Handshake](03-connection-lifecycle.md).
- Session resumption on Control Plane reconnect as defined in [Error Handling and Reconnection](06-error-handling-and-reconnection.md#control-plane-reconnection).
- The authentication schemes `"none"` and `"bearer"` defined in [Security Considerations](07-security-considerations.md).

A full server MUST expose both Control Plane bindings:

- TLS-wrapped TCP with ALPN `axon/1`.
- Secure WebSocket (`wss://`) at `/axon/v1` with `Sec-WebSocket-Protocol: axon.v1`.

A constrained server MAY expose only one Control Plane binding, but it MUST document that limitation and MUST still support the WebRTC Pipeline.

## MessagePack Vector Rules

The following vectors use MessagePack maps encoded in the field order shown. MessagePack itself does not assign semantic meaning to map order, but byte-for-byte tests MUST use the order shown here.

Hex strings are lowercase and omit spaces. The full frame hex includes the 16-byte Axon header followed by the MessagePack payload.

## Vector 1: `HELLO`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x001` |
| Flags | `0x0000` |
| Frame ID | `0x00000001` |
| Correlation ID | `0x00000000` |
| Payload Length | `46` |

Payload object:

```json
{
  "client_version": 1,
  "supported_encodings": ["msgpack"]
}
```

Payload hex:

```text
82ae636c69656e745f76657273696f6e01b3737570706f727465645f656e636f64696e677391a76d73677061636b
```

Full frame hex:

```text
1001000000000001000000000000002e82ae636c69656e745f76657273696f6e01b3737570706f727465645f656e636f64696e677391a76d73677061636b
```

## Vector 2: `SESSION_OFFER`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x002` |
| Flags | `0x0000` |
| Frame ID | `0x00000001` |
| Correlation ID | `0x00000000` |
| Payload Length | `152` |

Payload object:

```json
{
  "session_id": "00000000-0000-4000-8000-000000000001",
  "server_version": 1,
  "selected_encoding": "msgpack",
  "supported_transports": ["webrtc"],
  "capabilities": 3,
  "auth_scheme": "none"
}
```

Payload hex:

```text
86aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031ae7365727665725f76657273696f6e01b173656c65637465645f656e636f64696e67a76d73677061636bb4737570706f727465645f7472616e73706f72747391a6776562727463ac6361706162696c697469657303ab617574685f736368656d65a46e6f6e65
```

Full frame hex:

```text
1002000000000001000000000000009886aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031ae7365727665725f76657273696f6e01b173656c65637465645f656e636f64696e67a76d73677061636bb4737570706f727465645f7472616e73706f72747391a6776562727463ac6361706162696c697469657303ab617574685f736368656d65a46e6f6e65
```

## Vector 3: `PING`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x040` |
| Flags | `0x0000` |
| Frame ID | `0x00000002` |
| Correlation ID | `0x00000000` |
| Payload Length | `1` |

Payload object:

```json
{}
```

Payload hex:

```text
80
```

Full frame hex:

```text
1040000000000002000000000000000180
```

## Vector 4: `RPC_REQUEST`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x020` |
| Flags | `0x0000` |
| Frame ID | `0x00000003` |
| Correlation ID | `0x00000001` |
| Payload Length | `60` |

Payload object:

```json
{
  "method": "room.join",
  "params": { "id": 1 },
  "timeout_ms": 5000,
  "expects_stream": false
}
```

Payload hex:

```text
84a66d6574686f64a9726f6f6d2e6a6f696ea6706172616d7381a2696401aa74696d656f75745f6d73cd1388ae657870656374735f73747265616dc2
```

Full frame hex:

```text
1020000000000003000000010000003c84a66d6574686f64a9726f6f6d2e6a6f696ea6706172616d7381a2696401aa74696d656f75745f6d73cd1388ae657870656374735f73747265616dc2
```

## Vector 5: `RPC_RESPONSE`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x021` |
| Flags | `0x0001` (`FIN`) |
| Frame ID | `0x00000001` |
| Correlation ID | `0x00000001` |
| Payload Length | `26` |

Payload object:

```json
{
  "result": {
    "members": ["ada", "lin"]
  }
}
```

Payload hex:

```text
81a6726573756c7481a76d656d6265727392a3616461a36c696e
```

Full frame hex:

```text
1021000100000001000000010000001a81a6726573756c7481a76d656d6265727392a3616461a36c696e
```

## Vector 6: `SESSION_ACCEPT`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x003` |
| Flags | `0x0000` |
| Frame ID | `0x00000002` |
| Correlation ID | `0x00000000` |
| Payload Length | `106` |

Payload object:

```json
{
  "session_id": "00000000-0000-4000-8000-000000000001",
  "client_version": 1,
  "selected_transport": "webrtc",
  "capabilities": 3
}
```

Payload hex:

```text
84aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031ae636c69656e745f76657273696f6e01b273656c65637465645f7472616e73706f7274a6776562727463ac6361706162696c697469657303
```

Full frame hex:

```text
1003000000000002000000000000006a84aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031ae636c69656e745f76657273696f6e01b273656c65637465645f7472616e73706f7274a6776562727463ac6361706162696c697469657303
```

## Vector 7: `SESSION_READY`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x004` |
| Flags | `0x0000` |
| Frame ID | `0x00000004` |
| Correlation ID | `0x00000000` |
| Payload Length | `99` |

Payload object:

```json
{
  "session_id": "00000000-0000-4000-8000-000000000001",
  "selected_transport": "webrtc",
  "capabilities": 3,
  "resumed": false
}
```

Payload hex:

```text
84aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031b273656c65637465645f7472616e73706f7274a6776562727463ac6361706162696c697469657303a7726573756d6564c2
```

Full frame hex:

```text
1004000000000004000000000000006384aa73657373696f6e5f6964d92430303030303030302d303030302d343030302d383030302d303030303030303030303031b273656c65637465645f7472616e73706f7274a6776562727463ac6361706162696c697469657303a7726573756d6564c2
```

## Vector 8: `ERROR`

Header fields:

| Field | Value |
| ----- | ----- |
| Version | `0x1` |
| Type | `0x0F0` |
| Flags | `0x0000` |
| Frame ID | `0x00000005` |
| Correlation ID | `0x00000000` |
| Payload Length | `36` |

Payload object:

```json
{
  "code": 4,
  "message": "invalid frame",
  "fatal": true
}
```

Payload hex:

```text
83a4636f646504a76d657373616765ad696e76616c6964206672616d65a5666174616cc3
```

Full frame hex:

```text
10f0000000000005000000000000002483a4636f646504a76d657373616765ad696e76616c6964206672616d65a5666174616cc3
```

## Required Negative Tests

Implementations SHOULD include negative tests that verify rejection or required handling of:

- Header `Version` values other than `0x1`.
- Reserved flags.
- Zero `Frame ID`.
- Duplicate `Frame ID` values still inside the replay window.
- Payload lengths greater than 16 MiB.
- Text WebSocket messages.
- Text Data Channel messages.
- Multiple Axon frames inside one WebSocket or Data Channel message.
- Control signaling frames sent on `axon.rpc` or `axon.events`.
- RPC frames sent on `axon.events`.
- Event frames sent on `axon.rpc`.
- Application RPC or Event frames before `SESSION_READY`.
- Missing, mislabeled, or incorrectly configured required Axon Data Channels.
- Pipeline setup timeout before `SESSION_READY`.
- Unnegotiated RPC or Event channel use after `SESSION_READY`.
- A resumption request that is answered with a different `session_id`.
- `EVENT_ACK` frames that reference unknown events, which must be silently discarded.
