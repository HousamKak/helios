//! heliOS user shell — Phase 0 stub.
//!
//! On a real heliOS install, `helios-shell` is what `getty`/PAM hands the
//! user after login. It does not present bash. It launches Claude Code
//! with the events bus, entity store, and MCP gateway wired up, and lets
//! Claude Code drive everything from there.
//!
//! v0.1 deliverable (PLAN.md §6, Phase 1 demo):
//!   1. Boot image, log in.
//!   2. helios-shell starts. Sets env (HELIOS_BUS_SOCKET, HELIOS_MCP_CMD).
//!   3. Spawns `claude` with the heliOS MCP server attached.
//!   4. User talks to Claude Code; entities appear in the store; events
//!      flow on the bus.

use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!(
        "helios-shell: phase-0 stub — would launch Claude Code with bus + MCP wired. \
         See PLAN.md §6 Phase 1."
    );

    // Phase 1: PAM session setup, env wiring, exec claude.
    Ok(())
}
