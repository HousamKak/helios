//! heliOS events bus daemon — Phase 1.
//!
//! Linux: spawns five observability sources as tokio tasks, exposes
//! a Unix socket fanout for subscribers (helios-store, applets,
//! compositor), and prints emitted events to stdout for live debugging.
//!
//! Sources, each independently restartable:
//!   * procfs_source       — process exec/exit
//!   * dbus_source         — generic D-Bus system-bus signals
//!   * journal_source      — systemd journal tail (via journalctl)
//!   * network_source      — TCP connect/close from /proc/net/tcp{,6}
//! plus the socket_server  — fanout over /run/helios/events.sock
//!
//! Non-Linux: prints a stub message and exits non-zero.

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

    // procfs source — process lifecycle.
    let source_tx = tx.clone();
    let interval = Duration::from_millis(helios_events::PROCFS_POLL_INTERVAL_MS);
    tokio::spawn(async move {
        if let Err(err) = helios_events::procfs_source::run(source_tx, interval).await {
            tracing::error!(?err, "procfs source crashed");
        }
    });

    // D-Bus source — generic system-bus signal listener.
    let dbus_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(err) = helios_events::dbus_source::run(dbus_tx).await {
            tracing::error!(?err, "dbus source crashed");
        }
    });

    // Journal source — systemd journal tail.
    let journal_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(err) = helios_events::journal_source::run(journal_tx).await {
            tracing::error!(?err, "journal source crashed");
        }
    });

    // Network source — /proc/net/tcp polling.
    let network_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(err) = helios_events::network_source::run(network_tx).await {
            tracing::error!(?err, "network source crashed");
        }
    });

    // Unix socket fanout for external subscribers.
    let server_tx = tx.clone();
    let socket_path = helios_events::socket_server::socket_path_from_env();
    tokio::spawn(async move {
        if let Err(err) = helios_events::socket_server::serve(socket_path, server_tx).await {
            tracing::error!(?err, "socket server crashed");
        }
    });

    tracing::info!(
        budget_per_sec = helios_events::TARGET_SUSTAINED_EVENTS_PER_SEC,
        sources = "procfs,dbus,journal,network",
        "helios-events: phase-1 fanout running. Ctrl-C to stop."
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
        EventPayload::TcpConnect {
            local_addr,
            local_port,
            remote_addr,
            remote_port,
            ..
        } => {
            println!("[net+] {local_addr}:{local_port} -> {remote_addr}:{remote_port}");
        }
        EventPayload::TcpClose { connection_id } => {
            println!("[net-] {connection_id}");
        }
        EventPayload::DbusSignal {
            interface, member, ..
        } => {
            println!("[dbus] {interface}.{member}");
        }
        EventPayload::JournalRecord {
            unit,
            priority,
            message,
        } => {
            let unit_str = unit.as_deref().unwrap_or("-");
            let snippet: String = message.chars().take(100).collect();
            println!("[log p={priority}] {unit_str}: {snippet}");
        }
        _ => {}
    }
}
