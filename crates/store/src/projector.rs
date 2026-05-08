//! Event → row projector.
//!
//! Every `SystemEvent` is appended to the `events` table (universal log).
//! Process-lifecycle events additionally upsert into `processes` so
//! "what's currently running on this machine" is a single SELECT.
//!
//! Future variants (file events, network connections, applet
//! instantiation) project into their own tables when the corresponding
//! event sources land. For now they just hit the universal log.

use helios_schema::{EventPayload, SystemEvent, ipc::StoredEvent};
use rusqlite::{Connection, params};

/// Apply one event's effects to the database. Caller holds the DB lock.
pub fn project(conn: &Connection, event: &SystemEvent) -> anyhow::Result<()> {
    let stored = StoredEvent::from_system_event(event)?;
    let payload_json = serde_json::to_string(&stored.payload)?;

    conn.execute(
        "INSERT INTO events (id, type, timestamp, project_id, agent_id, task_id,
                              payload_json, source, correlation_id, causation_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            stored.id,
            stored.kind,
            stored.timestamp,
            stored.project_id,
            stored.agent_id,
            stored.task_id,
            payload_json,
            stored.source,
            stored.correlation_id,
            stored.causation_id,
        ],
    )?;

    match &event.payload {
        EventPayload::ProcessExec {
            pid,
            ppid,
            comm,
            cmdline,
            exe,
            uid,
            gid,
            cgroup,
        } => {
            conn.execute(
                "INSERT INTO processes (pid, ppid, cmdline, exe, comm, uid, gid, cgroup,
                                         status, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'running', ?9)
                 ON CONFLICT(pid) DO UPDATE SET
                     ppid = excluded.ppid,
                     cmdline = excluded.cmdline,
                     exe = excluded.exe,
                     comm = excluded.comm,
                     uid = excluded.uid,
                     gid = excluded.gid,
                     cgroup = excluded.cgroup,
                     status = 'running',
                     started_at = excluded.started_at,
                     exited_at = NULL,
                     exit_code = NULL",
                params![
                    pid,
                    ppid,
                    cmdline,
                    exe,
                    comm,
                    uid,
                    gid,
                    cgroup,
                    event.timestamp
                ],
            )?;
        }
        EventPayload::ProcessExit { pid, exit_code } => {
            conn.execute(
                "UPDATE processes SET status = 'dead', exited_at = ?1, exit_code = ?2
                 WHERE pid = ?3",
                params![event.timestamp, exit_code, pid],
            )?;
        }
        // Other payload variants are logged into `events` only;
        // dedicated table projections land alongside their event sources.
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_schema::{EventSource, generate_id, now};

    fn open_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for migration in helios_schema::migrations::MIGRATIONS {
            conn.execute_batch(migration.sql).unwrap();
        }
        conn
    }

    fn exec_event(pid: i32, comm: &str) -> SystemEvent {
        SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Procfs,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::ProcessExec {
                pid,
                ppid: Some(1),
                comm: comm.to_string(),
                cmdline: format!("/bin/{comm}"),
                exe: None,
                uid: 1000,
                gid: 1000,
                cgroup: None,
            },
        }
    }

    fn exit_event(pid: i32) -> SystemEvent {
        SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Procfs,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::ProcessExit { pid, exit_code: 0 },
        }
    }

    #[test]
    fn exec_inserts_process_row() {
        let conn = open_test_db();
        project(&conn, &exec_event(123, "bash")).unwrap();
        let comm: String = conn
            .query_row("SELECT comm FROM processes WHERE pid = 123", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(comm, "bash");
    }

    #[test]
    fn exit_marks_process_dead() {
        let conn = open_test_db();
        project(&conn, &exec_event(456, "vim")).unwrap();
        project(&conn, &exit_event(456)).unwrap();
        let status: String = conn
            .query_row("SELECT status FROM processes WHERE pid = 456", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "dead");
    }

    #[test]
    fn every_event_lands_in_events_table() {
        let conn = open_test_db();
        project(&conn, &exec_event(789, "ls")).unwrap();
        project(&conn, &exit_event(789)).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn pid_reuse_resurrects_row() {
        let conn = open_test_db();
        project(&conn, &exec_event(1234, "first")).unwrap();
        project(&conn, &exit_event(1234)).unwrap();
        project(&conn, &exec_event(1234, "second")).unwrap();
        let (status, comm): (String, String) = conn
            .query_row(
                "SELECT status, comm FROM processes WHERE pid = 1234",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "running");
        assert_eq!(comm, "second");
    }
}
