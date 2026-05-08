//! Client side of the events bus Unix socket.
//!
//! Connects to the events daemon, reads `[u32 BE length][JSON
//! SystemEvent]` frames in a loop, hands each event off to the caller
//! through an mpsc channel. Reconnects on disconnect — events bus may
//! restart for any reason, the store should pick up where it left off.

use std::path::{Path, PathBuf};
use std::time::Duration;

use helios_schema::SystemEvent;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;

const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Connect-and-read loop. Forwards every event to `tx`. Returns
/// when `tx` is closed (the projector dropped its receiver).
pub async fn run(socket_path: PathBuf, tx: mpsc::Sender<SystemEvent>) -> anyhow::Result<()> {
    loop {
        match connect_and_consume(&socket_path, &tx).await {
            Ok(()) => {
                tracing::info!("events socket closed cleanly; reconnecting");
            }
            Err(err) => {
                tracing::warn!(?err, "events client error; reconnecting");
            }
        }
        if tx.is_closed() {
            return Ok(());
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn connect_and_consume(
    socket_path: &Path,
    tx: &mpsc::Sender<SystemEvent>,
) -> anyhow::Result<()> {
    tracing::info!(path = %socket_path.display(), "connecting to events socket");
    let mut stream = UnixStream::connect(socket_path).await?;

    let mut len_buf = [0u8; 4];
    let mut payload_buf = Vec::new();

    loop {
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        payload_buf.resize(len, 0);
        stream.read_exact(&mut payload_buf).await?;

        let event: SystemEvent = serde_json::from_slice(&payload_buf)?;
        if tx.send(event).await.is_err() {
            return Ok(());
        }
    }
}

/// Resolve the events-bus socket path: prefer `HELIOS_EVENTS_SOCKET`
/// env var, fall back to the canonical default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_EVENTS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(helios_schema::ipc::DEFAULT_EVENTS_SOCKET).to_path_buf())
}
