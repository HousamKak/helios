//! systemd journal tail source.
//!
//! Spawns `journalctl --follow --output=json --lines=0` as a child
//! process and parses each line of JSON output into a
//! `JournalRecord` event. Pure Rust + a subprocess — no libsystemd
//! FFI needed for v0.1. Reconnects (re-spawns) if `journalctl` ever
//! exits.
//!
//! Each journal entry is a JSON object with keys like:
//! `MESSAGE`, `_SYSTEMD_UNIT`, `PRIORITY`, `_HOSTNAME`, `_PID`. We
//! extract the three the schema cares about; the rest is dropped.

use std::process::Stdio;
use std::time::Duration;

use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, now};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

const RESPAWN_DELAY: Duration = Duration::from_secs(2);

/// Run the journal-tailer until the broadcast channel is closed.
pub async fn run(tx: broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    loop {
        match run_once(&tx).await {
            Ok(()) => tracing::warn!("journalctl exited cleanly; respawning"),
            Err(err) => tracing::warn!(?err, "journalctl error; respawning"),
        }
        if tx.receiver_count() == 0 {
            return Ok(());
        }
        tokio::time::sleep(RESPAWN_DELAY).await;
    }
}

async fn run_once(tx: &broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    let mut child = Command::new("journalctl")
        .arg("--follow")
        .arg("--output=json")
        .arg("--lines=0")
        .arg("--no-pager")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("journalctl produced no stdout handle"))?;
    let mut lines = BufReader::new(stdout).lines();

    tracing::info!("journal source: tailing");

    while let Some(line) = lines.next_line().await? {
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(err) => {
                tracing::debug!(?err, "skipping non-JSON journal line");
                continue;
            }
        };

        let unit = entry
            .get("_SYSTEMD_UNIT")
            .and_then(|v| v.as_str())
            .map(String::from);

        let priority = entry
            .get("PRIORITY")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(6); // info

        let message = entry
            .get("MESSAGE")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let envelope = SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Journald,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::JournalRecord {
                unit,
                priority,
                message,
            },
        };

        let _ = tx.send(envelope);
    }

    // Reap the child to avoid leaving a zombie.
    let _ = child.wait().await;
    Ok(())
}
