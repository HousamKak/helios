//! heliOS Wayland compositor — Phase 2 month-4.
//!
//! Linux: opens a winit window backed by GlesRenderer, runs a real
//! render loop that clears the framebuffer to a heliOS-canvas
//! background colour each frame, and concurrently dispatches Wayland
//! protocol traffic via calloop.
//!
//! Phase 2 month-3 left us with a complete Wayland protocol surface
//! (compositor, shm, seat, xdg_shell, output) but no rendering. This
//! commit boots the renderer. Subsequent chunks add:
//!   * surface texture rendering (chunk 2)
//!   * damage-tracked redraws (chunk 3)
//!   * winit-driven input forwarding (chunk 4)
//!   * world-to-screen transforms (m-5 chunk 5)
//!   * pan/zoom gestures (m-5 chunk 6)
//!   * surface↔entity mapping + events emission (m-5 chunk 7)
//!   * helios-store-driven entity moves (m-5 chunk 8)
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
    use smithay::backend::renderer::Color32F;
    use smithay::backend::winit::WinitEvent;
    use smithay::desktop::space::render_output;
    use smithay::reexports::calloop::generic::Generic;
    use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
    use smithay::reexports::wayland_server::Display;
    use smithay::reexports::winit::platform::pump_events::PumpStatus;
    use smithay::wayland::socket::ListeningSocketSource;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Backend selection. `HELIOS_COMP_BACKEND` overrides the default;
    // when unset, we use winit. m-6.7 onward, the DRM path is a real
    // calloop-driven event loop; client surfaces are walked starting
    // m-6.9.
    let requested_backend =
        std::env::var("HELIOS_COMP_BACKEND").unwrap_or_else(|_| "winit".to_string());
    tracing::info!(backend = %requested_backend, "backend selected");

    if requested_backend == "drm" {
        let backend = helios_comp::backend::drm::DrmBackend::init()
            .map_err(|err| anyhow::anyhow!("DRM backend: {err}"))?;

        let display: smithay::reexports::wayland_server::Display<helios_comp::WaylandState> =
            smithay::reexports::wayland_server::Display::new()?;
        let dh = display.handle();
        let mut state = helios_comp::WaylandState::new(&dh);

        // m-8.3: events-bus publisher (DRM path).
        let (events_pub_tx, events_pub_rx) =
            std::sync::mpsc::channel::<helios_schema::SystemEvent>();
        helios_comp::events_publisher::spawn(
            helios_comp::events_publisher::socket_path_from_env(),
            events_pub_rx,
        );
        state.events_tx = Some(events_pub_tx);

        let runtime = std::env::var("HELIOS_COMP_LIFETIME_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0u64);
        return helios_comp::backend::drm::run::run(backend, display, state, runtime);
    }
    if requested_backend != "winit" {
        return Err(anyhow::anyhow!(
            "unknown HELIOS_COMP_BACKEND value: {requested_backend} (expected 'winit' or 'drm')"
        ));
    }

    tracing::info!(
        "helios-comp: Phase 2 month-4 chunk 1 — winit + GlesRenderer up, empty render loop"
    );

    // Wayland setup: display + listening socket + calloop event loop.
    // (Identical to the m-3 setup, except the loop drives by hand
    // below rather than via event_loop.run.)
    let display: Display<helios_comp::WaylandState> = Display::new()?;
    let dh = display.handle();
    let mut event_loop: EventLoop<helios_comp::WaylandState> = EventLoop::try_new()?;
    let handle = event_loop.handle();

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

    // m-7.2: opt-in XWayland spawn. No-op when HELIOS_XWAYLAND_ENABLED
    // is unset; otherwise launches the Xwayland binary as a child
    // process and registers the calloop event source that listens
    // for XWaylandEvent::Ready. Until that fires, state.xwayland is
    // None; afterwards it carries the X11 socket and display number.
    if helios_comp::xwayland::spawn::spawn_if_enabled(&handle, &dh)
        .map_err(|err| anyhow::anyhow!("xwayland: {err}"))?
    {
        tracing::info!("xwayland: spawn requested (winit path)");
    }

    let mut state = helios_comp::WaylandState::new(&dh);

    // m-8.3: spawn the events-bus publisher on a background thread
    // and wire its sender end onto state. Surface lifecycle handlers
    // (XdgShell + XwmHandler) will fan SurfaceMapped / SurfaceUnmapped
    // onto the bus from this point. Channel is unbounded; if the
    // events daemon is down, publish errors log at debug and the
    // compositor keeps rendering — best-effort by design.
    let (events_pub_tx, events_pub_rx) = std::sync::mpsc::channel::<helios_schema::SystemEvent>();
    helios_comp::events_publisher::spawn(
        helios_comp::events_publisher::socket_path_from_env(),
        events_pub_rx,
    );
    state.events_tx = Some(events_pub_tx);

    // m-5 chunk 8: subscribe to the heliOS events bus on a
    // background thread. EntityPlaced events get forwarded into the
    // render loop via a std::sync::mpsc channel; the main loop
    // drains it once per iteration and applies the moves to the
    // corresponding windows. Other event variants are dropped
    // (compositor doesn't care about ProcessExec, JournalRecord, etc.).
    //
    // The events daemon may not be running (dev iteration without
    // helios-events alive) — the subscriber retries on a 2s cadence
    // so the compositor still runs.
    let (events_tx, events_rx) = std::sync::mpsc::channel::<helios_comp::EntityMove>();
    helios_comp::events_client::spawn(
        helios_comp::events_client::socket_path_from_env(),
        events_tx,
    );

    // Bring up the winit + GlesRenderer backend. This opens a
    // native window in the host Wayland session and creates an EGL
    // context bound to that window's surface.
    let mut comp_backend = helios_comp::CompBackend::init()
        .map_err(|e| anyhow::anyhow!("winit/GlesRenderer bootstrap failed: {e}"))?;
    let initial_size = comp_backend.backend.window_size();
    tracing::info!(
        width = initial_size.w,
        height = initial_size.h,
        "winit window opened, GlesRenderer ready"
    );

    // heliOS canvas background. Dark navy — meant to read as
    // "the world has a colour distinct from any single surface".
    // Phase 2 m-7+ will replace this with a shader-driven gradient.
    let canvas_color = Color32F::from([0.05, 0.06, 0.10, 1.0]);

    let runtime = std::env::var("HELIOS_COMP_LIFETIME_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3u64);
    let mut dh_for_flush = dh;
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

    // Manual loop: pump winit, render, dispatch wayland clients.
    // Anvil's pattern (anvil/src/winit.rs:214). We can't use
    // EventLoop::run because we need to interleave winit's event
    // pump with the render step; calloop wraps the wayland side
    // only.
    //
    // `state.full_redraw` is a saturating counter incremented on
    // events that invalidate previous frames' contents (resize,
    // pan, zoom). While > 0, we pass age=0 to render_output, which
    // forces a full redraw and restores correctness; once it ticks
    // back to zero, normal `buffer_age`-based partial redraws
    // resume. Lives on state because state-side methods (zoom,
    // pan) need to bump it.
    loop {
        // 1. Pump winit events. Resized + CloseRequested handled
        //    here; Input is logged for now (chunk 4 forwards it
        //    into Seat).
        let pump_status = comp_backend.winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                tracing::debug!(w = size.w, h = size.h, "winit window resized");
                // Push the new mode to the output so wl_output
                // clients see the updated geometry. The damage
                // tracker is constructed from the output (auto
                // mode), so it picks up the change.
                let new_mode = smithay::output::Mode {
                    size,
                    refresh: 60_000,
                };
                state
                    .output
                    .change_current_state(Some(new_mode), None, None, None);
                state.output.set_preferred(new_mode);
                // Force a full redraw next frame: previous frame's
                // pixels are now meaningless.
                state.full_redraw = 4;
            }
            WinitEvent::CloseRequested => {
                tracing::info!("winit close requested");
            }
            WinitEvent::Input(input_event) => {
                // chunk 4: forward into the seat. WaylandState
                // owns pointer + keyboard handles and routes the
                // event to the focused surface.
                state.process_winit_input(input_event);
            }
            _ => {}
        });
        if let PumpStatus::Exit(_) = pump_status {
            tracing::info!("winit exit; shutting down");
            break;
        }

        // 1b. Drain the events-bus channel. Each EntityMove
        //     repositions a known window via state.move_entity,
        //     which re-maps the corresponding Window on Space and
        //     bumps full_redraw. Bounded to 64 per iteration so a
        //     burst of bus events can't starve rendering.
        for _ in 0..64 {
            match events_rx.try_recv() {
                Ok(m) => state.move_entity(&m.entity_id, m.world),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }

        // 2. Render one frame. Walk space.elements() via
        //    space::render_output, which builds RenderElements from
        //    each Window's surface tree and draws them via the
        //    OutputDamageTracker. Clear regions damage_tracker
        //    decides aren't covered by surfaces are filled with the
        //    canvas colour. Empty space → fully canvas-coloured.
        //
        //    The damage tracker uses `buffer_age` to decide which
        //    historical frame's damage rects to merge. age=0 always
        //    forces a full redraw; we use age=0 only when
        //    `full_redraw` is non-zero (just resized, etc.) and let
        //    the normal `backend.buffer_age()` logic handle the rest.
        state.full_redraw = state.full_redraw.saturating_sub(1);
        let age = if state.full_redraw > 0 {
            0
        } else {
            comp_backend.backend.buffer_age().unwrap_or(0)
        };
        // The bind borrow on backend has to release before submit(),
        // so the rendering happens in a scope that returns the damage
        // rectangles by value.
        let damage_to_submit: Option<
            Vec<smithay::utils::Rectangle<i32, smithay::utils::Physical>>,
        > = match comp_backend.backend.bind() {
            Ok((renderer, mut fb)) => {
                // No custom render elements yet (no cursor, no
                // chrome). The C type parameter is inferred from
                // SolidColorRenderElement, which implements
                // RenderElement<GlesRenderer>. m-4 chunk 4 adds
                // a cursor element here.
                let custom_elements: &[smithay::backend::renderer::element::solid::SolidColorRenderElement] = &[];
                match render_output(
                    &state.output,
                    renderer,
                    &mut fb,
                    1.0,
                    age,
                    [&state.space],
                    custom_elements,
                    &mut state.damage_tracker,
                    canvas_color,
                ) {
                    Ok(result) => result.damage.map(|d| d.to_vec()),
                    Err(err) => {
                        tracing::warn!(?err, "render_output failed");
                        None
                    }
                }
            }
            Err(err) => {
                tracing::warn!(?err, "bind failed");
                None
            }
        };

        if let Some(damage) = damage_to_submit {
            if let Err(err) = comp_backend.backend.submit(Some(&damage)) {
                tracing::warn!(?err, "submit failed");
            }
            // Send frame callbacks so clients know it's safe to draw
            // their next frame.
            for window in state.space.elements() {
                window.send_frame(&state.output, Duration::from_millis(0), None, |_, _| {
                    Some(state.output.clone())
                });
            }
        }

        // 3. Dispatch wayland clients (and any other calloop sources)
        //    with a short timeout so we redraw at ~60fps even if no
        //    clients are doing anything.
        if let Err(err) = event_loop.dispatch(Some(Duration::from_millis(16)), &mut state) {
            tracing::error!(?err, "calloop dispatch failed");
            break;
        }

        // 4. Push outgoing buffers to clients (configures, frame
        //    callbacks). Without this, clients may stall waiting for
        //    server-side responses.
        if let Err(err) = dh_for_flush.flush_clients() {
            tracing::warn!(?err, "flush_clients failed");
        }

        // 5. Lifetime gate.
        if let Some(d) = deadline
            && Instant::now() >= d
        {
            tracing::info!("lifetime deadline reached");
            break;
        }
    }

    tracing::info!("shutting down");
    Ok(())
}
