//! Calloop event loop driving the DRM backend.
//!
//! Phase 2 m-6 chunk 7 introduced this; chunk 9 closes it out by
//! turning the single-frame paint into a continuous render-on-vblank
//! loop and routing session pause/resume into the DRM device. Owns
//! five sources:
//!
//!   * `LibSeatSessionNotifier` — TTY switch (Activate / Pause)
//!     pauses or resumes the DRM device via
//!     `DrmBackend::handle_session_event`.
//!   * `LibinputInputBackend` — real keyboard / pointer events
//!     forwarded to `state.process_input_event` (m-6.8).
//!   * `DrmDeviceNotifier` — kernel page-flip / vblank events drive
//!     `DrmBackend::tick`, which renders the next frame.
//!   * Wayland display + listening socket.
//!
//! `DrmBackend` is owned by an `Rc<RefCell<…>>` so the calloop
//! callbacks (one per source) can share mutable access. Calloop runs
//! single-threaded, so RefCell is the right primitive — no Mutex
//! contention, no Send bounds, panic-on-double-borrow is a real
//! programming error and surfacing it is what we want.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay::backend::drm::DrmEvent;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::socket::ListeningSocketSource;

use super::DrmBackend;
use crate::{ClientState, WaylandState};

/// Drive the DRM backend until either the runtime budget elapses or
/// the wayland display closes. `runtime_secs == 0` runs indefinitely
/// (the production behaviour); positive values bound the run for CI
/// and dev iteration.
pub fn run(
    backend: DrmBackend,
    display: Display<WaylandState>,
    mut state: WaylandState,
    runtime_secs: u64,
) -> anyhow::Result<()> {
    let mut dh = display.handle();

    let mut event_loop: EventLoop<WaylandState> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    let backend = Rc::new(RefCell::new(backend));

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

    // Wayland display polled via calloop — protocol traffic dispatched
    // on each iteration.
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

    // Take notifiers + libinput out of the backend before borrowing
    // it from inside closures. Each is moved into its own calloop
    // source.
    let (session_notifier, drm_notifier, libinput_backend) = {
        let mut b = backend.borrow_mut();
        let s = b
            .session
            .take_notifier()
            .ok_or_else(|| anyhow::anyhow!("session notifier already taken"))?;
        let d = b
            .device
            .take_notifier()
            .ok_or_else(|| anyhow::anyhow!("drm notifier already taken"))?;
        let l = super::input::build_input_backend(&b.session.session)?;
        (s, d, l)
    };

    // Session source — TTY switch pause/resume.
    let backend_for_session = Rc::clone(&backend);
    handle
        .insert_source(session_notifier, move |event, _, _state| {
            backend_for_session.borrow_mut().handle_session_event(event);
        })
        .map_err(|e| anyhow::anyhow!("failed to insert session notifier source: {e}"))?;

    // libinput source — real keyboard + pointer events.
    handle
        .insert_source(libinput_backend, |event, _, state| {
            state.process_input_event(event);
        })
        .map_err(|e| anyhow::anyhow!("failed to insert libinput source: {e}"))?;

    // DRM page-flip / vblank source. On each VBlank, render the next
    // frame from `state.space`. Errors during render are logged but
    // don't bring down the loop — they're typically recoverable
    // (transient DRM busy, etc.).
    let backend_for_vblank = Rc::clone(&backend);
    handle
        .insert_source(drm_notifier, move |event, _meta, state| match event {
            DrmEvent::VBlank(crtc) => {
                tracing::trace!(?crtc, "DRM vblank");
                if let Err(err) = backend_for_vblank.borrow_mut().tick(state) {
                    tracing::warn!(?err, "drm tick on vblank failed");
                }
            }
            DrmEvent::Error(err) => {
                tracing::error!(?err, "DRM error");
            }
        })
        .map_err(|e| anyhow::anyhow!("failed to insert drm notifier source: {e}"))?;

    // Kick the first frame so the screen has something visible
    // immediately. Without this, the display sits black until a
    // client connects and triggers damage.
    if let Err(err) = backend.borrow_mut().tick(&mut state) {
        tracing::warn!(?err, "initial drm tick failed");
    }

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
