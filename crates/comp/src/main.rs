//! heliOS Wayland compositor — Phase 0 stub.
//!
//! Per `docs/research/01-compositor.md` and `PLAN.md` §6 (Phase 2):
//!   * Smithay 0.7+ as the foundation
//!   * niri as the 80% blueprint (custom GLSL shader for per-surface
//!     world-to-screen matrix; smooth-zoom-between-snaps trick)
//!   * GLES via `Smithay GlesRenderer` for v1 (defer wgpu)
//!   * XWayland support for legacy app integration
//!   * Subscribes to `helios-events` and `helios-store` for the entity
//!     graph; renders `canvas_entities` rows on each output
//!
//! Linux-only. Uses `cfg!` rather than `target_os` gating at the binary
//! level so the workspace builds cleanly on Windows for schema / applet
//! work.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-comp must be built on Linux. \
         Use the schema and applet crates from a Windows / macOS host; \
         compositor work requires a Linux dev VM or bare metal."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    tracing::info!(
        "helios-comp: phase-0 stub — Smithay backend not yet wired. \
         See PLAN.md §6 for Phase 2 work."
    );

    // Phase 2: anvil-style event loop, world-to-screen render element,
    // pan/zoom gestures, XWayland integration, entity-store subscriber.
    Ok(())
}
