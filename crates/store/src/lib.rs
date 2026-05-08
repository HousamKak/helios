//! heliOS entity store — library surface.
//!
//! Phase 0 stub. The real implementation persists `helios_schema` entities
//! into SQLite (WAL + FTS5), runs the migrations from `helios_schema::migrations`,
//! subscribes to `helios_events::SystemEvent` and projects them into rows.
//!
//! The store exposes:
//!   * a typed Rust API (used by other userland crates)
//!   * a Unix-socket query protocol (used by the compositor)
//!   * MCP tools (used by Claude Code via `helios-mcp`)

pub use helios_schema::migrations::MIGRATIONS;

/// Default on-disk location for the entity store. Lives under the user's
/// runtime dir so a per-session DB is straightforward.
pub const DEFAULT_DB_PATH: &str = "/var/lib/helios/store.sqlite";

/// Default Unix-socket the store listens on for typed queries.
pub const DEFAULT_SOCKET_PATH: &str = "/run/helios/store.sock";

pub fn placeholder() -> &'static str {
    "helios-store: phase-0 stub"
}
