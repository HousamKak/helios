//! heliOS canonical schema
//!
//! Rust types mirroring the SQL migrations in `migrations/`. These are the
//! shape every other crate in the workspace agrees on for entities and
//! events. The store crate persists them; the events bus fans them; the
//! compositor draws them; the MCP gateway exposes them; the applet runtime
//! constrains them.
//!
//! Organising rule: *the schema is the contract*. Adding a field anywhere
//! in `(struct, SQL migration, MCP tool I/O)` requires touching all three.

use serde::{Deserialize, Serialize};

pub mod ids;
pub mod entities;
pub mod events;
pub mod canvas;
pub mod migrations;

// Re-export the high-traffic types at the crate root.
pub use ids::{EntityId, generate_id};
pub use entities::*;
pub use events::*;
pub use canvas::*;

/// ISO-8601 UTC string. We deliberately do not use `chrono::DateTime<Utc>`
/// at storage boundaries to keep parity with H's TEXT timestamps and
/// preserve sub-second precision exactly as written. Convert at the edges.
pub type Timestamp = String;

/// Helper: current time as ISO-8601 UTC string (for defaults in code paths
/// where SQLite's `datetime('now')` isn't running).
pub fn now() -> Timestamp {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("invalid entity kind: {0}")]
    InvalidEntityKind(String),
    #[error("invalid status value: {0}")]
    InvalidStatus(String),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceSource {
    Telegram,
    Api,
    Cli,
    System,
    Websocket,
    Shell,
}
