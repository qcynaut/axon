# Introduction

Axon is a unified application-layer communication protocol designed to consolidate multiple transport paradigms: remote procedure calls (RPC), real-time event streaming, and media transport (audio/video). Axon eliminates the need for applications to manage separate protocols such as WebSocket, JSON-RPC, gRPC, and WebRTC concurrently.

Axon defines two logical planes per peer:

- **Control Plane**: signaling, session management, and transport fallback.
- **Pipeline**: high-throughput RPC, event delivery, and media transport.

This specification defines the architecture, connection lifecycle, message framing, channel semantics, and error-handling behavior of the Axon protocol.

## Motivation

Modern networked applications frequently require three distinct communication patterns simultaneously:

- **RPC**: Request/response calls for application logic, often served via JSON-RPC or gRPC.
- **Events**: Real-time push notifications from server to client, or bidirectional event streams, typically over WebSocket.
- **Media**: Audio and video streams with latency-sensitive delivery, typically over WebRTC.

Managing these systems separately introduces operational complexity: separate connection lifecycles, divergent authentication flows, inconsistent error models, and multiplied infrastructure surface area. Axon addresses this by unifying all three patterns under a single protocol with a well-defined layered architecture.

## Design Goals

- **Unified session**: A single Axon session encompasses RPC, events, and media, identified by one session identifier.
- **Transport flexibility**: Clients may connect using different transport pairs depending on their runtime environment, such as browser or native application.
- **Graceful degradation**: When the Pipeline is unavailable, traffic falls back to the Control Plane.
- **Extensibility**: Message types and channel kinds are versioned and extensible without breaking existing implementations.

## Scope

This specification covers:

- The logical architecture of an Axon endpoint.
- The connection lifecycle from handshake to teardown.
- Message framing and wire format for the Control Plane.
- Channel semantics for RPC, Event, and Media channels.
- Error handling and reconnection strategies.
- A draft version 0.1 implementation profile and interoperability test vectors.

Transport-specific bindings such as TCP, WebSocket, WebRTC Data Channel, and WebRTC Media Track are described where necessary. The underlying transport protocols themselves are not redefined by this document set.

For draft version 0.1, Axon standardizes the following implementation profile:

- Control Plane over TLS-wrapped TCP.
- Control Plane over secure WebSocket (`wss://`).
- Pipeline over WebRTC Data Channels and WebRTC Media Tracks.

UDP Pipeline transport is out of scope for draft version 0.1 and is reserved for a future transport binding.
