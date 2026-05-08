//! Phase-0 procfs polling source.
//!
//! Periodically scans `/proc`, diffs the PID set against the previous
//! scan, emits `ProcessExec` for newly-observed PIDs and `ProcessExit`
//! for departed PIDs. Coarse-grained — depends on the poll interval —
//! and misses processes that exec and exit between scans. That's why
//! Phase 1 replaces this with aya eBPF on `sched_process_exec` /
//! `sched_process_exit`, which is kernel-level and lossless.
//!
//! For Phase 0 this is good enough to validate the events bus
//! end-to-end without any kernel modules required.

use std::collections::HashMap;
use std::time::Duration;

use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, now};
use tokio::sync::broadcast;

/// One-row snapshot of a process at the moment of scan. Field set is
/// deliberately small — anything richer (cgroup, systemd unit, RSS,
/// CPU%) is enrichment that lands when the entity store wires up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub pid: i32,
    pub ppid: i32,
    pub comm: String,
    pub cmdline: String,
    pub exe: Option<String>,
    pub uid: u32,
    pub gid: u32,
}

/// Run the procfs poller until the broadcast channel is closed or an
/// unrecoverable scan error occurs. Caller owns shutdown.
pub async fn run(tx: broadcast::Sender<SystemEvent>, interval: Duration) -> anyhow::Result<()> {
    let mut last_scan: HashMap<i32, ProcessSnapshot> = scan_proc()?;
    tracing::info!(
        initial_pids = last_scan.len(),
        "procfs source: initial scan complete"
    );

    loop {
        tokio::time::sleep(interval).await;

        let current = match scan_proc() {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(?err, "procfs scan failed; will retry next tick");
                continue;
            }
        };

        for payload in diff_events(&last_scan, &current) {
            let envelope = SystemEvent {
                id: generate_id(),
                timestamp: now(),
                source: EventSource::Procfs,
                correlation_id: None,
                causation_id: None,
                payload,
            };
            // Receiver-less broadcasts are fine — Send returns Err
            // only when there are zero subscribers, which is harmless.
            let _ = tx.send(envelope);
        }

        last_scan = current;
    }
}

/// One scan of `/proc`. Defensive — any process that fails to read
/// (it likely exited mid-iteration) is skipped, never aborts the scan.
fn scan_proc() -> anyhow::Result<HashMap<i32, ProcessSnapshot>> {
    let mut out = HashMap::new();
    for proc_result in procfs::process::all_processes()? {
        let Ok(proc) = proc_result else { continue };
        let Ok(stat) = proc.stat() else { continue };

        let cmdline = proc.cmdline().unwrap_or_default().join(" ");
        let exe = proc.exe().ok().and_then(|p| p.to_str().map(String::from));
        let (uid, gid) = proc.status().map(|s| (s.ruid, s.rgid)).unwrap_or((0, 0));

        out.insert(
            stat.pid,
            ProcessSnapshot {
                pid: stat.pid,
                ppid: stat.ppid,
                comm: stat.comm.clone(),
                cmdline,
                exe,
                uid,
                gid,
            },
        );
    }
    Ok(out)
}

/// Pure diff function — given two snapshots, produce the events.
/// Public for testing without async machinery.
pub fn diff_events(
    prev: &HashMap<i32, ProcessSnapshot>,
    current: &HashMap<i32, ProcessSnapshot>,
) -> Vec<EventPayload> {
    let mut events = Vec::new();

    // New PIDs => ProcessExec.
    for (pid, snap) in current {
        if !prev.contains_key(pid) {
            events.push(EventPayload::ProcessExec {
                pid: *pid,
                ppid: Some(snap.ppid),
                comm: snap.comm.clone(),
                cmdline: snap.cmdline.clone(),
                exe: snap.exe.clone(),
                uid: snap.uid,
                gid: snap.gid,
                cgroup: None, // enrichment lands when store wires up
            });
        }
    }

    // Departed PIDs => ProcessExit. Polling can't observe exit code;
    // Phase 1 eBPF will give us the real value.
    for pid in prev.keys() {
        if !current.contains_key(pid) {
            events.push(EventPayload::ProcessExit {
                pid: *pid,
                exit_code: 0,
            });
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pid: i32, comm: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            ppid: 1,
            comm: comm.to_string(),
            cmdline: comm.to_string(),
            exe: None,
            uid: 1000,
            gid: 1000,
        }
    }

    #[test]
    fn no_change_emits_nothing() {
        let mut prev = HashMap::new();
        prev.insert(100, snap(100, "init"));
        let current = prev.clone();
        let events = diff_events(&prev, &current);
        assert!(events.is_empty());
    }

    #[test]
    fn new_pid_emits_exec() {
        let prev: HashMap<i32, ProcessSnapshot> = HashMap::new();
        let mut current = HashMap::new();
        current.insert(200, snap(200, "bash"));
        let events = diff_events(&prev, &current);
        assert_eq!(events.len(), 1);
        match &events[0] {
            EventPayload::ProcessExec { pid, comm, .. } => {
                assert_eq!(*pid, 200);
                assert_eq!(comm, "bash");
            }
            other => panic!("expected ProcessExec, got {other:?}"),
        }
    }

    #[test]
    fn departed_pid_emits_exit() {
        let mut prev = HashMap::new();
        prev.insert(300, snap(300, "vim"));
        let current: HashMap<i32, ProcessSnapshot> = HashMap::new();
        let events = diff_events(&prev, &current);
        assert_eq!(events.len(), 1);
        match &events[0] {
            EventPayload::ProcessExit { pid, .. } => {
                assert_eq!(*pid, 300);
            }
            other => panic!("expected ProcessExit, got {other:?}"),
        }
    }

    #[test]
    fn mixed_diff_emits_both() {
        let mut prev = HashMap::new();
        prev.insert(100, snap(100, "init"));
        prev.insert(200, snap(200, "bash"));

        let mut current = HashMap::new();
        current.insert(100, snap(100, "init")); // unchanged
        current.insert(300, snap(300, "vim")); // new
        // 200 departed

        let events = diff_events(&prev, &current);
        assert_eq!(events.len(), 2);
        let exec_count = events
            .iter()
            .filter(|e| matches!(e, EventPayload::ProcessExec { pid: 300, .. }))
            .count();
        let exit_count = events
            .iter()
            .filter(|e| matches!(e, EventPayload::ProcessExit { pid: 200, .. }))
            .count();
        assert_eq!(exec_count, 1, "exec for new pid 300");
        assert_eq!(exit_count, 1, "exit for departed pid 200");
    }
}
