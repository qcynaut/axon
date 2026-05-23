# Appendices

## Appendix A: Frame Type Quick Reference

| Code | Frame | Direction | Plane |
| ---- | ----- | --------- | ----- |
| `0x001` | `HELLO` | C -> S | Control |
| `0x002` | `SESSION_OFFER` | S -> C | Control |
| `0x003` | `SESSION_ACCEPT` | C -> S | Control |
| `0x004` | `SESSION_READY` | S -> C | Control |
| `0x005` | `GOODBYE` | Both | Control |
| `0x010` | `PIPELINE_OFFER` | C -> S | Control |
| `0x011` | `PIPELINE_ANSWER` | S -> C | Control |
| `0x012` | `ICE_CANDIDATE` | Both | Control |
| `0x013` | `PIPELINE_READY` | S -> C | Control |
| `0x020` | `RPC_REQUEST` | Both | Pipeline / Control |
| `0x021` | `RPC_RESPONSE` | Both | Pipeline / Control |
| `0x022` | `RPC_ERROR` | Both | Pipeline / Control |
| `0x030` | `EVENT` | Both | Pipeline / Control |
| `0x031` | `EVENT_ACK` | Both | Pipeline / Control |
| `0x040` | `PING` | Both | Either |
| `0x041` | `PONG` | Both | Either |
| `0x0F0` | `ERROR` | Both | Either |

## Appendix B: Example Session Flow (WebSocket + WebRTC)

```text
Client                                          Server
  |                                               |
  |  [WebSocket TCP Upgrade to /axon/v1, subprotocol axon.v1] |
  |---------------------------------------------->|
  |                                               |
  |  HELLO {client_version:1, supported_encodings:["msgpack"]} |
  |---------------------------------------------->|
  |                                               |
  |  SESSION_OFFER {session_id, server_version:1, selected_encoding:"msgpack",
  |                 supported_transports:["webrtc"], capabilities:15, auth_scheme:"none"}
  |<----------------------------------------------|
  |                                               |
  |  SESSION_ACCEPT {session_id, client_version:1,
  |                  selected_transport:"webrtc", capabilities:15}
  |---------------------------------------------->|
  |                                               |
  |  PIPELINE_OFFER {sdp_type:"offer", sdp:"v=0 ..."} |
  |---------------------------------------------->|
  |                                               |
  |  PIPELINE_ANSWER {sdp_type:"answer", sdp:"v=0 ..."} |
  |<----------------------------------------------|
  |                                               |
  |  ICE_CANDIDATE {candidate:"..."}  (trickle)   |
  |<--------------------------------------------->|
  |                                               |
  |  [WebRTC Data Channels: axon.rpc, axon.events]|
  |  [WebRTC Media Tracks negotiated in SDP]      |
  |                                               |
  |  PIPELINE_READY {transport:"webrtc", data_channels:["axon.events","axon.rpc"]} |
  |<----------------------------------------------|
  |                                               |
  |  SESSION_READY {session_id, selected_transport:"webrtc", capabilities:15, resumed:false} |
  |<----------------------------------------------|
  |                                               |
  |  [Session operational]                        |
  |                                               |
  |  RPC_REQUEST {method:"room.join",params:{id:1}}|
  |---------------------------------------------->|  Correlation ID: 0x00000001, via axon.rpc
  |                                               |
  |  RPC_RESPONSE {result:{members:[...]}}        |
  |<----------------------------------------------|  Correlation ID: 0x00000001, FIN, via axon.rpc
  |                                               |
  |  EVENT {topic:"chat.message", payload:{...}}  |
  |<----------------------------------------------|  via axon.events Data Channel
  |                                               |
  |  [Audio/Video flowing via Media Tracks]       |
  |<=============================================>|
```

## Appendix C: Example Session Flow (TCP + Bearer Auth)

```text
Client                                          Server
  |                                               |
  |  [TLS handshake to axon+tcp://host:4765, ALPN axon/1]   |
  |<=============================================>|
  |                                               |
  |  HELLO {client_version:1, supported_encodings:["msgpack"]} |
  |---------------------------------------------->|
  |                                               |
  |  SESSION_OFFER {session_id, server_version:1, selected_encoding:"msgpack",
  |                 supported_transports:["webrtc"], capabilities:3,
  |                 auth_scheme:"bearer"}
  |<----------------------------------------------|
  |                                               |
  |  SESSION_ACCEPT {session_id, client_version:1,
  |                  selected_transport:"webrtc", capabilities:3,
  |                  auth_response:"eyJhbGciOi..."}
  |---------------------------------------------->|
  |                                               |
  |  PIPELINE_OFFER {sdp_type:"offer", sdp:"v=0 ..."} |
  |---------------------------------------------->|
  |                                               |
  |  PIPELINE_ANSWER {sdp_type:"answer", sdp:"v=0 ..."} |
  |<----------------------------------------------|
  |                                               |
  |  ICE_CANDIDATE {candidate:"..."}  (trickle)   |
  |<--------------------------------------------->|
  |                                               |
  |  [WebRTC Data Channels: axon.events, axon.rpc]|
  |                                               |
  |  PIPELINE_READY {transport:"webrtc", data_channels:["axon.events","axon.rpc"]} |
  |<----------------------------------------------|
  |                                               |
  |  SESSION_READY {session_id, selected_transport:"webrtc", capabilities:3, resumed:false} |
  |<----------------------------------------------|
  |                                               |
  |  [Session operational — RPC and Events via Pipeline, media via WebRTC tracks] |
```
