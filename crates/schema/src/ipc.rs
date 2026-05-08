//! Inter-process protocol types shared between the events bus, the
//! entity store, the MCP gateway, and any other userland subscriber.
//!
//! Two flavours coexist:
//!
//! * **`StoreRequest` / `StoreResponse`** — typed query/command shapes
//!   the entity store accepts on its Unix socket. JSON-encoded for
//!   ergonomics (the store is queried by humans during dev, by Claude
//!   Code via MCP in production).
//! * **Events bus subscription** — the events daemon writes
//!   `SystemEvent` values straight onto each subscriber's connection
//!   using `postcard` framing. No request/response — the connection
//!   is push-only.
//!
//! Default socket paths live in this module so every consumer agrees.

use crate::{EntityId, EntityKind, Process, SystemEvent, Timestamp};
use serde::{Deserialize, Serialize};

/// Default Unix-socket the events bus listens on for subscribers.
pub const DEFAULT_EVENTS_SOCKET: &str = "/run/helios/events.sock";

/// Default Unix-socket the entity store listens on for queries.
pub const DEFAULT_STORE_SOCKET: &str = "/run/helios/store.sock";

/// Default on-disk path for the entity store's SQLite database.
pub const DEFAULT_STORE_DB_PATH: &str = "/var/lib/helios/store.sqlite";

// ---------------------------------------------------------------------------
// Store request/response
// ---------------------------------------------------------------------------

/// One request to the entity store. JSON-encoded, line-delimited on the
/// wire. Closed enum — adding a request requires touching every caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StoreRequest {
    /// Health check; replies with `Pong { migrations: N }`.
    Ping,
    /// List currently-running processes (status='running'), most recent first.
    ListProcesses { limit: Option<u32> },
    /// Fetch a single process by PID.
    GetProcess { pid: i32 },
    /// List the most recent events, optionally filtered by source.
    ListRecentEvents {
        limit: Option<u32>,
        source: Option<String>,
    },
    /// List entities of one kind on a given desktop (or all desktops if None).
    ListCanvasEntities {
        kind: Option<EntityKind>,
        desktop_id: Option<EntityId>,
    },
    /// Aggregate counts for the dashboard: total processes, alive
    /// processes, events seen this minute, etc.
    Stats,
}

/// One response from the entity store. Tagged like the request side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreResponse {
    Pong {
        migrations_applied: usize,
        schema_version: String,
    },
    Processes {
        processes: Vec<Process>,
    },
    Process {
        process: Option<Process>,
    },
    Events {
        events: Vec<StoredEvent>,
    },
    CanvasEntities {
        rows: Vec<crate::CanvasEntity>,
    },
    Stats {
        process_total: i64,
        process_running: i64,
        events_last_minute: i64,
        events_total: i64,
        last_event_at: Option<Timestamp>,
    },
    Error {
        message: String,
    },
}

/// One row of the persisted `events` table — distinct from the live
/// `SystemEvent` envelope so callers can see fields the runtime adds
/// (e.g. the `id` and the persisted `payload_json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: EntityId,
    pub kind: String,
    pub timestamp: Timestamp,
    pub source: String,
    pub project_id: Option<EntityId>,
    pub agent_id: Option<EntityId>,
    pub task_id: Option<EntityId>,
    pub correlation_id: Option<EntityId>,
    pub causation_id: Option<EntityId>,
    pub payload: serde_json::Value,
}

impl StoredEvent {
    /// Build a `StoredEvent` from a live `SystemEvent`. Fails only if
    /// the payload can't be JSON-encoded, which shouldn't happen for
    /// any well-formed `EventPayload`.
    pub fn from_system_event(event: &SystemEvent) -> Result<Self, serde_json::Error> {
        let payload_value = serde_json::to_value(&event.payload)?;
        let kind = match &payload_value {
            serde_json::Value::Object(map) => map
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("other")
                .to_string(),
            _ => "other".to_string(),
        };
        Ok(Self {
            id: event.id.clone(),
            kind,
            timestamp: event.timestamp.clone(),
            source: format!("{:?}", event.source).to_lowercase(),
            project_id: None,
            agent_id: None,
            task_id: None,
            correlation_id: event.correlation_id.clone(),
            causation_id: event.causation_id.clone(),
            payload: payload_value,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventPayload, EventSource, generate_id, now};

    #[test]
    fn store_request_roundtrips_as_tagged_json() {
        let req = StoreRequest::ListProcesses { limit: Some(50) };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"op\":\"list_processes\""));
        let parsed: StoreRequest = serde_json::from_str(&s).unwrap();
        assert!(matches!(
            parsed,
            StoreRequest::ListProcesses { limit: Some(50) }
        ));
    }

    #[test]
    fn stored_event_extracts_kind_from_payload() {
        let envelope = SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Procfs,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::ProcessExec {
                pid: 42,
                ppid: Some(1),
                comm: "init".to_string(),
                cmdline: "/sbin/init".to_string(),
                exe: None,
                uid: 0,
                gid: 0,
                cgroup: None,
            },
        };
        let stored = StoredEvent::from_system_event(&envelope).unwrap();
        assert_eq!(stored.kind, "process_exec");
        assert_eq!(stored.source, "procfs");
    }
}
