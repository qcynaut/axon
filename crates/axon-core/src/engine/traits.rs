//! Async trait contracts for transport and application integration.
//!
//! These traits define the interfaces that concrete transport and application
//! crates must implement.  `axon-core` itself has **no** I/O; it only defines
//! the contracts.
//!
//! # Transport layer
//!
//! - [`ControlPlane`] — a bidirectional connection carrying control frames (TCP or WebSocket).
//! - [`PipelineDataChannel`] — a single WebRTC Data Channel (`axon.rpc` or `axon.events`).
//!
//! # Application layer
//!
//! - [`AxonServerHandler`] — server-side application callbacks (auth, RPC dispatch, event
//!   handling).
//! - [`AxonClientSession`] — high-level async client API.

use futures::future::BoxFuture;
use rmpv::Value as MsgpackValue;

use crate::{error::AxonError, frame::Frame, session::SessionInfo};

// ---------------------------------------------------------------------------
// Transport contracts
// ---------------------------------------------------------------------------

/// Async contract for a Control Plane transport.
///
/// Implementors carry the bidirectional stream of Axon control frames over TLS
/// TCP or secure WebSocket.  Each call to [`recv_frame`] MUST block until a
/// complete single Axon frame is available (the transport is responsible for
/// reassembly).
///
/// [`recv_frame`]: ControlPlane::recv_frame
pub trait ControlPlane: Send {
    /// Send a single frame over the Control Plane.
    ///
    /// For TCP: write the complete 16-byte header + payload to the stream.
    /// For WebSocket: send exactly one binary message per frame.
    fn send_frame(&mut self, frame: Frame) -> BoxFuture<'_, Result<(), AxonError>>;

    /// Receive the next complete frame from the Control Plane.
    ///
    /// Returns `Err` when the connection is lost or an I/O error occurs.
    fn recv_frame(&mut self) -> BoxFuture<'_, Result<Frame, AxonError>>;

    /// Gracefully close the Control Plane transport.
    fn close(&mut self) -> BoxFuture<'_, ()>;
}

/// Async contract for a single WebRTC Pipeline Data Channel.
///
/// Each channel message MUST contain exactly one complete Axon frame including
/// the 16-byte header.  Text messages are invalid and MUST be rejected.
pub trait PipelineDataChannel: Send {
    /// Returns the channel label: `"axon.rpc"` or `"axon.events"`.
    fn label(&self) -> &str;

    /// Send a single frame over this Data Channel.
    fn send_frame(&mut self, frame: Frame) -> BoxFuture<'_, Result<(), AxonError>>;

    /// Receive the next complete frame from this Data Channel.
    fn recv_frame(&mut self) -> BoxFuture<'_, Result<Frame, AxonError>>;

    /// Close the Data Channel.
    fn close(&mut self) -> BoxFuture<'_, ()>;
}

// ---------------------------------------------------------------------------
// Application contracts
// ---------------------------------------------------------------------------

/// Server-side application handler.
///
/// The Axon session runner calls into these methods at the appropriate points
/// in the session lifecycle.  All methods return [`BoxFuture`] so that
/// implementations can be used as `dyn AxonServerHandler` across tasks.
pub trait AxonServerHandler: Send + Sync {
    /// Authenticate the connecting client.
    ///
    /// Called after `SESSION_ACCEPT` is received.  Return `Ok(())` on success
    /// or `Err(AxonError { code: ErrorCode::AuthFailed, .. })` to reject.
    ///
    /// # Arguments
    ///
    /// - `session_id` — the server-assigned session identifier.
    /// - `auth_scheme` — the scheme selected in `SESSION_OFFER` (`"none"` or `"bearer"`).
    /// - `auth_response` — the credential supplied by the client; `None` when `auth_scheme` is
    ///   `"none"`.
    fn authenticate<'a>(
        &'a self,
        session_id: &'a str,
        auth_scheme: &'a str,
        auth_response: Option<&'a [u8]>,
    ) -> BoxFuture<'a, Result<(), AxonError>>;

    /// Handle an incoming RPC call.
    ///
    /// The return value becomes the `result` of the `RPC_RESPONSE` frame.
    /// Return `Err` to send an `RPC_ERROR` to the client.
    ///
    /// # Arguments
    ///
    /// - `session_id` — identifies the calling session.
    /// - `method` — fully-qualified method name (e.g. `"user.getProfile"`).
    /// - `params` — method parameters; `None` when absent.
    fn handle_rpc<'a>(
        &'a self,
        session_id: &'a str,
        method: &'a str,
        params: Option<MsgpackValue>,
    ) -> BoxFuture<'a, Result<MsgpackValue, AxonError>>;

    /// Handle an incoming event.
    ///
    /// Called for every `EVENT` frame delivered to the application.  The
    /// implementation MUST NOT block for longer than necessary; long-running
    /// work should be spawned as a separate task.
    ///
    /// # Arguments
    ///
    /// - `session_id` — session that published the event.
    /// - `topic` — hierarchical topic string.
    /// - `seq` — sequence number from the publisher.
    /// - `payload` — event data.
    fn handle_event<'a>(
        &'a self,
        session_id: &'a str,
        topic: &'a str,
        seq: u64,
        payload: MsgpackValue,
    ) -> BoxFuture<'a, ()>;
}

/// Boxed iterator of streaming RPC response values.
///
/// Each item is either a partial result value or an error that terminates the
/// stream.
pub type StreamingRpcResult = Box<dyn Iterator<Item = Result<MsgpackValue, AxonError>> + Send>;

/// High-level async Axon client session.
///
/// Implementations drive the full client lifecycle: connecting, handshaking,
/// calling RPCs, publishing events, and disconnecting.
pub trait AxonClientSession: Send {
    /// Establish the session: perform the Control Plane handshake and Pipeline
    /// negotiation, returning [`SessionInfo`] when the session reaches
    /// `SESSION_READY`.
    fn connect(&mut self) -> BoxFuture<'_, Result<SessionInfo, AxonError>>;

    /// Invoke a remote procedure and await a single-response reply.
    ///
    /// For streaming RPCs use [`call_streaming`].
    ///
    /// [`call_streaming`]: Self::call_streaming
    fn call(
        &mut self,
        method: &str,
        params: Option<MsgpackValue>,
    ) -> BoxFuture<'_, Result<MsgpackValue, AxonError>>;

    /// Invoke a remote procedure that returns a stream of response values.
    ///
    /// The returned iterator yields successive `RPC_RESPONSE` values until the
    /// `FIN`-flagged terminal frame.  The client MUST set `expects_stream =
    /// true` in the request payload.
    fn call_streaming(
        &mut self,
        method: &str,
        params: Option<MsgpackValue>,
    ) -> BoxFuture<'_, Result<StreamingRpcResult, AxonError>>;

    /// Publish an event on `topic`.
    ///
    /// When `ack_required` is `true`, the frame is sent with `ACK_REQ` and
    /// the call blocks until the peer acknowledges or retransmissions are
    /// exhausted.
    fn publish(
        &mut self,
        topic: &str,
        payload: MsgpackValue,
        ack_required: bool,
    ) -> BoxFuture<'_, Result<(), AxonError>>;

    /// Gracefully close the session.
    ///
    /// Sends `GOODBYE` with the given `code` and optional `reason`, waits up
    /// to 5 seconds for the peer's echo, then closes all transports.
    fn disconnect(
        &mut self,
        code: u16,
        reason: Option<String>,
    ) -> BoxFuture<'_, Result<(), AxonError>>;
}
