//! heliOS events bus — library surface.
//!
//! Phase 0 stub. The real implementation per `docs/research/04-observability.md`
//! lands in Phase 1: aya for eBPF, zbus for D-Bus, libsystemd journal,
//! rtnetlink + sock_diag, fanotify or eBPF-LSM for files, all fanned into
//! a `tokio::sync::broadcast` with bounded mpsc fronts per producer and
//! `postcard`-encoded delivery over a Unix seqpacket socket.
//!
//! Subscribers (compositor, store, MCP gateway, applets) consume
//! `helios_schema::SystemEvent` values.

pub use helios_schema::{EventPayload, EventSource, SystemEvent};

/// v0.1 event-rate budget. See `PLAN.md` §4 and observability research.
pub const TARGET_SUSTAINED_EVENTS_PER_SEC: usize = 10_000;
pub const TARGET_BURST_EVENTS_PER_SEC: usize = 50_000;
pub const BROADCAST_CAPACITY: usize = 16_384;
pub const MPSC_FRONT_CAPACITY: usize = 4_096;

/// Default Unix-socket path the bus listens on. Subscribers connect here.
pub const DEFAULT_SOCKET_PATH: &str = "/run/helios/events.sock";

/// Stub: builds and exposes a placeholder bus. Real implementation in
/// Phase 1 wires the source tasks listed in the lib doc-comment.
pub fn placeholder() -> &'static str {
    "helios-events: phase-0 stub"
}
