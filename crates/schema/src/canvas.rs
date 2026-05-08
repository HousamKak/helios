//! Canvas-layer entities — the rows the compositor draws.
//!
//! Per `migrations/0002_os_entities.sql`:
//!   * `processes` / `files` / `network_connections` are the OS-observed
//!     things the events bus populates.
//!   * `applets` / `applet_instances` are the WASM applets installed and
//!     running.
//!   * `desktops` are workspaces; `canvas_entities` is the placement table
//!     that ties any entity (process, file, applet, agent, terminal,
//!     project, task, connection, even another desktop) to a position.

use crate::{EntityId, Timestamp};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Processes — every PID observed
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Process {
    pub pid: i32,
    pub ppid: Option<i32>,
    pub cmdline: String,
    pub exe: Option<String>,
    pub comm: String,
    pub uid: u32,
    pub gid: u32,
    pub cgroup: Option<String>,
    pub systemd_unit: Option<String>,
    pub project_id: Option<EntityId>,
    pub agent_id: Option<EntityId>,
    pub status: ProcessStatus,
    pub rss_kb: Option<i64>,
    pub cpu_percent: Option<f64>,
    pub started_at: Timestamp,
    pub exited_at: Option<Timestamp>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Zombie,
    Stopped,
    Dead,
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct File {
    pub id: EntityId,
    pub path: String,
    pub kind: FileKind,
    pub size_bytes: Option<i64>,
    pub mtime: Option<Timestamp>,
    pub project_id: Option<EntityId>,
    pub tags: Vec<String>,
    pub embedding_id: Option<EntityId>,
    pub last_accessed_at: Timestamp,
    pub access_count: i64,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Source,
    Config,
    Binary,
    Document,
    Image,
    Video,
    Archive,
    Log,
    Data,
    Unknown,
}

// ---------------------------------------------------------------------------
// Network connections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkConnection {
    pub id: EntityId,
    pub pid: Option<i32>,
    pub protocol: NetProtocol,
    pub direction: NetDirection,
    pub local_addr: String,
    pub local_port: Option<u16>,
    pub remote_addr: Option<String>,
    pub remote_port: Option<u16>,
    pub state: NetState,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub established_at: Timestamp,
    pub closed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetProtocol {
    Tcp,
    Udp,
    Unix,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetDirection {
    Inbound,
    Outbound,
    Listen,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetState {
    Established,
    Listen,
    TimeWait,
    CloseWait,
    Closed,
    SynSent,
    SynRecv,
}

// ---------------------------------------------------------------------------
// Applets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Applet {
    pub id: EntityId,
    pub name: String,
    pub description: Option<String>,
    pub source: AppletSource,
    pub manifest: serde_json::Value,
    pub wasm_path: String,
    pub wit_world: String,
    pub capabilities: Vec<AppletCapability>,
    pub signature: Option<String>,
    pub status: AppletStatus,
    pub installed_at: Timestamp,
    pub last_run_at: Option<Timestamp>,
    pub author_agent_id: Option<EntityId>,
    pub project_id: Option<EntityId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppletSource {
    Installed,
    AgentEmittedEphemeral,
    AgentEmittedPersistent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppletStatus {
    Idle,
    Running,
    Suspended,
    Crashed,
    Revoked,
}

/// A capability declared in the applet manifest. Each is a WIT-import the
/// host has to wire up; the host is free to deny or attenuate any of them
/// at runtime per policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppletCapability {
    /// Read entities of one or more kinds (no mutation).
    ReadEntity { kinds: Vec<String> },
    /// Subscribe to a topic on the events bus.
    SubscribeEvents { topics: Vec<String> },
    /// Call a named MCP tool.
    CallTool { name: String },
    /// Render via the host UI canvas widget world (`host:ui/canvas`).
    RenderUi,
    /// Bind to one specific entity for its lifetime (e.g. terminal applet
    /// bound to a PTY, file-inspector bound to one file).
    BindEntity { kind: String, id: EntityId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppletInstance {
    pub id: EntityId,
    pub applet_id: EntityId,
    pub state: serde_json::Value,
    pub bound_entity_kind: Option<String>,
    pub bound_entity_id: Option<EntityId>,
    pub started_at: Timestamp,
    pub stopped_at: Option<Timestamp>,
}

// ---------------------------------------------------------------------------
// Desktops + canvas placement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Desktop {
    pub id: EntityId,
    pub name: String,
    pub project_id: Option<EntityId>,
    pub origin_x: f64,
    pub origin_y: f64,
    pub default_zoom: f64,
    pub background: serde_json::Value,
    pub is_active: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// The unifying placement row. The compositor's primary query is
/// `SELECT * FROM canvas_entities WHERE desktop_id = ? AND visible = 1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanvasEntity {
    pub id: EntityId,
    pub desktop_id: EntityId,
    pub entity_kind: EntityKind,
    pub entity_id: EntityId,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z: i32,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub pinned: bool,
    pub visible: bool,
    pub relevance: f64,
    pub attached_applet_ids: Vec<EntityId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Discriminator for `canvas_entities.entity_kind`. Mirrors the SQL CHECK
/// constraint and is the closed set of types the compositor knows how to
/// render.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Process,
    File,
    Applet,
    Agent,
    Terminal,
    Task,
    Project,
    Connection,
    Desktop,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Process => "process",
            EntityKind::File => "file",
            EntityKind::Applet => "applet",
            EntityKind::Agent => "agent",
            EntityKind::Terminal => "terminal",
            EntityKind::Task => "task",
            EntityKind::Project => "project",
            EntityKind::Connection => "connection",
            EntityKind::Desktop => "desktop",
        }
    }
}
