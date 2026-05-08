//! heliOS MCP gateway — Phase 1.
//!
//! Reads JSON-RPC 2.0 messages from stdin, writes responses to stdout.
//! Newline-delimited (one JSON object per line) per the MCP stdio
//! transport spec.
//!
//! Tools backing the entity store: helios_ping, helios_list_processes,
//! helios_get_process, helios_list_recent_events, helios_stats.
//!
//! Logging goes to stderr (Claude Code captures it for diagnostics);
//! stdout is reserved for protocol output.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("helios-mcp: Linux-only past Phase 0.");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use helios_mcp::protocol::{JsonRpcRequest, JsonRpcResponse};
    use std::io::Write as _;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let store_socket = helios_mcp::store_client::socket_path_from_env();
    tracing::info!(socket = %store_socket.display(), "helios-mcp started");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            tracing::info!("stdin closed; shutting down");
            return Ok(());
        }

        let request: JsonRpcRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(err) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("parse error: {err}"));
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        let is_notification = request.is_notification();
        let id = request.id.clone();

        let response_value = handle_request(request, &store_socket).await;

        if is_notification {
            // Spec: do not reply to notifications. Done.
            continue;
        }

        let response = match response_value {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(err) => JsonRpcResponse::error(id, -32000, err.to_string()),
        };

        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
        std::io::stdout().flush().ok();
    }
}

#[cfg(target_os = "linux")]
async fn handle_request(
    req: helios_mcp::protocol::JsonRpcRequest,
    store_socket: &std::path::Path,
) -> anyhow::Result<serde_json::Value> {
    use serde_json::json;

    match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "helios-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),

        "tools/list" => Ok(json!({
            "tools": helios_mcp::tools::definitions()
        })),

        "tools/call" => {
            let params = req
                .params
                .ok_or_else(|| anyhow::anyhow!("missing params"))?;
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing tool name"))?
                .to_string();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            let store_request = helios_mcp::tools::build_store_request(&name, &arguments)?;
            let store_response =
                helios_mcp::store_client::call(store_socket, store_request).await?;
            let value = serde_json::to_value(&store_response)?;

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                }],
                "isError": false
            }))
        }

        "ping" => Ok(json!({})),

        "notifications/initialized" | "notifications/cancelled" => Ok(json!({})),

        other => anyhow::bail!("method not supported: {other}"),
    }
}
