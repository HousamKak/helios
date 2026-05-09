//! heliOS entity store — library surface.
//!
//! The store is the durable projection of the events bus. It runs the
//! migrations from `helios_schema::migrations`, opens a SQLite database
//! (WAL + foreign keys + FTS5), subscribes to the events bus over a
//! Unix socket, projects each `SystemEvent` into rows, and exposes a
//! typed query API on its own Unix socket.
//!
//! The daemon is the only writer to the SQLite file. Multiple
//! subscribers can connect to the query socket and read concurrently.

pub mod db;
pub mod projector;

#[cfg(target_os = "linux")]
pub mod events_client;

#[cfg(target_os = "linux")]
pub mod publisher;

#[cfg(target_os = "linux")]
pub mod server;

#[cfg(target_os = "linux")]
pub mod spawn;

pub use helios_schema::ipc::{StoreRequest, StoreResponse, StoredEvent};

/// Default on-disk path for the entity store's SQLite database.
pub const DEFAULT_DB_PATH: &str = helios_schema::ipc::DEFAULT_STORE_DB_PATH;

/// Default Unix-socket path the store listens on for typed queries.
pub const DEFAULT_SOCKET_PATH: &str = helios_schema::ipc::DEFAULT_STORE_SOCKET;
