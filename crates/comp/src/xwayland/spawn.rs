//! XWayland process spawn + calloop event source insertion.
//!
//! Phase 2 m-7 chunks 2 + 3. `smithay::xwayland::XWayland::spawn`
//! launches the `Xwayland` binary as a child of helios-comp, sets up
//! the Wayland and X11 socket pairs, and returns a calloop
//! EventSource. When the binary finishes starting up, the source
//! emits `XWaylandEvent::Ready { x11_socket, display_number }`.
//! Chunk 2 set `DISPLAY=:N` and stashed the socket; chunk 3 also
//! starts the X11 window manager via `X11Wm::start_wm`, plumbing
//! it into `WaylandState::xwm` so subsequent X events flow through
//! `XwmHandler` callbacks.
//!
//! Reference: smithay/anvil/src/xwayland.rs `spawn_xwayland_event`
//! handler block.

use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Mutex;

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::xwayland::{X11Wm, XWayland, XWaylandEvent};

use super::{XwaylandError, XwmState};
use crate::WaylandState;

pub fn spawn_if_enabled(
    handle: &LoopHandle<'static, WaylandState>,
    dh: &DisplayHandle,
) -> Result<bool, XwaylandError> {
    if std::env::var("HELIOS_XWAYLAND_ENABLED").ok().as_deref() != Some("1") {
        return Ok(false);
    }
    spawn(handle, dh)?;
    Ok(true)
}

fn spawn(
    handle: &LoopHandle<'static, WaylandState>,
    dh: &DisplayHandle,
) -> Result<(), XwaylandError> {
    let (xwayland, xwayland_client) = XWayland::spawn(
        dh,
        None,
        std::iter::empty::<(OsString, OsString)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_user_data| {
            // m-7.5+ may insert global filters here so the X server
            // can't bind privileged globals (DRM, control over
            // unrelated clients, etc.).
        },
    )?;

    // The wayland-server-side client representing the XWayland
    // process. Smithay's X11Wm::start_wm consumes it. We hand it
    // through the calloop closure via Mutex<Option<…>> so the FnMut
    // can `take` it on first fire (Ready) without violating the
    // closure-trait contract; the source disables itself afterwards.
    let xwayland_client = Mutex::new(Some(xwayland_client));
    let handle_clone = handle.clone();
    let dh_clone = dh.clone();

    handle
        .insert_source(xwayland, move |event, _, state| match event {
            XWaylandEvent::Ready {
                x11_socket,
                display_number,
            } => {
                tracing::info!(
                    display = display_number,
                    "xwayland: ready, DISPLAY=:{} set",
                    display_number,
                );
                // SAFETY: `set_var` is unsafe in Rust 1.95 because
                // some platforms have multi-threaded environment
                // races. Linux is fine here — we set this once at
                // startup, before any further child processes are
                // spawned by the compositor.
                unsafe {
                    std::env::set_var("DISPLAY", format!(":{}", display_number));
                }

                // Pull the client out of the Mutex<Option<…>>; this
                // is the first and only time Ready fires.
                let client = match xwayland_client.lock().ok().and_then(|mut g| g.take()) {
                    Some(c) => c,
                    None => {
                        tracing::error!("xwayland: client already consumed (double Ready?)");
                        return;
                    }
                };

                // Stand up the X11 window manager. start_wm registers
                // a calloop event source for the X11 socket, so X
                // protocol events flow through XwmHandler callbacks
                // on `state` from now on.
                match X11Wm::start_wm(handle_clone.clone(), x11_socket, client) {
                    Ok(xwm) => {
                        state.xwm = Some(xwm);
                        state.xwayland = Some(XwmState {
                            display_number,
                            x11_socket: None,
                        });
                        tracing::info!("xwayland: X11Wm started");
                    }
                    Err(err) => {
                        tracing::error!(?err, "xwayland: X11Wm::start_wm failed");
                    }
                }
                let _ = &dh_clone;
            }
            XWaylandEvent::Error => {
                tracing::error!("xwayland: spawn failed");
            }
        })
        .map_err(|e| XwaylandError::Insert(format!("{e}")))?;

    Ok(())
}
