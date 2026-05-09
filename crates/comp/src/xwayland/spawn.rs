//! XWayland process spawn + calloop event source insertion.
//!
//! Phase 2 m-7 chunk 2. `smithay::xwayland::XWayland::spawn` launches
//! the `Xwayland` binary as a child of helios-comp, sets up the
//! Wayland and X11 socket pairs, and returns a calloop EventSource.
//! When the binary finishes starting up, the source emits
//! `XWaylandEvent::Ready { x11_socket, display_number }`. We stash
//! both onto `WaylandState::xwayland` for chunk 7.3 to pick up, and
//! set `DISPLAY=:N` in our process environment so any client we
//! launch will find this X server.
//!
//! The XWayland handle itself stays alive as long as the calloop
//! source is registered; we don't need a separate handle field on
//! state. The `wayland_server::Client` representing the XWayland
//! server connection is also tracked by `Display` automatically;
//! we drop it.
//!
//! Reference: smithay/anvil/src/xwayland.rs `spawn_xwayland_event`
//! handler block.

use std::ffi::OsString;
use std::process::Stdio;

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::xwayland::{XWayland, XWaylandEvent};

use super::{XwaylandError, XwmState};
use crate::WaylandState;

/// Conditionally spawn XWayland. Returns `Ok(true)` when XWayland was
/// spawned (env var set, smithay accepted), `Ok(false)` when disabled
/// (env unset → no-op for backends that don't care). Errors only on
/// spawn failure — usually means the `Xwayland` binary isn't on PATH.
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

/// Spawn XWayland and register its event source on the given calloop.
/// On Linux, opening an abstract socket alongside the filesystem
/// socket lets clients connect via `@/tmp/.X11-unix/X{N}` without
/// touching the filesystem — slightly faster, and matches what most
/// distros' XWayland packaging expects.
fn spawn(
    handle: &LoopHandle<'static, WaylandState>,
    dh: &DisplayHandle,
) -> Result<(), XwaylandError> {
    // `display = None` lets smithay pick the next free number.
    // `envs` empty — XWayland inherits PATH and XDG_RUNTIME_DIR via
    // smithay's clear-and-pass logic. `Stdio::null()` for both
    // streams: XWayland's verbose output is rarely useful and would
    // pollute our trace logs.
    let (xwayland, _xwayland_client) = XWayland::spawn(
        dh,
        None,
        std::iter::empty::<(OsString, OsString)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_user_data| {
            // m-7.3 will use this closure to insert global filters
            // on the XWayland client (so the X server can't bind
            // privileged globals it shouldn't have).
        },
    )?;

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
                // SAFETY: set_var is unsafe in newer Rust because
                // some platforms have multi-threaded environment
                // races. Linux is fine here — we set this once, at
                // startup, before any further child processes can
                // be spawned by the compositor. Children we exec
                // later (skills, applets) will inherit DISPLAY from
                // our environment.
                unsafe {
                    std::env::set_var("DISPLAY", format!(":{}", display_number));
                }
                state.xwayland = Some(XwmState {
                    display_number,
                    x11_socket: Some(x11_socket),
                });
            }
            XWaylandEvent::Error => {
                tracing::error!("xwayland: spawn failed");
                // Leave state.xwayland as None; downstream code
                // gracefully no-ops on the absence.
            }
        })
        .map_err(|e| XwaylandError::Insert(format!("{e}")))?;

    Ok(())
}
