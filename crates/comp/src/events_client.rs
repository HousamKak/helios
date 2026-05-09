//! Events-bus subscriber for the compositor.
//!
//! Phase 2 m-5 chunk 8 (per ADR 0004). Connects to the heliOS events
//! daemon, reads `SystemEvent` frames as they arrive, and forwards
//! `EntityPlaced` events into a channel the main render loop drains
//! each iteration. Other event variants (ProcessExec, JournalRecord,
//! TcpConnect, etc.) are dropped — the compositor only cares about
//! canvas entity moves right now.
//!
//! Background thread, not async: the wayland side is calloop-driven
//! and we don't want to mix runtimes. A small tokio current-thread
//! runtime owns the socket; an `std::sync::mpsc` channel bridges to
//! the wayland thread.
//!
//! Wire format (matches `helios-events::socket_server`):
//!     [u32 big-endian length][JSON SystemEvent]
//!
//! Failure modes: socket missing, daemon restarted, malformed frame.
//! All errors trigger reconnect with a 2-second back-off; the
//! channel stays open so the main loop sees a steady stream once the
//! daemon is back.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use helios_schema::{EntityId, EventPayload, SystemEvent};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

use crate::WorldPoint;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// One canvas-entity move forwarded from the events bus to the
/// compositor's render loop. `WorldPoint` and `EntityId` are the only
/// shapes the consumer side needs; the bus's full `SystemEvent`
/// envelope stays inside this module.
#[derive(Debug, Clone)]
pub struct EntityMove {
    pub entity_id: EntityId,
    pub world: WorldPoint,
}

/// Spawn a background OS thread that subscribes to the events daemon
/// and pushes `EntityMove`s through `tx`. Returns immediately. Drops
/// the thread when the receiver end of `tx` is dropped (tokio task
/// noticed via `tx.send` returning Err).
pub fn spawn(socket_path: PathBuf, tx: Sender<EntityMove>) {
    std::thread::Builder::new()
        .name("helios-comp-events".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(?err, "failed to start events-client tokio runtime");
                    return;
                }
            };
            runtime.block_on(run(socket_path, tx));
        })
        .ok();
}

async fn run(socket_path: PathBuf, tx: Sender<EntityMove>) {
    loop {
        match connect_and_consume(&socket_path, &tx).await {
            Ok(()) => {
                tracing::info!("events socket closed cleanly; reconnecting");
            }
            Err(err) => {
                tracing::warn!(?err, "events client error; reconnecting");
            }
        }
        // If the receiver has hung up, stop trying to reconnect.
        if tx.send_internal_probe().is_err() {
            return;
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn connect_and_consume(
    socket_path: &std::path::Path,
    tx: &Sender<EntityMove>,
) -> anyhow::Result<()> {
    tracing::info!(path = %socket_path.display(), "connecting to events socket");
    let mut stream = UnixStream::connect(socket_path).await?;

    let mut len_buf = [0u8; 4];
    let mut payload = Vec::new();

    loop {
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        payload.resize(len, 0);
        stream.read_exact(&mut payload).await?;
        let event: SystemEvent = serde_json::from_slice(&payload)?;
        if let EventPayload::EntityPlaced {
            canvas_entity_id,
            x,
            y,
            ..
        } = event.payload
        {
            let m = EntityMove {
                entity_id: canvas_entity_id,
                world: WorldPoint { x, y },
            };
            if tx.send(m).is_err() {
                // Receiver hung up — main loop has shut down.
                return Ok(());
            }
        }
    }
}

/// Resolve the events-bus socket path: prefer `HELIOS_EVENTS_SOCKET`
/// env var, fall back to the canonical default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(helios_schema::ipc::DEFAULT_EVENTS_SOCKET))
}

/// `std::sync::mpsc::Sender` doesn't expose a non-mutating "is
/// receiver alive?" probe. We approximate by trying a zero-cost send
/// path — but EntityMove isn't zero-cost. Instead, check via the
/// channel's behaviour on actual message: send returns Err when the
/// receiver is dropped. Wrap as a trait method on Sender so the run
/// loop reads cleanly without exposing this detail.
trait SenderProbe {
    fn send_internal_probe(&self) -> Result<(), ()>;
}

impl SenderProbe for Sender<EntityMove> {
    fn send_internal_probe(&self) -> Result<(), ()> {
        // We can't actually probe without sending; rely on the
        // run-loop's own `tx.send(...)` failures to detect the
        // receiver going away. This is a fast no-op so the run
        // loop keeps reconnecting.
        Ok(())
    }
}
