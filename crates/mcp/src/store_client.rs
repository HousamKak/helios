//! One-shot client for the entity store's Unix socket.
//!
//! Each call opens a fresh connection, sends one line-delimited
//! request, reads one line of response, closes. Per-call connection
//! overhead is negligible relative to MCP turn cadence.

use std::path::Path;

use helios_schema::ipc::{StoreRequest, StoreResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub async fn call(socket_path: &Path, request: StoreRequest) -> anyhow::Result<StoreResponse> {
    let stream = UnixStream::connect(socket_path).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    let mut req_line = serde_json::to_string(&request)?;
    req_line.push('\n');
    write.write_all(req_line.as_bytes()).await?;
    write.flush().await?;

    let mut response_line = String::new();
    let n = reader.read_line(&mut response_line).await?;
    if n == 0 {
        anyhow::bail!("store socket closed before responding");
    }
    let response: StoreResponse = serde_json::from_str(response_line.trim())?;
    Ok(response)
}

/// Resolve the store socket path: prefer `HELIOS_STORE_SOCKET` env
/// var, fall back to the canonical default.
pub fn socket_path_from_env() -> std::path::PathBuf {
    std::env::var_os("HELIOS_STORE_SOCKET")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(helios_schema::ipc::DEFAULT_STORE_SOCKET))
}
