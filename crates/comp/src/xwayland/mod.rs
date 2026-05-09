//! XWayland integration — legacy X11 application support.
//!
//! Phase 2 m-7 lands this module incrementally:
//!   m-7.1: scaffold — module skeleton + `spawn_if_enabled()` returns
//!          `NotImplemented` when `HELIOS_XWAYLAND_ENABLED=1`. The
//!          `xwayland` smithay feature is on; downstream code can
//!          start importing types but no XWayland process spawns yet.
//!          (this commit)
//!   m-7.2: process spawn — `XWayland::spawn` launches the binary.
//!          calloop event source receives `XWaylandEvent::Ready`,
//!          captures the X11 socket, sets `DISPLAY=:N` for the
//!          environment used to spawn child processes.
//!   m-7.3: X11Wm + X11Surface lifecycle — wires `XwmHandler`
//!          callbacks (new_window, map, configure_request, etc.)
//!          and maps X surfaces onto `Space` as
//!          `Window(WindowSurface::X11(_))`.
//!   m-7.4: surface buffer rendering — minor edits to handlers.rs
//!          so X11 surface commits go through the same import path
//!          as native xdg_toplevels.
//!   m-7.5: server-side decoration — set `_MOTIF_WM_HINTS` to
//!          `NoDecoration` so X clients don't draw their own chrome.
//!   m-7.6: override-redirect — separate Vec for popups; rendered
//!          last, screen-fixed, ignore pan/zoom.
//!   m-7.7: test matrix + research note for known quirks.
//!
//! ADR 0004: XWayland is a third producer of `wl_surface`s. Once
//! mapped onto Space, surfaces are rendered identically regardless
//! of producer. Override-redirect popups break that pattern and need
//! their own bookkeeping.
//!
//! Reference: smithay/anvil/src/xwayland.rs is the canonical example
//! — read it top-to-bottom; the X11 lifecycle is hard to grep at.

pub mod spawn;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum XwaylandError {
    #[error("XWayland not yet implemented (m-7 in progress)")]
    NotImplemented,
}

/// Top-level state for the XWayland integration. Each chunk fills in
/// more fields:
///   m-7.2: + the `XWayland` handle and its calloop registration token.
///   m-7.3: + `X11Wm`, surface↔entity bookkeeping, OR-popups Vec.
pub struct XwmState {
    /// Display number XWayland is listening on (set on
    /// `XWaylandEvent::Ready`). `0` until then. m-7.2 fills this in.
    pub display_number: u32,
}

/// Conditionally spawn XWayland. Gated behind `HELIOS_XWAYLAND_ENABLED=1`
/// so the env var acts as an opt-in feature flag during m-7
/// development. Returns `Ok(None)` if the env var is unset (XWayland
/// disabled) and `Err(NotImplemented)` while the chunks are
/// incomplete.
pub fn spawn_if_enabled() -> Result<Option<XwmState>, XwaylandError> {
    if std::env::var("HELIOS_XWAYLAND_ENABLED").ok().as_deref() != Some("1") {
        return Ok(None);
    }
    tracing::warn!("xwayland: not yet implemented (m-7 in progress)");
    Err(XwaylandError::NotImplemented)
}
