# Axon Protocol Specification

Draft version: 0.1

Generated: May 2026

Axon is a unified application-layer communication protocol for RPC, real-time events, and media transport. The specification is organized as topic-focused documents rather than as a single RFC-style memo.

Draft version 0.1 targets TLS-wrapped TCP and secure WebSocket for the Control Plane, with WebRTC for the Pipeline.

## Documents

1. [Introduction](00-introduction.md)
2. [Terminology](01-terminology.md)
3. [Architecture Overview](02-architecture.md)
4. [Connection Lifecycle and Handshake](03-connection-lifecycle.md)
5. [Message Framing and Wire Format](04-message-framing.md)
6. [Channel Semantics](05-channel-semantics.md)
7. [Error Handling and Reconnection](06-error-handling-and-reconnection.md)
8. [Security Considerations](07-security-considerations.md)
9. [Implementation Notes](08-implementation-notes.md)
10. [Appendices (Frame Reference, Example Flows)](09-appendices.md)
11. [Implementation Profile and Test Vectors](10-implementation-profile.md)

## Status

This is an early-stage draft specification. It does not represent a finalized standard. Distribution is unrestricted.
