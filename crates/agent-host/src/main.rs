//! heliOS agent-host daemon — Phase 0 stub.
//!
//! The valuable parts of H's `agents` package that survive in heliOS:
//!   * Skills registry (markdown-with-frontmatter, three sources)
//!   * Hooks (settings.json events: SessionStart, PreCompact, PostCompact)
//!   * Plugins (scope-aware install/enable)
//!   * Compaction (forked-agent summarizer, "next steps + verbatim quotes")
//!   * Memory-extractor (end-of-turn forked agent populating MemoryRecord)
//!   * AutoDream (24h + 5-session gated cross-session consolidation)
//!
//! All re-projected as MCP-exposed services Claude Code can consume.
//!
//! See `docs/research/05-h-reuse-audit.md` for the H package mapping.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!("helios-agent-host: phase-0 stub starting (skills/hooks/plugins not wired yet)");

    tokio::signal::ctrl_c().await?;
    Ok(())
}
