//! Calloop event loop driving the DRM backend.
//!
//! Phase 2 m-6 chunk 7. Replaces the manual winit-style poll loop
//! with a calloop event loop that owns three sources:
//!
//!   * `LibSeatSessionNotifier` — TTY switch (Activate / Pause)
//!     pauses or resumes the DRM device. Without this, switching to
//!     another VT and back leaves the compositor with stale DRM
//!     master state.
//!   * `DrmDeviceNotifier` — kernel page-flip / vblank events. On
//!     `VBlank(crtc)`, we mark the previous frame submitted and
//!     schedule the next frame.
//!   * Wayland display + socket — same shape as the winit path.
//!
//! Chunk 7 paints exactly one canvas-clear frame and lets the loop
//! idle on the next vblank. No client surfaces are walked yet —
//! that's m-6.9. The test criterion is: real TTY → screen turns
//! heliOS-navy, CPU drops to ~0 after the frame is on screen.
//!
//! Reference: smithay/anvil/src/udev.rs — search for
//! `LoopHandle::insert_source` of the DrmDeviceNotifier and the
//! `LibSeatSession` notifier.

use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::backend::drm::DrmEvent;
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::session::Event as SessionEvent;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::socket::ListeningSocketSource;

use super::DrmBackend;
use super::output::OutputData;
use crate::{ClientState, WaylandState};

/// heliOS canvas background colour. Same constant the winit path uses
/// — kept in sync deliberately so both backends paint the same world.
const CANVAS_COLOR: [f32; 4] = [0.05, 0.06, 0.10, 1.0];

/// Drive the DRM backend until either the runtime budget elapses or
/// the wayland display closes. `runtime_secs == 0` runs indefinitely
/// (the production behaviour); positive values bound the run for CI
/// and dev iteration.
pub fn run(
    mut backend: DrmBackend,
    display: Display<WaylandState>,
    mut state: WaylandState,
    runtime_secs: u64,
) -> anyhow::Result<()> {
    let mut dh = display.handle();

    let mut event_loop: EventLoop<WaylandState> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    // Wayland clients listen here. Same shape as the winit path.
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_string_lossy().into_owned();
    tracing::info!(socket = %socket_name, "wayland socket listening (drm path)");
    let mut dh_for_clients = dh.clone();
    handle
        .insert_source(socket, move |client_stream, _, _state| {
            if let Err(err) =
                dh_for_clients.insert_client(client_stream, Arc::new(ClientState::default()))
            {
                tracing::warn!(?err, "failed to insert wayland client");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert wayland socket source: {e}"))?;

    // Wayland display polled via calloop — protocol traffic is
    // dispatched on each iteration.
    handle
        .insert_source(
            Generic::new(display, Interest::READ, Mode::Level),
            |_, display, state| {
                // SAFETY: NoIoDrop exposes only &mut Display; we never
                // drop the display during the closure body.
                unsafe { display.get_mut().dispatch_clients(state)? };
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to insert wayland display source: {e}"))?;

    // Session notifier — TTY switch pause/resume. Chunk 7 only logs
    // and toggles DrmDevice::pause / activate; chunks 8 and 9 will
    // also pause libinput and re-render on resume.
    let session_notifier = backend
        .session
        .take_notifier()
        .ok_or_else(|| anyhow::anyhow!("session notifier already taken"))?;
    // We can't keep `&mut backend` inside the closure because calloop
    // wants `&mut WaylandState` only. Pass behaviour through a shared
    // cell. For chunk 7 we own the DrmDevice and DrmCompositor inside
    // `backend`, but we don't need them from the session callback yet
    // — we only log activate/deactivate transitions. m-6.9 will route
    // these into device.pause / activate via shared ownership.
    handle
        .insert_source(session_notifier, move |event, _, _state| match event {
            SessionEvent::ActivateSession => {
                tracing::info!("session: activated (TTY switch back)");
            }
            SessionEvent::PauseSession => {
                tracing::info!("session: paused (TTY switched away)");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert session notifier source: {e}"))?;

    // libinput source. Real keyboard + pointer events flow through
    // `state.process_input_event` — the same generic handler the
    // winit backend uses, so app-side input behaviour (typing,
    // pan/zoom gestures, button clicks) is identical across the two
    // backends. m-6.8 entry point.
    let libinput_backend = super::input::build_input_backend(&backend.session.session)?;
    handle
        .insert_source(libinput_backend, |event, _, state| {
            state.process_input_event(event);
        })
        .map_err(|e| anyhow::anyhow!("failed to insert libinput source: {e}"))?;

    // DRM page-flip / vblank source. On VBlank(crtc) we mark the
    // previous frame submitted. Chunk 7 doesn't queue a follow-up
    // frame — we paint one frame at startup and then idle. Chunk 9
    // turns this into a continuous render-on-vblank loop.
    let drm_notifier = backend
        .device
        .take_notifier()
        .ok_or_else(|| anyhow::anyhow!("drm notifier already taken"))?;
    handle
        .insert_source(drm_notifier, |event, _meta, _state| match event {
            DrmEvent::VBlank(crtc) => {
                tracing::trace!(?crtc, "DRM vblank");
                // m-6.9 will:
                //   - call compositor.frame_submitted()
                //   - render_frame for the next frame
                //   - queue_frame
                // For chunk 7 we let the canvas just sit on screen.
            }
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM error");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert drm notifier source: {e}"))?;

    // Kick the first frame so the screen actually shows something
    // visible during the chunk-7 demo. No render elements yet —
    // m-6.9 will walk WaylandState::space.elements() here. This
    // proves the renderer + DrmCompositor + KMS pipeline is alive
    // without needing any clients connected.
    paint_initial_frame(&mut backend)?;

    let deadline = if runtime_secs == 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_secs(runtime_secs))
    };
    tracing::info!(
        seconds = runtime_secs,
        "running drm event loop ({})",
        if runtime_secs == 0 {
            "indefinitely"
        } else {
            "bounded"
        }
    );

    // Main loop. calloop blocks in epoll until something fires; CPU
    // is ~0 between events. Each iteration: dispatch ready sources,
    // flush wayland clients, check the runtime budget.
    loop {
        let timeout = match deadline {
            Some(d) => Some(d.saturating_duration_since(Instant::now())),
            None => Some(Duration::from_millis(16)),
        };
        if let Some(t) = timeout
            && t.is_zero()
            && deadline.is_some()
        {
            tracing::info!("runtime budget elapsed; exiting drm loop");
            break;
        }

        event_loop.dispatch(timeout, &mut state)?;
        dh.flush_clients()?;
    }

    Ok(())
}

/// Render and queue exactly one frame. m-6.9 generalises this into a
/// repeated render-on-vblank loop. For chunk 7 we just demonstrate
/// the path: GLES → swapchain buffer → KMS framebuffer → page flip.
fn paint_initial_frame(backend: &mut DrmBackend) -> anyhow::Result<()> {
    let Some(output_data) = backend.outputs.first_mut() else {
        anyhow::bail!("no outputs to render");
    };
    let OutputData {
        compositor, output, ..
    } = output_data;

    // No surface elements yet (chunk 9 walks the wayland space). An
    // empty slice still drives a clear-to-canvas-color frame because
    // DrmCompositor renders the clear-color across regions not
    // covered by any element.
    let elements: &[SolidColorRenderElement] = &[];

    let render_result = compositor
        .render_frame::<_, _>(
            &mut backend.device.renderer,
            elements,
            Color32F::from(CANVAS_COLOR),
            FrameFlags::DEFAULT,
        )
        .map_err(|e| anyhow::anyhow!("render_frame failed: {e}"))?;
    if render_result.is_empty {
        // Nothing changed (uninitialized swapchain shouldn't hit
        // this on the first call, but be defensive).
        tracing::warn!("initial frame reported empty; skipping queue");
        return Ok(());
    }
    compositor
        .queue_frame(())
        .map_err(|e| anyhow::anyhow!("queue_frame failed: {e}"))?;
    let (w, h) = (
        output.current_mode().map(|m| m.size.w).unwrap_or(0),
        output.current_mode().map(|m| m.size.h).unwrap_or(0),
    );
    tracing::info!(w, h, "DRM: initial canvas-clear frame queued");
    Ok(())
}
