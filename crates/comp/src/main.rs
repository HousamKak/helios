//! heliOS Wayland compositor — Phase 2 scaffold.
//!
//! This binary is the canvas surface of the OS. Per
//! `docs/research/01-compositor.md` and `PLAN.md` §6 (Phase 2):
//!   * Smithay 0.7+ as the foundation
//!   * niri as the 80% blueprint (custom GLSL shader for per-surface
//!     world-to-screen matrix; smooth-zoom-between-snaps trick)
//!   * GLES via `Smithay::GlesRenderer` for v1 (defer wgpu)
//!   * XWayland support for legacy app integration
//!   * Subscribes to `helios-events` and `helios-store` for the entity
//!     graph; renders `canvas_entities` rows on each output
//!
//! Phase 2 month-1 deliverable: this binary boots, builds an empty
//! `HeliosState`, builds a `RenderPlan` from it, logs both, exits.
//! Smithay event loop arrives in month 2 once the host environment
//! has libdrm-dev / libinput-dev / libxkbcommon-dev / libgbm-dev.
//!
//! Linux-only past Phase 0. Windows / macOS builds emit a stub.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-comp must be built on Linux. \
         Use the schema/applet/cli crates from a Windows / macOS host; \
         compositor work requires a Linux dev environment with libdrm, \
         libinput, libxkbcommon, libgbm, and libegl1-mesa."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        "helios-comp: Phase 2 scaffold — smithay event loop not yet wired."
    );

    let state = helios_comp::HeliosState::new();
    let plan = helios_comp::RenderPlan::build(&state);

    tracing::info!(
        viewport_zoom = state.viewport.zoom,
        viewport_w = state.viewport.screen_width,
        viewport_h = state.viewport.screen_height,
        placements = state.placement_count(),
        plan_items = plan.item_count(),
        "initial state + render plan built; exiting"
    );

    // Phase 2 month 2+: replace this with a smithay event loop. Expected
    // structure (commented out so CI compiles without smithay deps):
    //
    //   let mut display: Display<HeliosState> = Display::new()?;
    //   let socket_name = display.handle().add_socket_auto()?;
    //   let mut event_loop = calloop::EventLoop::try_new()?;
    //   let signal = event_loop.get_signal();
    //   let mut state = HeliosState::new_with_display(&mut display)?;
    //   register_input_sources(&mut event_loop, &mut state)?;
    //   register_events_bus_listener(&mut event_loop, &mut state).await?;
    //   register_store_subscriber(&mut event_loop, &mut state).await?;
    //   register_xwayland(&mut event_loop, &mut state)?;
    //   event_loop.run(None, &mut state, |s| {
    //       redraw_if_dirty(s);
    //       display.dispatch_clients(s).ok();
    //       display.flush_clients().ok();
    //   })?;

    Ok(())
}
