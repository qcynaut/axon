# Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" are to be interpreted as described in RFC 2119.

| Term | Definition |
| ---- | ---------- |
| **Axon Endpoint** | Any participant in an Axon session, either client or server. |
| **Session** | The top-level logical association between a client and server, identified by a Session ID. |
| **Control Plane** | The reliable, ordered transport connection used for signaling and fallback traffic. Carried over TCP or WebSocket. |
| **Pipeline** | The primary high-throughput transport connection. In draft version 0.1, carried over WebRTC Data Channels for RPC and Events, and WebRTC Media Tracks for media. |
| **Carrier** | A concrete path that carries complete Axon frames: the Control Plane connection, the `axon.rpc` Data Channel, or the `axon.events` Data Channel. |
| **Channel** | A logical, typed, multiplexed stream within a session. Types: RPC, Event, Media. |
| **Frame** | The smallest unit of data exchange on the wire. |
| **Message** | An application-level unit composed of one or more frames. |
| **Principal** | The authenticated identity associated with a session, such as a user, service account, or anonymous identity. |
| **Session Resumption** | Re-establishing a lost Control Plane connection while preserving eligible server-side protocol state for the same Session ID. |
| **SDP** | Session Description Protocol, used during WebRTC negotiation. |
| **ICE** | Interactive Connectivity Establishment, used for WebRTC peer discovery. |
