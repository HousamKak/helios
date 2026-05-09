//! heliOS entity store daemon — Phase 1.
//!
//! Opens (or creates) the SQLite database, runs pending migrations,
//! subscribes to the events bus over a Unix socket, projects events
//! into rows, and exposes typed queries on its own Unix socket.
//!
//! Run on a Linux host:
//!
//! ```sh
//! HELIOS_STORE_DB=/tmp/helios-store.sqlite \
//! HELIOS_STORE_SOCKET=/tmp/helios-store.sock \
//! HELIOS_EVENTS_SOCKET=/tmp/helios-events.sock \
//!     cargo run -p helios-store
//! ```
//!
//! ```sh
//! # In another terminal: query it.
//! echo '{"op":"ping"}' | socat - UNIX-CONNECT:/tmp/helios-store.sock
//! echo '{"op":"list_processes","limit":10}' \
//!     | socat - UNIX-CONNECT:/tmp/helios-store.sock
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-store: Linux-only past Phase 0. Build runs on other \
         platforms; the daemon does not."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::path::{Path, PathBuf};
    use tokio::sync::mpsc;
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let db_path: PathBuf = std::env::var_os("HELIOS_STORE_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(helios_store::DEFAULT_DB_PATH).to_path_buf());

    let (db, applied) = helios_store::db::open(&db_path)?;
    tracing::info!(
        db = %db_path.display(),
        migrations_applied = applied,
        "store database open"
    );

    // Spawn the events client. It feeds a bounded mpsc that the
    // projector drains. mpsc back-pressures the client if the
    // projector falls behind, which is the correct behaviour.
    let (event_tx, mut event_rx) = mpsc::channel::<helios_schema::SystemEvent>(8_192);
    let events_socket = helios_store::events_client::socket_path_from_env();
    tokio::spawn(async move {
        if let Err(err) = helios_store::events_client::run(events_socket, event_tx).await {
            tracing::error!(?err, "events client crashed");
        }
    });

    // Projector task: pulls events off the channel, applies them.
    let projector_db = db.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let db = projector_db.clone();
            let result = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap();
                helios_store::projector::project(&conn, &event)
            })
            .await;
            if let Err(err) = result {
                tracing::warn!(?err, "projector task panicked");
            } else if let Ok(Err(err)) = result {
                tracing::warn!(?err, "projection failed");
            }
        }
        tracing::info!("event channel closed; projector exiting");
    });

    // m-8.4: events publisher for `MoveEntity` → `EntityPlaced`
    // emission. Lazy connect — if the events daemon isn't up yet
    // the publisher reconnects on the first publish.
    let publisher = helios_store::publisher::connect().await;

    // Query server.
    let server_socket = helios_store::server::socket_path_from_env();
    let server_db = db.clone();
    let server_publisher = Some(publisher.clone());
    tokio::spawn(async move {
        if let Err(err) =
            helios_store::server::serve(server_socket, server_db, server_publisher).await
        {
            tracing::error!(?err, "store server crashed");
        }
    });

    tracing::info!("helios-store running. Ctrl-C to stop.");
    tokio::signal::ctrl_c().await?;
    tracing::info!("shutdown");
    Ok(())
}
