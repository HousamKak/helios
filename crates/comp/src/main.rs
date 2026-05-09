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
    use smithay::backend::renderer::{Frame, Renderer};
    use smithay::backend::winit::WinitEvent;
    use smithay::reexports::calloop::generic::Generic;
    use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
    use smithay::reexports::wayland_server::Display;
    use smithay::reexports::winit::platform::pump_events::PumpStatus;
    use smithay::utils::{Rectangle, Transform};
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

    let mut state = helios_comp::WaylandState::new(&dh);

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
    loop {
        // 1. Pump winit events. Resized + CloseRequested handled
        //    here; Input is logged for now (chunk 4 forwards it
        //    into Seat).
        let pump_status = comp_backend.winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                tracing::debug!(w = size.w, h = size.h, "winit window resized");
            }
            WinitEvent::CloseRequested => {
                tracing::info!("winit close requested");
            }
            WinitEvent::Input(_) => {
                // chunk 4: forward to Seat::motion / Seat::button /
                // Seat::keyboard. For now ignored.
            }
            _ => {}
        });
        if let PumpStatus::Exit(_) = pump_status {
            tracing::info!("winit exit; shutting down");
            break;
        }

        // 2. Render one frame. Clear to canvas colour, no surfaces
        //    drawn yet (chunk 2).
        let size = comp_backend.backend.window_size();
        let render_res = comp_backend.backend.bind().and_then(|(renderer, mut fb)| {
            let mut frame = renderer.render(&mut fb, size, Transform::Flipped180)?;
            frame.clear(canvas_color, &[Rectangle::from_size(size)])?;
            let _sync = frame.finish()?;
            Ok(())
        });
        match render_res {
            Ok(()) => {
                if let Err(err) = comp_backend.backend.submit(None) {
                    tracing::warn!(?err, "submit failed");
                }
            }
            Err(err) => {
                tracing::warn!(?err, "render failed");
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
