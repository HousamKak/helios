//! heliOS Wayland compositor — Phase 2 month-3.
//!
//! Linux: opens a smithay-backed Wayland display, registers the
//! `wl_compositor`, `wl_subcompositor`, `wl_shm`, `wl_seat`,
//! `xdg_wm_base`, and `wl_output` globals, and runs a `calloop`-based
//! event loop that drives both the listening socket (new client
//! connections) and the wayland display (per-client request dispatch).
//!
//! No rendering yet — that arrives with `GlesRenderer` + the winit
//! backend in month-4. Real Wayland clients can connect, bind globals,
//! create surfaces and toplevels, but their buffers are not painted.
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
    use smithay::reexports::calloop::generic::Generic;
    use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
    use smithay::reexports::wayland_server::Display;
    use smithay::wayland::socket::ListeningSocketSource;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        "helios-comp: Phase 2 month-3 — calloop event loop with full Wayland protocol surface"
    );

    let display: Display<helios_comp::WaylandState> = Display::new()?;
    let dh = display.handle();
    let mut event_loop: EventLoop<helios_comp::WaylandState> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    // Listening socket — accepts new client connections. Each fired
    // event hands us a Unix stream we hand off to the display via
    // `insert_client`. The closure can't drop the dh handle, so we
    // clone it into the move closure.
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_string_lossy().into_owned();
    tracing::info!(socket = %socket_name, "wayland socket listening");

    let mut dh_for_clients = dh.clone();
    handle
        .insert_source(socket, move |client_stream, _, _state| {
            if let Err(err) = dh_for_clients
                .insert_client(client_stream, Arc::new(helios_comp::ClientState::default()))
            {
                tracing::warn!(?err, "failed to insert wayland client");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert wayland socket source: {e}"))?;

    // Wayland display source — readable when any connected client has
    // pending requests. Display<State> implements AsFd via
    // backend.poll_fd(), so calloop can poll it. The closure dispatches
    // every queued request through smithay's delegate machinery.
    handle
        .insert_source(
            Generic::new(display, Interest::READ, Mode::Level),
            |_, display, state| {
                // SAFETY: NoIoDrop exposes only &mut F, and we never drop
                // the wrapped Display during the closure.
                unsafe {
                    display.get_mut().dispatch_clients(state)?;
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to insert wayland display source: {e}"))?;

    let mut state = helios_comp::WaylandState::new(&dh);
    tracing::info!(
        viewport_zoom = state.canvas.viewport.zoom,
        viewport_w = state.canvas.viewport.screen_width,
        viewport_h = state.canvas.viewport.screen_height,
        placements = state.canvas.placement_count(),
        "initial state ready (compositor + shm + seat + xdg_shell + output advertised)"
    );

    // Lifetime control. `HELIOS_COMP_LIFETIME_SECS=0` (or unset =
    // default 0 in tests, default 3 here) bounds the run for CI.
    // Setting it to e.g. 0 means "run forever" which is what a real
    // session wants; CI sets a small value so the binary exits.
    let runtime = std::env::var("HELIOS_COMP_LIFETIME_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3u64);

    // Clone the display handle once for the per-iteration flush. The
    // event-loop callback runs after each `dispatch` and is where we
    // push outgoing buffers (configures, frame callbacks) to clients.
    let mut dh_for_flush = dh;
    let signal = event_loop.get_signal();
    let deadline = if runtime == 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_secs(runtime))
    };
    tracing::info!(
        seconds = runtime,
        "running event loop ({})",
        if runtime == 0 {
            "indefinitely"
        } else {
            "bounded"
        }
    );

    event_loop.run(Some(Duration::from_millis(16)), &mut state, move |_state| {
        if let Err(err) = dh_for_flush.flush_clients() {
            tracing::warn!(?err, "flush_clients failed");
        }
        if let Some(d) = deadline
            && Instant::now() >= d
        {
            signal.stop();
        }
    })?;

    tracing::info!("shutting down");
    Ok(())
}
