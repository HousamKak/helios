//! Thin wrapper around `helios_events::publisher::EventsPublisher`.
//!
//! Phase 2 m-8.4. The store needs to emit `EntityPlaced` events
//! whenever a `MoveEntity` request lands, so the compositor can pick
//! up the move from the bus and visually reposition the window. This
//! module factors the "open a publisher, share it across requests"
//! concern out of `server.rs` and `main.rs` so both places see the
//! same `Arc<EventsPublisher>`.
//!
//! Why a separate module: keeps the publisher path obvious for the
//! next person reading `server.rs` ("oh, MoveEntity requires
//! `publisher`, that comes from publisher.rs at startup").

use std::path::PathBuf;
use std::sync::Arc;

use helios_events::publisher::EventsPublisher;

/// Resolve the publisher socket path the same way the events crate
/// does — env override, falling back to the canonical default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_INGRESS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(helios_schema::ipc::DEFAULT_EVENTS_INGRESS_SOCKET))
}

/// Open a publisher targeting the events daemon's ingress socket.
/// Returns an `Arc<EventsPublisher>` so the same connection is
/// shared across the store's many request-handler tasks.
///
/// The initial connect is lazy — if the events daemon isn't up yet,
/// the publisher reports as `Disconnected` and the next `publish()`
/// reconnects. This means starting the store before the events
/// daemon is fine.
pub async fn connect() -> Arc<EventsPublisher> {
    Arc::new(EventsPublisher::connect(socket_path_from_env()).await)
}
