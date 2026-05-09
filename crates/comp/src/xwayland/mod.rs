//! XWayland integration — legacy X11 application support.
//!
//! Phase 2 m-7 lands this module incrementally:
//!   m-7.1: scaffold — module skeleton + `spawn_if_enabled()` hook.
//!   m-7.2: process spawn — `XWayland::spawn` launches the binary,
//!          calloop receives `XWaylandEvent::Ready`, captures the
//!          X11 socket, sets `DISPLAY=:N`. (this commit)
//!   m-7.3: X11Wm + X11Surface lifecycle.
//!   m-7.4: surface buffer rendering for X11Surface.
//!   m-7.5: server-side decoration via _MOTIF_WM_HINTS.
//!   m-7.6: override-redirect (popups, menus).
//!   m-7.7: test matrix + research note.
//!
//! ADR 0004: XWayland is a third producer of `wl_surface`s. Once
//! mapped onto Space, surfaces are rendered identically regardless
//! of producer. Override-redirect popups break that pattern and need
//! their own bookkeeping.
//!
//! Reference: smithay/anvil/src/xwayland.rs is the canonical example
//! — read it top-to-bottom; the X11 lifecycle is hard to grep at.

pub mod spawn;

use std::os::unix::net::UnixStream;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum XwaylandError {
    #[error("XWayland spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("calloop insert_source failed: {0}")]
    Insert(String),
}

/// Top-level state for the XWayland integration. Each chunk fills in
/// more fields:
///   m-7.2: `display_number` + `x11_socket` once `Ready` fires.
///   m-7.3: + `X11Wm`, surface↔entity bookkeeping, OR-popups Vec.
pub struct XwmState {
    /// Display number XWayland is listening on. Populated when the
    /// calloop source emits `XWaylandEvent::Ready`. Until then the
    /// `XwmState` lives on `WaylandState` as `None`.
    pub display_number: u32,
    /// Privileged X11 connection to XWayland. m-7.3 hands this to
    /// `X11Wm::start_wm` to drive the window-manager side. We
    /// `Option` it so chunk 7.3 can `take()` ownership.
    pub x11_socket: Option<UnixStream>,
}
