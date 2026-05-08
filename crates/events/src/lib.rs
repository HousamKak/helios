//! heliOS events bus — library surface.
//!
//! Per `docs/research/04-observability.md`, the v0.1 architecture fans
//! Linux observability sources (eBPF, procfs, fanotify, zbus, journal,
//! netlink) into a single `tokio::sync::broadcast` channel and exposes
//! it on a Unix socket using length-prefixed JSON encoding.
//!
//! Phase 1 sources currently wired:
//!   * **procfs**         — process exec/exit (polling diff against /proc)
//!   * **socket fanout**  — Unix-socket subscribers (helios-store etc.)
//!   * **D-Bus signals**  — generic system-bus signal listener
//!   * **journal tail**   — systemd journal records
//!   * **network**        — TCP connection lifecycle from /proc/net/tcp
//!
//! Phase 2 will add aya eBPF (kernel-level latency for exec/file/tcp),
//! fanotify (file events), and typed zbus_systemd proxies for unit-state
//! events into a dedicated `systemd_units` table.

pub use helios_schema::{EventPayload, EventSource, SystemEvent};

#[cfg(target_os = "linux")]
pub mod procfs_source;

#[cfg(target_os = "linux")]
pub mod socket_server;

#[cfg(target_os = "linux")]
pub mod dbus_source;

#[cfg(target_os = "linux")]
pub mod journal_source;

#[cfg(target_os = "linux")]
pub mod network_source;

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
