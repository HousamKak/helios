//! H-origin entities — direct ports of the rows defined in
//! `migrations/0001_h_origin.sql`.
//!
//! Field naming uses Rust's snake_case; serde keeps it aligned to the SQL
//! column names without `rename` annotations. JSON-blob columns are
//! deserialized as `serde_json::Value` to keep the schema crate
//! dependency-free of higher-level types — the consumer crates can refine.

use crate::{EntityId, Timestamp};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: EntityId,
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub config: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Paused,
    Archived,
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    pub role: String,
    pub name: String,
    pub description: Option<String>,
    pub system_prompt: String,
    pub capabilities: Vec<String>,
    pub llm_provider: String,
    pub model: Option<String>,
    pub max_concurrent_tasks: i64,
    pub temperature: f64,
    pub token_budget: i64,
    pub max_turns: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentInstance {
    pub id: EntityId,
    pub definition_role: String,
    pub project_id: EntityId,
    pub status: AgentStatus,
    pub current_task_id: Option<EntityId>,
    pub pid: Option<i32>,
    pub token_budget: i64,
    pub turn_count: i64,
    pub spawned_at: Timestamp,
    pub last_active_at: Timestamp,
    pub terminated_at: Option<Timestamp>,
    pub error_message: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Spawning,
    Idle,
    Working,
    Paused,
    Terminated,
    Error,
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: EntityId,
    pub project_id: EntityId,
    pub parent_task_id: Option<EntityId>,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub required_role: String,
    pub assigned_agent_id: Option<EntityId>,
    pub dependencies: Vec<EntityId>,
    pub subtasks: Vec<EntityId>,
    pub result: Option<serde_json::Value>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Assigned,
    InProgress,
    Review,
    Completed,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryRecord {
    pub id: EntityId,
    pub project_id: Option<EntityId>,
    pub agent_id: Option<EntityId>,
    pub kind: MemoryKind,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub access_count: i64,
    pub last_accessed_at: Timestamp,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Fact,
    Decision,
    Pattern,
    Preference,
    Context,
    ErrorLesson,
}

// ---------------------------------------------------------------------------
// Conversations / Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conversation {
    pub id: EntityId,
    pub project_id: Option<EntityId>,
    pub agent_id: Option<EntityId>,
    pub task_id: Option<EntityId>,
    pub interface_source: crate::InterfaceSource,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: EntityId,
    pub conversation_id: EntityId,
    pub role: MessageRole,
    pub agent_id: Option<EntityId>,
    pub content: String,
    pub tool_calls: Option<serde_json::Value>,
    pub tool_results: Option<serde_json::Value>,
    pub token_count: Option<i64>,
    pub timestamp: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Agent,
    System,
    Tool,
}

// ---------------------------------------------------------------------------
// Blackboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlackboardEntry {
    pub id: EntityId,
    pub project_id: EntityId,
    pub agent_id: EntityId,
    pub task_id: Option<EntityId>,
    pub kind: BlackboardKind,
    pub content: String,
    pub confidence: f64,
    pub resolved: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlackboardKind {
    Hypothesis,
    Decision,
    Blocker,
    Discovery,
    CodeContext,
    TestResult,
    ReviewComment,
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: EntityId,
    pub name: Option<String>,
    pub status: SessionStatus,
    pub started_at: Timestamp,
    pub paused_at: Option<Timestamp>,
    pub resumed_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub focus_description: Option<String>,
    pub config: serde_json::Value,
    pub snapshot: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Paused,
    Completed,
    Abandoned,
}

// ---------------------------------------------------------------------------
// Terminals (managed processes — distinct from canvas::Process which is
// every PID)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Terminal {
    pub id: EntityId,
    pub session_id: EntityId,
    pub project_id: EntityId,
    pub agent_id: Option<EntityId>,
    pub name: String,
    pub kind: TerminalKind,
    pub status: TerminalStatus,
    pub pid: Option<i32>,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: serde_json::Value,
    pub exit_code: Option<i32>,
    pub started_at: Timestamp,
    pub stopped_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    Shell,
    ClaudeCodeAutomated,
    ClaudeCodeInteractive,
    DevServer,
    Watcher,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Starting,
    Running,
    Stopped,
    Crashed,
    Completed,
}
