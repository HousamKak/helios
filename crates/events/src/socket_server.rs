//! Unix socket fanout for the events bus.
//!
//! The procfs source feeds a `tokio::sync::broadcast::Sender`. This
//! module runs the listener side: it owns the socket file, accepts
//! incoming subscribers, and gives each one its own `broadcast::Receiver`
//! plus a writer task that frames events on the wire.
//!
//! Wire format: `[u32 BE length][postcard-encoded SystemEvent]`. Length
//! is `u32` because a `SystemEvent` is bounded by `EventPayload`'s
//! variants, all of which serialize to under a few KB.
//!
//! Per `docs/research/04-observability.md`, the broadcast-and-fanout
//! shape (bounded mpsc front per producer + single broadcast + one
//! drainer per subscriber) is the v0.1 budget at 10k events/sec.

use std::path::{Path, PathBuf};

use helios_schema::SystemEvent;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

/// Run the socket fanout until the broadcast channel is closed or the
/// listener errors fatally. Caller owns shutdown via dropping the
/// broadcast::Sender.
pub async fn serve(socket_path: PathBuf, tx: broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    if let Some(parent) = socket_path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
    }
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!(path = %socket_path.display(), "events socket listening");

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(?err, "accept failed; continuing");
                continue;
            }
        };
        let rx = tx.subscribe();
        tokio::spawn(async move {
            if let Err(err) = handle_subscriber(stream, rx).await {
                tracing::debug!(?err, "subscriber dropped");
            }
        });
    }
}

async fn handle_subscriber(
    mut stream: UnixStream,
    mut rx: broadcast::Receiver<SystemEvent>,
) -> anyhow::Result<()> {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let bytes = postcard::to_allocvec(&event)?;
                let len = u32::try_from(bytes.len())?;
                stream.write_all(&len.to_be_bytes()).await?;
                stream.write_all(&bytes).await?;
                stream.flush().await?;
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(dropped = n, "subscriber lagged; events skipped");
            }
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

/// Resolve the socket path: prefer `HELIOS_EVENTS_SOCKET` env var, fall
/// back to the canonical default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(helios_schema::ipc::DEFAULT_EVENTS_SOCKET).to_path_buf())
}
