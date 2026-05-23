# Architecture Overview

## Dual-Plane Model

Each Axon endpoint maintains two logical connections to its peer:

```text
+---------------------------------------------------------+
|                      Axon Session                       |
|                                                         |
|  +---------------------+   +-------------------------+  |
|  |    Control Plane    |   |        Pipeline         |  |
|  |  (TCP / WebSocket)  |   |  (WebRTC)               |  |
|  |                     |   |                         |  |
|  | - Signaling         |   | - RPC Channel (primary) |  |
|  | - Session mgmt      |   | - Event Channel         |  |
|  | - RPC fallback      |   |   (primary)             |  |
|  | - Event fallback    |   | - Media Track           |  |
|  |                     |   |   (audio/video)         |  |
|  +---------------------+   +-------------------------+  |
+---------------------------------------------------------+
```

The Control Plane MUST be established before the Pipeline. The Pipeline is negotiated via signaling messages carried over the Control Plane.

## Transport Matrix

Draft version 0.1 defines two Control Plane transport bindings and one Pipeline transport binding:

| Transport | Plane | Protocol |
| --------- | ----- | -------- |
| TCP | Control Plane | TLS-wrapped TCP socket with Axon framing |
| WebSocket | Control Plane | Secure WebSocket (`wss://`, RFC 6455) carrying Axon frames |
| WebRTC | Pipeline | Data Channel for RPC/Events and Media Track for media |

Full servers implementing draft version 0.1 MUST support both Control Plane transport bindings and MUST support the WebRTC Pipeline binding.

Clients implementing Axon MUST support at least one Control Plane transport and the WebRTC Pipeline transport. During session establishment, the client selects one active Control Plane transport by connecting to the corresponding endpoint, then selects WebRTC from the server's `supported_transports` advertisement.

## Control Plane Endpoint Binding

Before opening a session, a client MUST be configured with an Axon Control Plane endpoint. An endpoint is one of:

- `axon+tcp://<host>[:<port>]`
- `wss://<host>[:<port>]/axon/v1`

For `axon+tcp`, the client opens a TLS-wrapped TCP connection to `<host>:<port>`. If `<port>` is absent, the default port is `4765`. The client MUST advertise TLS ALPN protocol `axon/1`; a full server MUST select `axon/1` for Axon TCP connections and MUST close the TLS connection if no supported ALPN protocol is negotiated.

For `wss`, the WebSocket path for draft version 0.1 is `/axon/v1`. If `<port>` is absent, the default port is `443`. The client MUST send `Sec-WebSocket-Protocol: axon.v1` in the opening handshake. The server MUST select `axon.v1`; if it cannot, it MUST reject the WebSocket upgrade.

Deployments MAY publish non-default hosts, ports, or additional discovery metadata, but those values are deployment configuration. The wire behavior after the Control Plane connection is established remains the same.

Common valid active pairs include:

| Pair | Control Plane | Pipeline |
| ---- | ------------- | -------- |
| 1 | TCP | WebRTC |
| 2 | WebSocket | WebRTC |

A client MAY implement both Control Plane transports and select the optimal pair at runtime, such as preferring WebSocket in browser environments and TCP in native applications.

## Channel Types

Within a session, traffic is multiplexed across typed logical channels:

| Channel Type | Transport (Primary) | Transport (Fallback) | Description |
| ------------ | ------------------- | -------------------- | ----------- |
| RPC | Pipeline (Data Channel) | Control Plane | Request/response calls |
| Event | Pipeline (Data Channel) | Control Plane | Push notifications, streams |
| Media | Pipeline (Media Track) | None | Audio/video tracks |

Media channels have no fallback; if the Pipeline is unavailable, media is suspended until it recovers.

## Server Architecture Requirements

A full Axon server implementing draft version 0.1 MUST:

- Accept Control Plane connections on TCP and WebSocket.
- Accept Pipeline connections via WebRTC, including Data Channel and Media Track.
- Maintain session state independently of any individual transport connection.

A constrained Axon server, such as an embedded implementation, MAY omit one Control Plane transport, provided it documents which transports are available and does not claim to implement the full server profile. A draft version 0.1 server MUST NOT advertise UDP as a Pipeline transport.
