//! Smithay Wayland integration. Linux-only.
//!
//! Phase 2 month-3 milestone (this commit):
//!   * `WaylandState` carries `CompositorState`.
//!   * `ClientState` is the per-client data attached on `insert_client`.
//!   * `delegate_compositor!` (in `handlers.rs`) advertises the
//!     `wl_compositor` and `wl_subcompositor` globals. Clients can now
//!     bind them and create `wl_surface`s.
//!
//! Future commits add (in order):
//!   * `delegate_shm!` + ShmHandler + ShmState
//!   * `delegate_xdg_shell!` + XdgShellHandler + XdgShellState
//!   * `delegate_seat!` + SeatHandler + SeatState
//!   * calloop event loop replacing the sleep-poll
//!   * GlesRenderer + winit backend for nested-Wayland iteration
//!   * Subscription to `helios-events` to react to entity changes
//!   * Periodic `helios-store` queries to refresh the placement cache

use smithay::input::SeatState;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::backend::ClientData;
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;

use crate::HeliosState as CanvasState;

/// Top-level state owned by the Wayland event loop. Smithay's
/// `delegate_*!` macros require trait impls on this struct; those
/// live in `handlers.rs`.
pub struct WaylandState {
    /// Canvas-level state — viewport, placement cache, active desktop.
    pub canvas: CanvasState,

    /// `wl_compositor` + `wl_subcompositor` global state. Owned per
    /// server, shared across clients.
    pub compositor_state: CompositorState,

    /// `wl_shm` global state. Tracks supported formats and validates
    /// buffer pools. ARGB8888 + XRGB8888 are mandatory and always
    /// advertised; we add no extras for now.
    pub shm_state: ShmState,

    /// `wl_seat` global state. One seat per server is enough for
    /// Phase 2 — multi-seat (each user with their own keyboard +
    /// pointer) lands when the agent-multiplexing story is real.
    /// Required by `delegate_xdg_shell`'s transitive trait bounds:
    /// XdgShell wants `SeatHandler` on the same state struct.
    pub seat_state: SeatState<Self>,

    /// `xdg_wm_base` global state. Routes toplevel and popup
    /// surface creation to the compositor. Toplevels are real
    /// windows (apps); popups are menus, dropdowns, tooltips.
    pub xdg_shell_state: XdgShellState,
    // Future fields:
    //   pub output_state: smithay::wayland::output::OutputState,
    //   pub data_device_state: smithay::wayland::selection::data_device::DataDeviceState,
    //   pub space: smithay::desktop::Space<smithay::desktop::Window>,
    //   pub renderer: Option<smithay::backend::renderer::gles::GlesRenderer>,
    //   pub xwayland: Option<smithay::xwayland::XWayland>,
}

impl WaylandState {
    /// Construct fresh state. Requires the display handle to register
    /// the compositor and shm globals.
    pub fn new(display_handle: &DisplayHandle) -> Self {
        let mut seat_state = SeatState::<Self>::new();
        // Register the seat-0 global so clients can bind wl_seat.
        // The returned `Seat<Self>` would be used to attach
        // keyboard/pointer/touch capabilities; Phase 2 month-3 only
        // advertises the seat — capabilities arrive with the calloop
        // input loop in month-4.
        let _seat = seat_state.new_wl_seat(display_handle, "seat-0");

        Self {
            canvas: CanvasState::new(),
            compositor_state: CompositorState::new::<Self>(display_handle),
            shm_state: ShmState::new::<Self>(display_handle, Vec::new()),
            seat_state,
            xdg_shell_state: XdgShellState::new::<Self>(display_handle),
        }
    }
}

/// Per-client data attached at connection time. Lookup is via
/// `client.get_data::<ClientState>()` from inside protocol handlers.
#[derive(Default)]
pub struct ClientState {
    /// Per-client state for the `wl_compositor` protocol.
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::reexports::wayland_server::Display;

    #[test]
    fn new_constructs_with_compositor_state() {
        let display: Display<WaylandState> = Display::new().unwrap();
        let state = WaylandState::new(&display.handle());
        assert_eq!(state.canvas.placement_count(), 0);
        // If we got here, CompositorState::new::<Self>(...) succeeded,
        // which means delegate_compositor! generated valid
        // GlobalDispatch impls. That's the integration check.
    }

    #[test]
    fn client_state_default_is_empty() {
        let cs = ClientState::default();
        // CompositorClientState's Default is also valid — implicit
        // construction proves the trait bound.
        let _ = &cs.compositor_state;
    }
}
