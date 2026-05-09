//! Events-bus publisher for the compositor.
//!
//! Phase 2 m-8.3. Symmetrical counterpart of `events_client.rs`:
//! that one consumes events from the bus, this one emits onto it.
//! Same architectural pattern — background OS thread runs a small
//! tokio current-thread runtime; a `std::sync::mpsc` channel bridges
//! from the calloop-driven wayland thread (which can't be async) to
//! the publisher.
//!
//! Compositor handlers (`XdgShellHandler::new_toplevel`,
//! `XwmHandler::surface_associated`, etc.) are sync — they fire from
//! the calloop event loop. They `events_tx.send(event)` synchronously
//! (non-blocking, std::sync::mpsc is unbounded) and the publisher
//! thread drains and forwards to the events daemon's ingress socket.
//!
//! Failure modes: events daemon down, ingress socket missing, bus
//! restart. All recovered by `EventsPublisher::publish`'s
//! reconnect-on-failure logic. The channel stays open so events keep
//! flowing once the daemon is back.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use helios_events::publisher::EventsPublisher;
use helios_schema::SystemEvent;

/// Spawn a background OS thread that owns an `EventsPublisher` and
/// drains `rx`, calling `publisher.publish(&event)` for each. Returns
/// immediately. The thread exits when the sender end of `rx` is
/// dropped (`recv` returns Err).
pub fn spawn(socket_path: PathBuf, rx: Receiver<SystemEvent>) {
    std::thread::Builder::new()
        .name("helios-comp-publisher".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(?err, "failed to start events-publisher tokio runtime");
                    return;
                }
            };
            runtime.block_on(run(socket_path, rx));
        })
        .ok();
}

async fn run(socket_path: PathBuf, rx: Receiver<SystemEvent>) {
    let publisher = EventsPublisher::connect(socket_path).await;
    loop {
        // Blocking recv() inside the async runtime is OK because this
        // is the only thing this thread does — no other tasks compete
        // for the runtime worker. Same pattern events_client uses.
        let event = match rx.recv() {
            Ok(e) => e,
            Err(_) => {
                tracing::info!("events-publisher: sender dropped, exiting");
                return;
            }
        };
        if let Err(err) = publisher.publish(&event).await {
            tracing::debug!(
                ?err,
                "events-publisher: publish failed (will retry on next event)"
            );
        }
    }
}

/// Resolve the publisher socket path: prefer
/// `HELIOS_EVENTS_INGRESS_SOCKET` env var, fall back to the canonical
/// default. Same env var the events daemon uses for its ingress
/// listener — keeps both ends in sync at deploy time.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_INGRESS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(helios_schema::ipc::DEFAULT_EVENTS_INGRESS_SOCKET))
}
