//! Publisher-side ingress for the events bus.
//!
//! Symmetrical counterpart of `socket_server.rs`. Where that module
//! fans events OUT to subscribers, this one fans events IN from
//! external publishers (helios-comp, helios-store, future skills /
//! applets) into the same `broadcast::Sender` the in-process
//! sources (procfs / dbus / journal / network) feed. From the
//! perspective of subscribers, in-process and external events are
//! indistinguishable — they're all just `SystemEvent`s on the same
//! channel.
//!
//! Wire format: `[u32 BE length][JSON-encoded SystemEvent]`. Same
//! as the subscriber side.
//!
//! Phase 2 m-8.1. Single ingress socket, multiple publisher
//! connections. Per ADR-equivalent guidance in the m-8 briefing:
//! v0.1 is single-trusted-user; per-publisher capability gating is
//! post-v0.1.
//!
//! Failure modes:
//!   * Malformed frame from a publisher → drop the connection, log,
//!     keep accepting new publishers.
//!   * `tx.send` returns Err only when there are no subscribers, and
//!     the broadcast crate treats that as non-fatal — events
//!     submitted with no listeners just disappear, which is the
//!     desired behaviour (no buffering on the bus side).

use std::path::{Path, PathBuf};

use helios_schema::SystemEvent;
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

/// Run the publisher-ingress listener. Owns the socket file. Each
/// accepted connection is a publisher; we spawn a per-connection
/// reader task that frames events and forwards them onto `tx`.
pub async fn serve_ingress(
    socket_path: PathBuf,
    tx: broadcast::Sender<SystemEvent>,
) -> anyhow::Result<()> {
    if let Some(parent) = socket_path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!(path = %socket_path.display(), "events ingress socket listening");

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(?err, "ingress accept failed; continuing");
                continue;
            }
        };
        let publisher_tx = tx.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_publisher(stream, publisher_tx).await {
                // Most errors here are clean "publisher closed the
                // socket" or "sent garbage". Log at debug so the
                // common case doesn't spam.
                tracing::debug!(?err, "publisher dropped");
            }
        });
    }
}

async fn handle_publisher(
    mut stream: UnixStream,
    tx: broadcast::Sender<SystemEvent>,
) -> anyhow::Result<()> {
    let mut len_buf = [0u8; 4];
    let mut payload = Vec::new();
    loop {
        // Read the 4-byte big-endian length prefix.
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        // Bound the per-frame allocation so a malicious publisher
        // can't claim a 4 GB frame and OOM us. Real `SystemEvent`
        // values stay well under 64 KB even with verbose payloads.
        const MAX_FRAME_BYTES: usize = 1 << 20; // 1 MiB
        if len > MAX_FRAME_BYTES {
            anyhow::bail!("publisher frame too large: {len} bytes");
        }
        payload.resize(len, 0);
        stream.read_exact(&mut payload).await?;

        let event: SystemEvent = serde_json::from_slice(&payload)?;
        // `send` returns Err only when there are zero receivers; that's
        // not a publisher-side problem, so swallow it.
        let _ = tx.send(event);
    }
}

/// Resolve the ingress socket path: prefer
/// `HELIOS_EVENTS_INGRESS_SOCKET` env var, fall back to the canonical
/// default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_INGRESS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(helios_schema::ipc::DEFAULT_EVENTS_INGRESS_SOCKET).to_path_buf()
        })
}
