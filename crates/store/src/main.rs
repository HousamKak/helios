//! heliOS entity store daemon — Phase 0 stub.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!(
        db = helios_store::DEFAULT_DB_PATH,
        socket = helios_store::DEFAULT_SOCKET_PATH,
        migrations = helios_store::MIGRATIONS.len(),
        "helios-store: phase-0 stub starting (no DB wired yet)"
    );

    tokio::signal::ctrl_c().await?;
    Ok(())
}
