//! Unix-socket query server for the entity store.
//!
//! Each connection speaks line-delimited JSON: one `StoreRequest` per
//! line, one `StoreResponse` per line. The protocol is push-pull —
//! send a request, read a response. Pipelining is allowed.
//!
//! All DB calls happen inside `tokio::task::spawn_blocking` so a slow
//! query never blocks the runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use helios_events::publisher::EventsPublisher;
use helios_schema::EntityKind;
use helios_schema::ipc::{StoreRequest, StoreResponse, StoredEvent};
use rusqlite::params;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::db::SharedDb;

/// Bind the socket and accept connections forever. m-8.4: takes an
/// optional `EventsPublisher` so `MoveEntity` requests can emit
/// `EntityPlaced` events onto the bus after a successful SQL update.
/// `None` is fine for tests / dev runs without an events daemon —
/// the move still updates the row, just without bus notification.
pub async fn serve(
    socket_path: PathBuf,
    db: SharedDb,
    publisher: Option<Arc<EventsPublisher>>,
) -> anyhow::Result<()> {
    if let Some(parent) = socket_path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!(path = %socket_path.display(), "store socket listening");

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(?err, "accept failed");
                continue;
            }
        };
        let db = db.clone();
        let publisher = publisher.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, db, publisher).await {
                tracing::debug!(?err, "client dropped");
            }
        });
    }
}

async fn handle_client(
    stream: UnixStream,
    db: SharedDb,
    publisher: Option<Arc<EventsPublisher>>,
) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }

        let req_result: Result<StoreRequest, _> = serde_json::from_str(line.trim());
        let resp = match req_result {
            Ok(req) => {
                let db_clone = db.clone();
                let result = tokio::task::spawn_blocking(move || dispatch(&db_clone, req)).await?;
                match result {
                    Ok((response, Some(event))) => {
                        // m-8.4: SQL succeeded; fire the bus
                        // notification AFTER the row is durable. If
                        // the publisher is missing or fails, log and
                        // continue — the row is the source of truth,
                        // the bus is the notification channel.
                        if let Some(p) = publisher.as_ref()
                            && let Err(err) = p.publish(&event).await
                        {
                            tracing::debug!(?err, "store: bus publish failed");
                        }
                        response
                    }
                    Ok((response, None)) => response,
                    Err(err) => StoreResponse::Error {
                        message: err.to_string(),
                    },
                }
            }
            Err(err) => StoreResponse::Error {
                message: format!("invalid request: {err}"),
            },
        };

        let mut out = serde_json::to_string(&resp)?;
        out.push('\n');
        write.write_all(out.as_bytes()).await?;
        write.flush().await?;
    }
}

/// Dispatch one request. Returns `(response, optional_event_to_publish)`.
/// The event is `Some` only for write-style requests that need to be
/// announced on the bus (currently `MoveEntity`). Read-only requests
/// always return `None` for the event slot.
fn dispatch(
    db: &SharedDb,
    req: StoreRequest,
) -> anyhow::Result<(StoreResponse, Option<helios_schema::SystemEvent>)> {
    match req {
        StoreRequest::Ping => Ok((handle_ping(), None)),
        StoreRequest::ListProcesses { limit } => {
            handle_list_processes(db, limit.unwrap_or(100)).map(|r| (r, None))
        }
        StoreRequest::GetProcess { pid } => handle_get_process(db, pid).map(|r| (r, None)),
        StoreRequest::ListRecentEvents { limit, source } => {
            handle_list_recent_events(db, limit.unwrap_or(100), source).map(|r| (r, None))
        }
        StoreRequest::ListCanvasEntities { kind, desktop_id } => {
            handle_list_canvas_entities(db, kind, desktop_id).map(|r| (r, None))
        }
        StoreRequest::Stats => handle_stats(db).map(|r| (r, None)),
        StoreRequest::MoveEntity { id, x, y } => handle_move_entity(db, &id, x, y),
    }
}

/// m-8.4: update the canvas entity row + build the corresponding
/// EntityPlaced event for downstream emission.
fn handle_move_entity(
    db: &SharedDb,
    id: &str,
    x: f64,
    y: f64,
) -> anyhow::Result<(StoreResponse, Option<helios_schema::SystemEvent>)> {
    let timestamp = helios_schema::now();
    let conn = db.lock().unwrap();
    // Update the row. SQLite's UPDATE returns the number of changed
    // rows, which is our "did the id exist?" signal.
    let rows_changed = conn.execute(
        "UPDATE canvas_entities SET x = ?2, y = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, x, y, timestamp],
    )?;
    drop(conn);
    if rows_changed == 0 {
        // Row didn't exist. Tell the caller, don't emit anything.
        return Ok((StoreResponse::Moved { ok: false }, None));
    }
    // Read the scale back so the event carries it (the schema's
    // EntityPlaced has scale alongside x/y). We re-acquire the lock
    // briefly — it's the same SharedDb, so cheap.
    let conn = db.lock().unwrap();
    let scale: f64 = conn
        .query_row(
            "SELECT scale FROM canvas_entities WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(1.0);
    drop(conn);
    let event = helios_schema::SystemEvent {
        id: helios_schema::generate_id(),
        timestamp,
        source: helios_schema::EventSource::Tool,
        correlation_id: None,
        causation_id: None,
        payload: helios_schema::EventPayload::EntityPlaced {
            canvas_entity_id: id.to_string(),
            x,
            y,
            scale,
        },
    };
    Ok((StoreResponse::Moved { ok: true }, Some(event)))
}

fn handle_ping() -> StoreResponse {
    StoreResponse::Pong {
        migrations_applied: helios_schema::migrations::MIGRATIONS.len(),
        schema_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn handle_list_processes(db: &SharedDb, limit: u32) -> anyhow::Result<StoreResponse> {
    let limit = limit.min(1000);
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT pid, ppid, cmdline, exe, comm, uid, gid, cgroup, systemd_unit,
                project_id, agent_id, status, rss_kb, cpu_percent,
                started_at, exited_at, exit_code
         FROM processes
         WHERE status = 'running'
         ORDER BY started_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |row| Ok(row_to_process(row)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(StoreResponse::Processes { processes: rows })
}

fn handle_get_process(db: &SharedDb, pid: i32) -> anyhow::Result<StoreResponse> {
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT pid, ppid, cmdline, exe, comm, uid, gid, cgroup, systemd_unit,
                project_id, agent_id, status, rss_kb, cpu_percent,
                started_at, exited_at, exit_code
         FROM processes
         WHERE pid = ?1",
    )?;
    let process = stmt.query_row([pid], |row| Ok(row_to_process(row))).ok();
    Ok(StoreResponse::Process {
        process: process.map(Box::new),
    })
}

fn handle_list_recent_events(
    db: &SharedDb,
    limit: u32,
    source: Option<String>,
) -> anyhow::Result<StoreResponse> {
    let limit = limit.min(1000);
    let conn = db.lock().unwrap();

    let (sql, has_source_filter) = if source.is_some() {
        (
            "SELECT id, type, timestamp, project_id, agent_id, task_id,
                    payload_json, source, correlation_id, causation_id
             FROM events
             WHERE source = ?2
             ORDER BY timestamp DESC
             LIMIT ?1",
            true,
        )
    } else {
        (
            "SELECT id, type, timestamp, project_id, agent_id, task_id,
                    payload_json, source, correlation_id, causation_id
             FROM events
             ORDER BY timestamp DESC
             LIMIT ?1",
            false,
        )
    };

    let mut stmt = conn.prepare(sql)?;
    let events: Vec<StoredEvent> = if has_source_filter {
        stmt.query_map(params![limit, source.unwrap()], row_to_stored_event)?
            .collect::<rusqlite::Result<_>>()?
    } else {
        stmt.query_map(params![limit], row_to_stored_event)?
            .collect::<rusqlite::Result<_>>()?
    };

    Ok(StoreResponse::Events { events })
}

fn handle_list_canvas_entities(
    db: &SharedDb,
    kind: Option<EntityKind>,
    desktop_id: Option<String>,
) -> anyhow::Result<StoreResponse> {
    let conn = db.lock().unwrap();

    let kind_str = kind.map(|k| k.as_str().to_string());

    let sql = match (&kind_str, &desktop_id) {
        (Some(_), Some(_)) => {
            "SELECT id, desktop_id, entity_kind, entity_id, x, y, scale, rotation, z,
                    width, height, pinned, visible, relevance,
                    attached_applet_ids_json, created_at, updated_at
             FROM canvas_entities
             WHERE entity_kind = ?1 AND desktop_id = ?2"
        }
        (Some(_), None) => {
            "SELECT id, desktop_id, entity_kind, entity_id, x, y, scale, rotation, z,
                    width, height, pinned, visible, relevance,
                    attached_applet_ids_json, created_at, updated_at
             FROM canvas_entities
             WHERE entity_kind = ?1"
        }
        (None, Some(_)) => {
            "SELECT id, desktop_id, entity_kind, entity_id, x, y, scale, rotation, z,
                    width, height, pinned, visible, relevance,
                    attached_applet_ids_json, created_at, updated_at
             FROM canvas_entities
             WHERE desktop_id = ?1"
        }
        (None, None) => {
            "SELECT id, desktop_id, entity_kind, entity_id, x, y, scale, rotation, z,
                    width, height, pinned, visible, relevance,
                    attached_applet_ids_json, created_at, updated_at
             FROM canvas_entities"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<helios_schema::CanvasEntity> = match (&kind_str, &desktop_id) {
        (Some(k), Some(d)) => stmt
            .query_map(params![k, d], row_to_canvas_entity)?
            .collect::<rusqlite::Result<_>>()?,
        (Some(k), None) => stmt
            .query_map(params![k], row_to_canvas_entity)?
            .collect::<rusqlite::Result<_>>()?,
        (None, Some(d)) => stmt
            .query_map(params![d], row_to_canvas_entity)?
            .collect::<rusqlite::Result<_>>()?,
        (None, None) => stmt
            .query_map([], row_to_canvas_entity)?
            .collect::<rusqlite::Result<_>>()?,
    };
    Ok(StoreResponse::CanvasEntities { rows })
}

fn handle_stats(db: &SharedDb) -> anyhow::Result<StoreResponse> {
    let conn = db.lock().unwrap();
    let process_total: i64 = conn.query_row("SELECT COUNT(*) FROM processes", [], |r| r.get(0))?;
    let process_running: i64 = conn.query_row(
        "SELECT COUNT(*) FROM processes WHERE status = 'running'",
        [],
        |r| r.get(0),
    )?;
    let events_total: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
    let events_last_minute: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events
         WHERE timestamp >= datetime('now', '-1 minute')",
        [],
        |r| r.get(0),
    )?;
    let last_event_at: Option<String> = conn
        .query_row(
            "SELECT timestamp FROM events ORDER BY timestamp DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();

    Ok(StoreResponse::Stats {
        process_total,
        process_running,
        events_last_minute,
        events_total,
        last_event_at,
    })
}

// ---------------------------------------------------------------------------
// Row mappers
// ---------------------------------------------------------------------------

fn row_to_process(row: &rusqlite::Row<'_>) -> helios_schema::Process {
    helios_schema::Process {
        pid: row.get_unwrap(0),
        ppid: row.get_unwrap(1),
        cmdline: row.get_unwrap(2),
        exe: row.get_unwrap(3),
        comm: row.get_unwrap(4),
        uid: row.get_unwrap(5),
        gid: row.get_unwrap(6),
        cgroup: row.get_unwrap(7),
        systemd_unit: row.get_unwrap(8),
        project_id: row.get_unwrap(9),
        agent_id: row.get_unwrap(10),
        status: parse_process_status(row.get::<_, String>(11).unwrap_or_default().as_str()),
        rss_kb: row.get_unwrap(12),
        cpu_percent: row.get_unwrap(13),
        started_at: row.get_unwrap(14),
        exited_at: row.get_unwrap(15),
        exit_code: row.get_unwrap(16),
    }
}

fn row_to_stored_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    let payload_str: String = row.get(6)?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
    Ok(StoredEvent {
        id: row.get(0)?,
        kind: row.get(1)?,
        timestamp: row.get(2)?,
        project_id: row.get(3)?,
        agent_id: row.get(4)?,
        task_id: row.get(5)?,
        payload,
        source: row.get(7)?,
        correlation_id: row.get(8)?,
        causation_id: row.get(9)?,
    })
}

fn row_to_canvas_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<helios_schema::CanvasEntity> {
    let attached_json: String = row.get(14)?;
    let attached_applet_ids: Vec<String> = serde_json::from_str(&attached_json).unwrap_or_default();
    let kind_str: String = row.get(2)?;
    Ok(helios_schema::CanvasEntity {
        id: row.get(0)?,
        desktop_id: row.get(1)?,
        entity_kind: parse_entity_kind(&kind_str),
        entity_id: row.get(3)?,
        x: row.get(4)?,
        y: row.get(5)?,
        scale: row.get(6)?,
        rotation: row.get(7)?,
        z: row.get(8)?,
        width: row.get(9)?,
        height: row.get(10)?,
        pinned: row.get::<_, i64>(11)? != 0,
        visible: row.get::<_, i64>(12)? != 0,
        relevance: row.get(13)?,
        attached_applet_ids,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn parse_process_status(s: &str) -> helios_schema::ProcessStatus {
    match s {
        "running" => helios_schema::ProcessStatus::Running,
        "sleeping" => helios_schema::ProcessStatus::Sleeping,
        "zombie" => helios_schema::ProcessStatus::Zombie,
        "stopped" => helios_schema::ProcessStatus::Stopped,
        _ => helios_schema::ProcessStatus::Dead,
    }
}

fn parse_entity_kind(s: &str) -> EntityKind {
    match s {
        "process" => EntityKind::Process,
        "file" => EntityKind::File,
        "applet" => EntityKind::Applet,
        "agent" => EntityKind::Agent,
        "terminal" => EntityKind::Terminal,
        "task" => EntityKind::Task,
        "project" => EntityKind::Project,
        "connection" => EntityKind::Connection,
        _ => EntityKind::Desktop,
    }
}

/// Resolve the store socket path: prefer `HELIOS_STORE_SOCKET`, else default.
pub fn socket_path_from_env() -> PathBuf {
    std::env::var_os("HELIOS_STORE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(helios_schema::ipc::DEFAULT_STORE_SOCKET).to_path_buf())
}
