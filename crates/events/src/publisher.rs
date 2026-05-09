//! Client-side helper for publishing events onto the bus.
//!
//! Symmetrical counterpart of `helios-store::events_client` (the
//! consumer-side client). Where that connects, reads frames, and
//! forwards them onto an mpsc channel, this connects, accepts events
//! from the local caller, and writes frames out to the events
//! daemon's ingress socket (m-8.1).
//!
//! Design choices:
//!   * **Lazy reconnect on publish.** `connect()` does the initial
//!     dial; if it fails, we still return a usable `EventsPublisher`
//!     with `state == Disconnected`. The next `publish()` call
//!     attempts to reconnect transparently. This keeps the compositor
//!     and the store from blocking on startup if the events daemon
//!     isn't up yet.
//!   * **Best-effort.** A `publish()` failure logs and returns; the
//!     caller never panics. Per the m-8 brief: "publishers should be
//!     best-effort, never blocking".
//!   * **`tokio::sync::Mutex`** for the stream state — `publish` is
//!     `&self` so multiple call sites can share one publisher
//!     (compositor's surface lifecycle handlers fire from different
//!     code paths).
//!   * **One connection, sequential writes.** Multiple in-flight
//!     publishes are serialized through the mutex. This is fine: the
//!     bus's per-publisher capacity is "anything we can fit into the
//!     subscriber broadcast in real time", which at v0.1 budgets is
//!     ~10 µs per event. Lock contention at this scale is irrelevant.
//!
//! Phase 2 m-8.2.

use std::path::PathBuf;

use helios_schema::SystemEvent;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// Publisher handle. Cheap to clone via `Arc`; share one across the
/// places in your service that emit events.
pub struct EventsPublisher {
    socket_path: PathBuf,
    /// `None` while disconnected. `connect()` and the reconnect-on-publish
    /// path swap a `Some` in.
    stream: Mutex<Option<UnixStream>>,
}

impl EventsPublisher {
    /// Open a publisher targeting the given socket. If the initial
    /// connect fails, returns a `Disconnected` publisher that will
    /// retry on the next `publish()`. This lets startup proceed even
    /// when the events daemon isn't up yet.
    pub async fn connect(socket_path: PathBuf) -> Self {
        let stream = match UnixStream::connect(&socket_path).await {
            Ok(s) => {
                tracing::info!(
                    path = %socket_path.display(),
                    "events publisher connected",
                );
                Some(s)
            }
            Err(err) => {
                tracing::warn!(
                    path = %socket_path.display(),
                    ?err,
                    "events publisher: initial connect failed; will retry on publish",
                );
                None
            }
        };
        Self {
            socket_path,
            stream: Mutex::new(stream),
        }
    }

    /// Send one event to the bus. Best-effort: on any failure (no
    /// connection, write error, serialize error) we drop the stream
    /// and return Err — the caller logs and moves on. The next
    /// `publish()` will try to reconnect.
    pub async fn publish(&self, event: &SystemEvent) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(event)?;
        let len = u32::try_from(bytes.len())?.to_be_bytes();

        let mut guard = self.stream.lock().await;
        if guard.is_none() {
            // Lazy reconnect.
            match UnixStream::connect(&self.socket_path).await {
                Ok(s) => {
                    tracing::info!(
                        path = %self.socket_path.display(),
                        "events publisher reconnected",
                    );
                    *guard = Some(s);
                }
                Err(err) => {
                    return Err(anyhow::anyhow!("events publisher: reconnect failed: {err}"));
                }
            }
        }

        let stream = guard.as_mut().expect("connected above");
        // Two writes followed by a flush. If any fails, we drop the
        // stream so the next publish reconnects from scratch.
        if let Err(err) = stream.write_all(&len).await {
            *guard = None;
            return Err(err.into());
        }
        if let Err(err) = stream.write_all(&bytes).await {
            *guard = None;
            return Err(err.into());
        }
        if let Err(err) = stream.flush().await {
            *guard = None;
            return Err(err.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket_ingress;
    use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, now};
    use tempfile::TempDir;
    use tokio::sync::broadcast;
    use tokio::time::{Duration, timeout};

    fn sample_event() -> SystemEvent {
        SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Procfs,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::ProcessExit {
                pid: 42,
                exit_code: 0,
            },
        }
    }

    #[tokio::test]
    async fn publish_roundtrips_through_ingress_socket() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events-in.sock");

        let (tx, mut rx) = broadcast::channel::<SystemEvent>(16);
        let server_path = path.clone();
        let server_tx = tx.clone();
        tokio::spawn(async move {
            let _ = socket_ingress::serve_ingress(server_path, server_tx).await;
        });

        // Give the listener a beat to bind. UnixListener::bind is sync
        // but the spawn boundary needs a poll cycle.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let publisher = EventsPublisher::connect(path).await;
        let event = sample_event();
        publisher.publish(&event).await.unwrap();

        let received = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("did not receive event in time")
            .expect("broadcast channel closed");
        assert_eq!(received.id, event.id);
    }
}
