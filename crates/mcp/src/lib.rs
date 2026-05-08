//! heliOS MCP gateway — library surface.
//!
//! Implements a small subset of the Model Context Protocol over stdio
//! using JSON-RPC 2.0 newline-delimited framing. Exposes the entity
//! store's queries as MCP tools so Claude Code can read system state
//! through the standard MCP transport.
//!
//! Methods supported:
//!   * `initialize` — handshake; advertises tool capability.
//!   * `tools/list` — returns the registered tools.
//!   * `tools/call` — dispatches one of the tools to the store.
//!   * `notifications/initialized` — no-op acknowledgement.
//!
//! The store is reached over its Unix socket. Each tool call opens a
//! short-lived connection.

pub mod protocol;
pub mod tools;

#[cfg(target_os = "linux")]
pub mod store_client;
