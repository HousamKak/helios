//! heliOS applet runtime daemon — Phase 0 stub.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!(
        applets_dir = helios_applets::DEFAULT_APPLET_DIR,
        cache_dir = helios_applets::DEFAULT_CACHE_DIR,
        "helios-applets: phase-0 stub starting (no Wasmtime engine yet)"
    );

    tokio::signal::ctrl_c().await?;
    Ok(())
}
