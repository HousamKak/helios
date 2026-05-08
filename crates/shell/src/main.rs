//! heliOS user shell — Phase 1.
//!
//! On a real heliOS install, the login flow `exec`s `helios-shell`.
//! This binary writes the Claude Code MCP config (so the agent knows
//! about `helios-mcp`), sets a few `HELIOS_*` env vars, and then
//! `exec`s `claude`. From the user's perspective, login lands them
//! directly in a Claude Code session with the heliOS tool surface
//! attached — no bash prompt at all.
//!
//! Env overrides:
//!   * `HELIOS_CLAUDE_BIN`  — path to `claude` (default: just "claude")
//!   * `HELIOS_MCP_BIN`     — path to `helios-mcp` (default: /usr/local/bin/helios-mcp)
//!   * `HELIOS_MCP_CONFIG`  — alternative config path (default: $HOME/.config/claude-code/mcp.json)
//!
//! Dev usage:
//!
//! ```sh
//! cargo run -p helios-shell
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-shell: Linux-only past Phase 0. Build runs on other \
         platforms; the daemon does not."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let claude_bin = std::env::var("HELIOS_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());

    if let Err(err) = write_mcp_config_if_missing() {
        tracing::warn!(?err, "failed to write Claude Code MCP config; continuing");
    }

    tracing::info!(claude_bin = %claude_bin, "exec claude");

    // exec replaces this process with claude. Returns only on error.
    let err = std::process::Command::new(&claude_bin)
        .env("HELIOS_SHELL", "1")
        .env(
            "HELIOS_STORE_SOCKET",
            std::env::var_os("HELIOS_STORE_SOCKET")
                .unwrap_or_else(|| helios_schema::ipc::DEFAULT_STORE_SOCKET.into()),
        )
        .env(
            "HELIOS_EVENTS_SOCKET",
            std::env::var_os("HELIOS_EVENTS_SOCKET")
                .unwrap_or_else(|| helios_schema::ipc::DEFAULT_EVENTS_SOCKET.into()),
        )
        .exec();

    Err(anyhow::anyhow!(
        "failed to exec '{claude_bin}': {err}. Is Claude Code installed and on PATH?"
    ))
}

#[cfg(target_os = "linux")]
fn write_mcp_config_if_missing() -> anyhow::Result<()> {
    use std::path::PathBuf;

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME not set"))?;

    let config_path: PathBuf = std::env::var_os("HELIOS_MCP_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config/claude-code/mcp.json"));

    if config_path.exists() {
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mcp_bin =
        std::env::var("HELIOS_MCP_BIN").unwrap_or_else(|_| "/usr/local/bin/helios-mcp".to_string());

    let config = serde_json::json!({
        "mcpServers": {
            "helios": {
                "command": mcp_bin,
                "args": [],
                "env": {}
            }
        }
    });

    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    tracing::info!(path = %config_path.display(), "wrote Claude Code MCP config");
    Ok(())
}
