//! heliOS MCP gateway daemon — Phase 0 stub.
//!
//! At v0.1 this exposes (per `PLAN.md` §6, Phase 1):
//!   * entity-store CRUD (every kind in `helios_schema`)
//!   * memory get/set/recall
//!   * tasks: enqueue, status, complete
//!   * blackboard: post, list
//!   * files: read, write, glob, grep
//!   * processes: list, spawn, signal
//!   * applets: list, install, run, stop
//!
//! Speaks MCP over stdio so Claude Code can attach to it as a child
//! transport from the agent shell.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!("helios-mcp: phase-0 stub starting (no tools wired yet)");

    tokio::signal::ctrl_c().await?;
    Ok(())
}
