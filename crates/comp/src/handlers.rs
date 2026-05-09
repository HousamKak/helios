//! Smithay protocol handler trait impls + delegate macros.
//!
//! Smithay's `delegate_*!` macros generate the boilerplate
//! `Dispatch` / `GlobalDispatch` impls on `WaylandState`. The trait
//! impls below are what those macros call into when a client request
//! arrives.
//!
//! Phase 2 month-3 (this commit) lands one delegate: `compositor`.
//! That advertises `wl_compositor` + `wl_subcompositor` globals.
//! Clients can now bind them and create surfaces.
//!
//! Subsequent commits add one delegate per push:
//!   * `delegate_shm!`
//!   * `delegate_xdg_shell!`
//!   * `delegate_seat!`
//!   * `delegate_output!`
//!   * `delegate_data_device!`
//!
//! Each is its own focused commit. Bundling them risks 1k+ lines of
//! handler code that has to be validated against real client behaviour
//! all at once.

use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};

use crate::wayland::{ClientState, WaylandState};

// ===========================================================================
// wl_compositor + wl_subcompositor
// ===========================================================================

impl CompositorHandler for WaylandState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("every client should be inserted with ClientState")
            .compositor_state
    }

    fn commit(&mut self, _surface: &WlSurface) {
        // Phase 2 month-3: surface state is committed but we don't
        // have a renderer yet — nothing to redraw. Phase 2 month-4
        // wires this to the renderer's redraw scheduler so each
        // commit triggers a frame for the affected output.
    }
}

smithay::delegate_compositor!(WaylandState);

// ===========================================================================
// wl_buffer (shared by every protocol that handles client buffers)
// ===========================================================================

impl BufferHandler for WaylandState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {
        // Phase 2 month-3: nothing tracks buffer ownership yet.
        // The renderer (month-4+) will release any cached texture
        // imported from this buffer here.
    }
}

// ===========================================================================
// wl_shm
// ===========================================================================

impl ShmHandler for WaylandState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

smithay::delegate_shm!(WaylandState);

// ===========================================================================
// xdg_shell — toplevels + popups
// ===========================================================================

impl XdgShellHandler for WaylandState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, _surface: ToplevelSurface) {
        // Phase 2 month-3: log only. Phase 2 month-4 wraps this in
        // a `Window`, places it on `Space`, and assigns canvas
        // coordinates from the active desktop's policy.
        tracing::info!("xdg: new toplevel surface");
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        tracing::info!("xdg: new popup surface");
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: WlSeat, _serial: Serial) {
        // Phase 2 month-5+: interactive window move translates into
        // a canvas pan that follows the cursor while the gesture
        // continues. Until then, ignore — the client's window can
        // be moved via the canvas's own pan/zoom.
    }

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
        // Phase 2 month-5+: interactive resize via the entity's
        // bounding box on the canvas.
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {
        // Phase 2 month-5+: route input to the popup until grab
        // ends. Important for menus and dropdowns.
    }

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        // Phase 2 month-5+: re-anchor the popup with the new positioner.
    }
}

smithay::delegate_xdg_shell!(WaylandState);
