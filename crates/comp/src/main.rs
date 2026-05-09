//! heliOS Wayland compositor — Phase 2 month-2.
//!
//! Linux: opens a smithay-backed Wayland display, binds a Wayland
//! socket via `add_socket_auto()`, runs a 3-second client dispatch
//! loop, exits. No protocol globals are advertised yet — the
//! compositor exists as a Wayland server but doesn't expose any
//! protocol surface. Future commits add `CompositorState`,
//! `XdgShellState`, `ShmState`, `SeatState`, the calloop event loop,
//! `GlesRenderer`, and the canvas render pipeline.
//!
//! Linux-only past Phase 0. Windows / macOS builds emit a stub.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "helios-comp must be built on Linux. \
         Use the schema/applet/cli crates from a Windows / macOS host; \
         compositor work requires a Linux dev environment with libdrm, \
         libinput, libxkbcommon, libgbm, libegl1-mesa, and libwayland."
    );
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use smithay::reexports::wayland_server::backend::ClientData;
    use smithay::reexports::wayland_server::{Display, ListeningSocket};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tracing_subscriber::EnvFilter;

    /// Per-client data attached to each accepted connection. Empty
    /// for now — protocol delegates will populate it as they land.
    struct EmptyClientData;
    impl ClientData for EmptyClientData {}

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("helios-comp: Phase 2 month-2 — wayland display alive, no globals yet");

    let mut display: Display<helios_comp::WaylandState> = Display::new()?;
    let socket = ListeningSocket::bind_auto("wayland", 1..33)?;
    let socket_name = socket
        .socket_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".to_string());
    tracing::info!(socket = %socket_name, "wayland socket bound");

    let mut state = helios_comp::WaylandState::new();
    tracing::info!(
        viewport_zoom = state.canvas.viewport.zoom,
        viewport_w = state.canvas.viewport.screen_width,
        viewport_h = state.canvas.viewport.screen_height,
        placements = state.canvas.placement_count(),
        "initial state ready"
    );

    // Phase 2 month-2 dispatch loop: accept incoming clients, run
    // dispatch, sleep. Phase 2 month-3 replaces this with a calloop
    // event loop wired to libinput, the events-bus subscriber, and
    // the redraw rhythm.
    let runtime = std::env::var("HELIOS_COMP_LIFETIME_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3u64);
    tracing::info!(seconds = runtime, "running wayland dispatch loop");

    let start = Instant::now();
    let lifetime = Duration::from_secs(runtime);
    while start.elapsed() < lifetime {
        if let Some(stream) = socket.accept()? {
            match display
                .handle()
                .insert_client(stream, Arc::new(EmptyClientData))
            {
                Ok(_) => tracing::info!("wayland client connected"),
                Err(err) => tracing::warn!(?err, "failed to insert client"),
            }
        }
        display.dispatch_clients(&mut state)?;
        display.flush_clients()?;
        std::thread::sleep(Duration::from_millis(50));
    }

    tracing::info!("shutting down");
    Ok(())
}
