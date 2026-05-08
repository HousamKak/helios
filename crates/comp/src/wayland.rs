//! Smithay Wayland integration. Linux-only.
//!
//! Phase 2 month-2 milestone (this commit):
//!   * `WaylandState` wraps the canvas-level `HeliosState` plus the
//!     fields smithay's protocol delegates need.
//!   * `main` constructs a `Display`, binds a Wayland socket, runs a
//!     small dispatch loop. No globals are advertised yet — the
//!     compositor exists as a Wayland server but doesn't expose any
//!     protocol. `wayland-info` will see the server alive; clients
//!     attempting to bind compositor / xdg-shell / shm find nothing.
//!
//! Future commits add (in order):
//!   * `delegate_compositor!` + `CompositorHandler` impl + `CompositorState`
//!   * `delegate_shm!` + ShmHandler + ShmState
//!   * `delegate_xdg_shell!` + XdgShellHandler + XdgShellState
//!   * `delegate_seat!` + SeatHandler + SeatState
//!   * calloop event loop replacing the simple sleep-poll
//!   * GlesRenderer + winit backend for nested-Wayland iteration
//!   * Subscription to `helios-events` to react to entity changes
//!   * Periodic `helios-store` queries to refresh the placement cache
//!
//! Each addition is its own well-bounded commit. Adding them all at
//! once would mean writing ~1k lines of smithay handler code blind
//! and watching CI bisect.

use crate::HeliosState as CanvasState;

/// Top-level state owned by the Wayland event loop. Wraps the
/// canvas-level `HeliosState`. Future smithay protocol-state fields
/// (CompositorState, XdgShellState, ShmState, SeatState, OutputState,
/// DataDeviceState, Space<Window>) attach to this struct directly so
/// the smithay `delegate_*!` macros can find them.
pub struct WaylandState {
    /// Canvas state — viewport, placement cache, active desktop.
    pub canvas: CanvasState,
    // Future fields:
    //   pub compositor_state: smithay::wayland::compositor::CompositorState,
    //   pub xdg_shell_state: smithay::wayland::shell::xdg::XdgShellState,
    //   pub shm_state: smithay::wayland::shm::ShmState,
    //   pub seat_state: smithay::wayland::seat::SeatState<Self>,
    //   pub output_state: smithay::wayland::output::OutputState,
    //   pub data_device_state: smithay::wayland::selection::data_device::DataDeviceState,
    //   pub space: smithay::desktop::Space<smithay::desktop::Window>,
    //   pub renderer: Option<smithay::backend::renderer::gles::GlesRenderer>,
    //   pub xwayland: Option<smithay::xwayland::XWayland>,
}

impl WaylandState {
    pub fn new() -> Self {
        Self {
            canvas: CanvasState::new(),
        }
    }
}

impl Default for WaylandState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_constructs_with_default_canvas() {
        let s = WaylandState::new();
        assert_eq!(s.canvas.placement_count(), 0);
        assert!(s.canvas.active_desktop_id.is_none());
    }
}
