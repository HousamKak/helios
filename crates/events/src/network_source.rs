//! /proc/net/tcp polling network source.
//!
//! Parses `/proc/net/tcp` and `/proc/net/tcp6` on a periodic interval,
//! diffs the connection set against the previous scan, emits
//! `TcpConnect` for new connections and `TcpClose` for departed ones.
//! Same shape as the procfs process source — coarse polling, replaced
//! by aya eBPF kprobes (`tcp_connect`, `tcp_close`) in Phase 2 for
//! kernel-level latency.
//!
//! PID lookup from socket inode (via `/proc/*/fd/*` readlink) is
//! deferred — too slow to do every poll. Phase 2 eBPF gives us PIDs
//! cheaply.

use std::collections::HashMap;
use std::time::Duration;

use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, now};
use tokio::sync::broadcast;

const DEFAULT_POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnKey {
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: String,
    pub remote_port: u16,
}

#[derive(Debug, Clone)]
pub struct ConnSnapshot {
    pub key: ConnKey,
    pub state: String,
    pub inode: u64,
}

/// Run the polling loop until the broadcast channel has no subscribers.
pub async fn run(tx: broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    let mut last = scan_all().unwrap_or_default();
    tracing::info!(initial = last.len(), "network source: initial scan");

    loop {
        tokio::time::sleep(DEFAULT_POLL).await;
        if tx.receiver_count() == 0 {
            return Ok(());
        }

        let current = match scan_all() {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(?err, "network scan failed; will retry");
                continue;
            }
        };

        for payload in diff_events(&last, &current) {
            let envelope = SystemEvent {
                id: generate_id(),
                timestamp: now(),
                source: EventSource::Procfs,
                correlation_id: None,
                causation_id: None,
                payload,
            };
            let _ = tx.send(envelope);
        }
        last = current;
    }
}

/// Snapshot all TCP sockets visible in /proc/net/tcp and /proc/net/tcp6.
fn scan_all() -> anyhow::Result<HashMap<ConnKey, ConnSnapshot>> {
    let mut out = HashMap::new();
    if let Ok(v4) = procfs::net::tcp() {
        for entry in v4 {
            let snap = snapshot_from(&entry);
            out.insert(snap.key.clone(), snap);
        }
    }
    if let Ok(v6) = procfs::net::tcp6() {
        for entry in v6 {
            let snap = snapshot_from(&entry);
            out.insert(snap.key.clone(), snap);
        }
    }
    Ok(out)
}

fn snapshot_from(entry: &procfs::net::TcpNetEntry) -> ConnSnapshot {
    let local_addr = entry.local_address.ip().to_string();
    let local_port = entry.local_address.port();
    let remote_addr = entry.remote_address.ip().to_string();
    let remote_port = entry.remote_address.port();
    ConnSnapshot {
        key: ConnKey {
            local_addr,
            local_port,
            remote_addr,
            remote_port,
        },
        state: format!("{:?}", entry.state).to_lowercase(),
        inode: entry.inode,
    }
}

/// Pure diff. Public for testing without touching /proc.
pub fn diff_events(
    prev: &HashMap<ConnKey, ConnSnapshot>,
    current: &HashMap<ConnKey, ConnSnapshot>,
) -> Vec<EventPayload> {
    let mut events = Vec::new();

    for (key, snap) in current {
        if !prev.contains_key(key) {
            events.push(EventPayload::TcpConnect {
                pid: None,
                local_addr: snap.key.local_addr.clone(),
                local_port: snap.key.local_port,
                remote_addr: snap.key.remote_addr.clone(),
                remote_port: snap.key.remote_port,
            });
        }
    }

    for key in prev.keys() {
        if !current.contains_key(key) {
            // We don't have a stable connection_id yet — Phase 2 eBPF
            // will give us one. For now, embed the 4-tuple as the id
            // so projection can correlate.
            let id = format!(
                "{}:{}-{}:{}",
                key.local_addr, key.local_port, key.remote_addr, key.remote_port
            );
            events.push(EventPayload::TcpClose { connection_id: id });
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(local: &str, lp: u16, remote: &str, rp: u16) -> ConnSnapshot {
        ConnSnapshot {
            key: ConnKey {
                local_addr: local.into(),
                local_port: lp,
                remote_addr: remote.into(),
                remote_port: rp,
            },
            state: "established".into(),
            inode: 1,
        }
    }

    #[test]
    fn new_connection_emits_connect() {
        let prev: HashMap<ConnKey, ConnSnapshot> = HashMap::new();
        let mut current = HashMap::new();
        let s = snap("127.0.0.1", 12345, "1.2.3.4", 443);
        current.insert(s.key.clone(), s);
        let evts = diff_events(&prev, &current);
        assert_eq!(evts.len(), 1);
        assert!(matches!(evts[0], EventPayload::TcpConnect { .. }));
    }

    #[test]
    fn departed_connection_emits_close() {
        let mut prev = HashMap::new();
        let s = snap("127.0.0.1", 12345, "1.2.3.4", 443);
        prev.insert(s.key.clone(), s);
        let current: HashMap<ConnKey, ConnSnapshot> = HashMap::new();
        let evts = diff_events(&prev, &current);
        assert_eq!(evts.len(), 1);
        assert!(matches!(evts[0], EventPayload::TcpClose { .. }));
    }

    #[test]
    fn no_change_emits_nothing() {
        let mut both = HashMap::new();
        let s = snap("127.0.0.1", 12345, "1.2.3.4", 443);
        both.insert(s.key.clone(), s);
        assert!(diff_events(&both, &both).is_empty());
    }
}
