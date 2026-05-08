//! heliOS events bus daemon — Phase 0 stub.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!(
        socket = helios_events::DEFAULT_SOCKET_PATH,
        budget_per_sec = helios_events::TARGET_SUSTAINED_EVENTS_PER_SEC,
        "helios-events: phase-0 stub starting (no sources wired yet)"
    );

    // Phase 1: spawn source tasks (eBPF, fanotify, zbus, journal,
    // rtnetlink, sock_diag) and a broadcast fanout. Hold the runtime
    // alive in the meantime.
    tokio::signal::ctrl_c().await?;
    tracing::info!("helios-events: shutdown");
    Ok(())
}
