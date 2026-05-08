//! heliOS events bus — library surface.
//!
//! Per `docs/research/04-observability.md`, the v0.1 architecture fans
//! Linux observability sources (eBPF, procfs, fanotify, zbus, journal,
//! netlink) into a single `tokio::sync::broadcast` channel and exposes
//! it on a Unix socket using length-prefixed JSON encoding.
//!
//! Phase 0 ships only the **procfs** source — a polling diff against
//! `/proc` that emits `ProcessExec` and `ProcessExit` events. Phase 1
//! adds the Unix socket fanout so the entity store can subscribe.
//! Future phases replace procfs with aya eBPF on
//! `sched_process_exec`/`sched_process_exit` for kernel-level latency.

pub use helios_schema::{EventPayload, EventSource, SystemEvent};

#[cfg(target_os = "linux")]
pub mod procfs_source;

#[cfg(target_os = "linux")]
pub mod socket_server;

/// v0.1 event-rate budget. See `PLAN.md` §4 and observability research.
pub const TARGET_SUSTAINED_EVENTS_PER_SEC: usize = 10_000;
pub const TARGET_BURST_EVENTS_PER_SEC: usize = 50_000;
pub const BROADCAST_CAPACITY: usize = 16_384;
pub const MPSC_FRONT_CAPACITY: usize = 4_096;

/// Default Unix-socket path the bus listens on. Subscribers connect here.
pub const DEFAULT_SOCKET_PATH: &str = helios_schema::ipc::DEFAULT_EVENTS_SOCKET;

/// Default polling interval for the procfs source. Phase 0 quality —
/// half-second granularity is fine for "see the canvas update when I
/// run a command" but misses processes that exec+exit in under 500 ms.
pub const PROCFS_POLL_INTERVAL_MS: u64 = 500;
