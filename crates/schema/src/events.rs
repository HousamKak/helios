//! System events — the message types fanned out by the events bus.
//!
//! These are *transient* messages, distinct from the persisted
//! `events` SQL row (which carries `payload_json` for any of these). The
//! enum is the wire format on the in-process broadcast channel and on the
//! Unix-socket fanout to subscribers (length-prefixed JSON encoded — see
//! `helios_events::socket_server`).
//!
//! Per `docs/research/04-observability.md`: event budget v0.1 is 10k/sec
//! sustained, 50k burst, drop-oldest with a `dropped` counter.

use crate::{EntityId, Timestamp};
use serde::{Deserialize, Serialize};

/// Top-level event envelope. Every payload variant carries the minimum
/// fields the bus needs to route, persist, and project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub id: EntityId,
    pub timestamp: Timestamp,
    pub source: EventSource,
    pub correlation_id: Option<EntityId>,
    pub causation_id: Option<EntityId>,
    pub payload: EventPayload,
}

/// Origin of the event. Mirrors `events.source` in the H schema, extended
/// with the new OS-level sources.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    // H-origin sources
    Orchestrator,
    Agent,
    Tool,
    User,
    // OS-level sources (introduced by heliOS)
    Kernel,        // eBPF / netlink
    Procfs,        // /proc enrichment
    Fanotify,      // file events
    Dbus,          // system bus signals
    Journald,      // log records
    Systemd,       // unit state
    Compositor,    // surface lifecycle
    AppletRuntime, // applet events
}

/// The discriminated payload. Each variant maps cleanly to a row in the
/// persisted `events` table (with `type` = the variant name in snake_case
/// and `payload_json` = the variant's body).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    // ---- Process lifecycle (kernel + procfs) ----
    ProcessExec {
        pid: i32,
        ppid: Option<i32>,
        comm: String,
        cmdline: String,
        exe: Option<String>,
        uid: u32,
        gid: u32,
        cgroup: Option<String>,
    },
    ProcessExit {
        pid: i32,
        exit_code: i32,
    },
    ProcessFork {
        parent_pid: i32,
        child_pid: i32,
    },

    // ---- File events (fanotify / eBPF LSM) ----
    FileOpen {
        pid: Option<i32>,
        path: String,
        flags: i32,
    },
    FileWrite {
        pid: Option<i32>,
        path: String,
        bytes: u64,
    },
    FileCreate {
        pid: Option<i32>,
        path: String,
    },
    FileUnlink {
        pid: Option<i32>,
        path: String,
    },

    // ---- Network (sock_diag / eBPF tcp probes) ----
    TcpConnect {
        pid: Option<i32>,
        local_addr: String,
        local_port: u16,
        remote_addr: String,
        remote_port: u16,
    },
    TcpClose {
        connection_id: EntityId,
    },

    // ---- D-Bus / systemd ----
    DbusSignal {
        sender: String,
        path: String,
        interface: String,
        member: String,
        body_json: serde_json::Value,
    },
    SystemdUnitStateChanged {
        unit: String,
        active_state: String,
        sub_state: String,
    },
    JournalRecord {
        unit: Option<String>,
        priority: u8,
        message: String,
    },

    // ---- Compositor ----
    SurfaceMapped {
        surface_id: EntityId,
        client_pid: Option<i32>,
        kind: String, // "xdg_toplevel" | "x11" | "applet"
    },
    SurfaceUnmapped {
        surface_id: EntityId,
    },
    EntityPlaced {
        canvas_entity_id: EntityId,
        x: f64,
        y: f64,
        scale: f64,
    },

    // ---- Applet runtime ----
    AppletInstantiated {
        applet_id: EntityId,
        instance_id: EntityId,
    },
    AppletRendered {
        instance_id: EntityId,
        frame: u64,
    },
    AppletCrashed {
        instance_id: EntityId,
        reason: String,
    },

    // ---- H-origin orchestration events (preserved for parity) ----
    AgentSpawned {
        agent_id: EntityId,
        role: String,
        project_id: EntityId,
    },
    AgentTerminated {
        agent_id: EntityId,
        reason: String,
    },
    TaskCreated {
        task_id: EntityId,
        project_id: EntityId,
    },
    TaskCompleted {
        task_id: EntityId,
        success: bool,
    },
    BlackboardUpdated {
        entry_id: EntityId,
        project_id: EntityId,
    },

    // Escape hatch for less-common or third-party events; should be rare.
    Other {
        kind: String,
        body: serde_json::Value,
    },
}
