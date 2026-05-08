//! heliOS events bus daemon — Phase 1.
//!
//! Linux: spawns the procfs poller as a tokio task, runs the Unix
//! socket fanout for subscribers (helios-store and any future
//! consumer), and prints emitted events to stdout for live debugging.
//!
//! Non-Linux: prints a stub message and exits non-zero. heliOS targets
//! Linux exclusively past Phase 0; cross-platform is for the schema
//! and applet crates only.
//!
//! Run on a Linux host:
//!
//! ```sh
//! cargo run -p helios-events
//! # In another terminal:
//! sleep 1; ls /tmp; true
//! # The first window prints [exec] and [exit] lines for every PID
//! # involved.
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-events: Linux-only past Phase 0. Build runs on other \
         platforms; the daemon does not."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let (tx, mut rx) =
        broadcast::channel::<helios_schema::SystemEvent>(helios_events::BROADCAST_CAPACITY);

    // Spawn the procfs source as a background task.
    let source_tx = tx.clone();
    let interval = Duration::from_millis(helios_events::PROCFS_POLL_INTERVAL_MS);
    tokio::spawn(async move {
        if let Err(err) = helios_events::procfs_source::run(source_tx, interval).await {
            tracing::error!(?err, "procfs source crashed");
        }
    });

    // Spawn the Unix socket fanout for external subscribers.
    let server_tx = tx.clone();
    let socket_path = helios_events::socket_server::socket_path_from_env();
    tokio::spawn(async move {
        if let Err(err) = helios_events::socket_server::serve(socket_path, server_tx).await {
            tracing::error!(?err, "socket server crashed");
        }
    });

    tracing::info!(
        budget_per_sec = helios_events::TARGET_SUSTAINED_EVENTS_PER_SEC,
        poll_ms = helios_events::PROCFS_POLL_INTERVAL_MS,
        "helios-events: phase-1 procfs + socket fanout running. Ctrl-C to stop."
    );

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutdown");
                return Ok(());
            }
            recv = rx.recv() => {
                match recv {
                    Ok(event) => print_event(&event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(dropped = n, "subscriber lagged behind broadcast capacity");
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn print_event(event: &helios_schema::SystemEvent) {
    use helios_schema::EventPayload;
    match &event.payload {
        EventPayload::ProcessExec {
            pid, comm, cmdline, ..
        } => {
            println!("[exec] pid={pid:<6} comm={comm:<16} cmd={cmdline}");
        }
        EventPayload::ProcessExit { pid, .. } => {
            println!("[exit] pid={pid}");
        }
        // Other variants don't fire from the procfs source yet.
        _ => {}
    }
}
