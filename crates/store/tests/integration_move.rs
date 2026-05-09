//! End-to-end test for the m-8 agent → canvas loop.
//!
//! Exercises:
//!   1. `helios-events` ingress socket (m-8.1) accepts SystemEvents.
//!   2. `EventsPublisher` (m-8.2) frames + writes them.
//!   3. `helios-store` accepts a `MoveEntity` request (m-8.4),
//!      updates the canvas_entities row, and emits an `EntityPlaced`
//!      event onto the bus.
//!   4. A subscriber on the events bus's broadcast channel observes
//!      the relayed event with the correct id + coordinates.
//!
//! The compositor side (m-5.8 events_client + m-8.3 surface emitter)
//! is exercised manually on a real desktop — it would require the
//! full smithay stack which doesn't run in CI without a display.
//!
//! Linux-only.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::time::Duration;

use helios_schema::{EventPayload, EventSource, SystemEvent, ipc};
use rusqlite::params;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::broadcast;
use tokio::time::timeout;

async fn wait_for_socket(path: &std::path::Path) {
    for _ in 0..50 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("socket {} did not appear in time", path.display());
}

#[tokio::test]
async fn move_entity_relays_through_bus() -> anyhow::Result<()> {
    // -- Set up sockets + temp DB -----------------------------------
    let dir = TempDir::new()?;
    let store_socket = dir.path().join("store.sock");
    let ingress_socket = dir.path().join("events-in.sock");
    let db_path = dir.path().join("store.sqlite");

    // -- events bus ingress + broadcast channel ---------------------
    // Mimics what helios-events::main would do: an ingress listener
    // feeding a broadcast channel. We subscribe on the channel
    // directly (no need for the subscriber-side fanout in a test).
    let (bus_tx, mut bus_rx) = broadcast::channel::<SystemEvent>(64);
    let ingress_path = ingress_socket.clone();
    let ingress_tx = bus_tx.clone();
    tokio::spawn(async move {
        let _ = helios_events::socket_ingress::serve_ingress(ingress_path, ingress_tx).await;
    });
    wait_for_socket(&ingress_socket).await;

    // -- store server with a publisher ------------------------------
    let (db, _applied) = helios_store::db::open(&db_path)?;

    // Insert a desktop + a canvas_entity so the MoveEntity request
    // has something to update. Values are minimal — the test only
    // cares about (id, x, y, scale) of the resulting EntityPlaced.
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO desktops (id, name) VALUES (?1, ?2)",
            params!["desktop-test", "test"],
        )?;
        conn.execute(
            "INSERT INTO canvas_entities (id, desktop_id, entity_kind, entity_id, x, y, scale)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "entity-123",
                "desktop-test",
                "process",
                "1234",
                0.0_f64,
                0.0_f64,
                1.5_f64
            ],
        )?;
    }

    let publisher =
        Arc::new(helios_events::publisher::EventsPublisher::connect(ingress_socket.clone()).await);
    let server_socket = store_socket.clone();
    let server_db = db.clone();
    let server_pub = Some(publisher.clone());
    tokio::spawn(async move {
        let _ = helios_store::server::serve(server_socket, server_db, server_pub).await;
    });
    wait_for_socket(&store_socket).await;

    // -- Issue the MoveEntity request -------------------------------
    let req = ipc::StoreRequest::MoveEntity {
        id: "entity-123".to_string(),
        x: 300.0,
        y: 400.0,
    };
    let mut stream = UnixStream::connect(&store_socket).await?;
    let mut payload = serde_json::to_string(&req)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes()).await?;
    stream.flush().await?;
    let (read, _write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line)).await??;
    let resp: ipc::StoreResponse = serde_json::from_str(line.trim())?;
    assert!(matches!(resp, ipc::StoreResponse::Moved { ok: true }));

    // -- Verify the row was actually updated ------------------------
    {
        let conn = db.lock().unwrap();
        let (x, y): (f64, f64) = conn.query_row(
            "SELECT x, y FROM canvas_entities WHERE id = ?1",
            params!["entity-123"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(x, 300.0);
        assert_eq!(y, 400.0);
    }

    // -- Verify the bus relay happened ------------------------------
    let event = timeout(Duration::from_secs(2), bus_rx.recv()).await??;
    match event.payload {
        EventPayload::EntityPlaced {
            canvas_entity_id,
            x,
            y,
            scale,
        } => {
            assert_eq!(canvas_entity_id, "entity-123");
            assert_eq!(x, 300.0);
            assert_eq!(y, 400.0);
            // Scale was preserved from the row's original value.
            assert_eq!(scale, 1.5);
        }
        other => panic!("expected EntityPlaced, got {other:?}"),
    }
    assert_eq!(event.source, EventSource::Tool);
    Ok(())
}

#[tokio::test]
async fn move_entity_unknown_id_returns_ok_false() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let store_socket = dir.path().join("store.sock");
    let ingress_socket = dir.path().join("events-in.sock");
    let db_path = dir.path().join("store.sqlite");

    let (bus_tx, mut bus_rx) = broadcast::channel::<SystemEvent>(64);
    let ingress_path = ingress_socket.clone();
    let ingress_tx = bus_tx.clone();
    tokio::spawn(async move {
        let _ = helios_events::socket_ingress::serve_ingress(ingress_path, ingress_tx).await;
    });
    wait_for_socket(&ingress_socket).await;

    let (db, _) = helios_store::db::open(&db_path)?;
    let publisher =
        Arc::new(helios_events::publisher::EventsPublisher::connect(ingress_socket.clone()).await);
    let server_socket = store_socket.clone();
    let server_db = db.clone();
    let server_pub = Some(publisher.clone());
    tokio::spawn(async move {
        let _ = helios_store::server::serve(server_socket, server_db, server_pub).await;
    });
    wait_for_socket(&store_socket).await;

    let req = ipc::StoreRequest::MoveEntity {
        id: "does-not-exist".to_string(),
        x: 1.0,
        y: 2.0,
    };
    let mut stream = UnixStream::connect(&store_socket).await?;
    let mut payload = serde_json::to_string(&req)?;
    payload.push('\n');
    stream.write_all(payload.as_bytes()).await?;
    stream.flush().await?;
    let (read, _write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line)).await??;
    let resp: ipc::StoreResponse = serde_json::from_str(line.trim())?;
    assert!(matches!(resp, ipc::StoreResponse::Moved { ok: false }));

    // No EntityPlaced event should fire when the row didn't exist —
    // a miss isn't a state change.
    let result = timeout(Duration::from_millis(200), bus_rx.recv()).await;
    assert!(
        result.is_err(),
        "no event should have been emitted, got {result:?}",
    );
    Ok(())
}
