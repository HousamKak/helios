//! End-to-end integration test for the entity store pipeline.
//!
//! Exercises the full data plane: `db::open` runs migrations,
//! `projector::project` writes events into rows, `server::serve`
//! exposes them on a Unix socket, and a client roundtrips
//! `StoreRequest` → `StoreResponse` over that socket.
//!
//! Linux-only because the server binds a Unix socket. Skipped on
//! other targets so the workspace cross-compiles.

#![cfg(target_os = "linux")]

use std::time::Duration;

use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, ipc, now};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

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

async fn call(
    socket: &std::path::Path,
    request: ipc::StoreRequest,
) -> anyhow::Result<ipc::StoreResponse> {
    let stream = UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    let mut req_line = serde_json::to_string(&request)?;
    req_line.push('\n');
    write.write_all(req_line.as_bytes()).await?;
    write.flush().await?;

    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;
    Ok(serde_json::from_str(response_line.trim())?)
}

async fn wait_for_socket(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("server socket {path:?} never appeared");
}

#[tokio::test]
async fn pipeline_projects_and_serves_queries() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let db_path = tmp.path().join("integration.sqlite");
    let socket_path = tmp.path().join("integration-store.sock");

    let (db, applied) = helios_store::db::open(&db_path)?;
    assert_eq!(
        applied,
        helios_schema::migrations::MIGRATIONS.len(),
        "fresh DB should apply every migration"
    );

    // Project a small batch of events through the projector.
    {
        let conn = db.lock().unwrap();
        for i in 1..=5 {
            helios_store::projector::project(&conn, &exec_event(2000 + i, &format!("proc{i}")))?;
        }
        // Two of them exit.
        helios_store::projector::project(&conn, &exit_event(2002))?;
        helios_store::projector::project(&conn, &exit_event(2004))?;
    }

    // Spin up the query server.
    let server_db = db.clone();
    let server_socket = socket_path.clone();
    let server = tokio::spawn(async move {
        let _ = helios_store::server::serve(server_socket, server_db).await;
    });
    wait_for_socket(&socket_path).await;

    // Ping
    match call(&socket_path, ipc::StoreRequest::Ping).await? {
        ipc::StoreResponse::Pong {
            migrations_applied, ..
        } => {
            assert_eq!(
                migrations_applied,
                helios_schema::migrations::MIGRATIONS.len()
            );
        }
        other => panic!("expected Pong, got {other:?}"),
    }

    // Stats — events_total should be 7 (5 execs + 2 exits)
    match call(&socket_path, ipc::StoreRequest::Stats).await? {
        ipc::StoreResponse::Stats {
            events_total,
            process_total,
            process_running,
            ..
        } => {
            assert_eq!(events_total, 7, "5 exec + 2 exit");
            assert_eq!(process_total, 5);
            assert_eq!(process_running, 3, "5 minus 2 exited");
        }
        other => panic!("expected Stats, got {other:?}"),
    }

    // ListProcesses — should return 3 running
    match call(
        &socket_path,
        ipc::StoreRequest::ListProcesses { limit: Some(10) },
    )
    .await?
    {
        ipc::StoreResponse::Processes { processes } => {
            assert_eq!(processes.len(), 3);
            for p in &processes {
                assert!(matches!(p.status, helios_schema::ProcessStatus::Running));
            }
        }
        other => panic!("expected Processes, got {other:?}"),
    }

    // GetProcess — fetch one we exited
    match call(&socket_path, ipc::StoreRequest::GetProcess { pid: 2002 }).await? {
        ipc::StoreResponse::Process { process } => {
            let p = process.expect("pid 2002 should exist");
            assert_eq!(p.pid, 2002);
            assert!(matches!(p.status, helios_schema::ProcessStatus::Dead));
            assert!(p.exited_at.is_some());
        }
        other => panic!("expected Process, got {other:?}"),
    }

    // ListRecentEvents
    match call(
        &socket_path,
        ipc::StoreRequest::ListRecentEvents {
            limit: Some(20),
            source: Some("procfs".to_string()),
        },
    )
    .await?
    {
        ipc::StoreResponse::Events { events } => {
            assert_eq!(events.len(), 7);
            for e in &events {
                assert_eq!(e.source, "procfs");
            }
        }
        other => panic!("expected Events, got {other:?}"),
    }

    // Bad request shape — should get an Error response, not a panic.
    let bad: ipc::StoreResponse = {
        let stream = UnixStream::connect(&socket_path).await?;
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        write.write_all(b"not valid json\n").await?;
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        serde_json::from_str(line.trim())?
    };
    assert!(
        matches!(bad, ipc::StoreResponse::Error { .. }),
        "malformed request should produce Error response"
    );

    server.abort();
    Ok(())
}
