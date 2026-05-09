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

use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::desktop::Window;
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::Client;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{CompositorClientState, CompositorHandler, CompositorState};
use smithay::wayland::output::OutputHandler;
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

    fn commit(&mut self, surface: &WlSurface) {
        // Import any newly-attached SHM buffer into a GLES texture.
        // This is the core handoff: client → wl_shm → SurfaceData →
        // GlesTexture. The render loop reads back from SurfaceData
        // when it walks `space.elements()`.
        on_commit_buffer_handler::<Self>(surface);

        // Refresh space so dead surfaces are pruned and per-output
        // bookkeeping stays consistent. Cheap; called every commit.
        self.space.refresh();
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
// wl_seat — keyboard + pointer + touch (advertisement only for now)
// ===========================================================================
//
// Phase 2 month-3: the seat global is advertised so clients can bind
// wl_seat. No capabilities (keyboard/pointer/touch) are attached yet —
// those need a calloop input loop fed by libinput, which arrives in
// month-4. Until then, `focus_changed` and `cursor_image` are noops.
//
// We pin all three focus types to `WlSurface` (smithay's minimal
// pattern). A future refactor will introduce a `FocusTarget` enum if
// we need to focus things that aren't surfaces (e.g. canvas-only
// entities like agent labels), per the anvil example.

impl SeatHandler for WaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {
        // Phase 2 month-4+: when input arrives, this is where we
        // route data-device focus, primary-selection focus, and
        // (eventually) update the canvas's focused-entity tracker.
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {
        // Phase 2 month-4+: track the client-supplied cursor surface
        // so the renderer can composite it on top of the canvas.
    }
}

smithay::delegate_seat!(WaylandState);

// ===========================================================================
// xdg_shell — toplevel windows + popups
// ===========================================================================
//
// Phase 2 month-3: handlers log when a toplevel or popup is created.
// Phase 2 month-4 wraps each toplevel in a `Window`, places it on
// `Space`, and assigns canvas coordinates from the active desktop's
// placement policy. Until then, the protocol surface is correct
// (configures, ack_configure, popup positioning) but nothing renders.
//
// Only the no-default trait methods are implemented — smithay
// supplies sensible defaults for move_request, resize_request, etc.,
// which we override later when interactive window manipulation
// translates into canvas pan/zoom.

impl XdgShellHandler for WaylandState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // Wrap the toplevel in a `Window` and place it on the space.
        // m-4 chunk 2 uses fullscreen-centred placement (0, 0) — the
        // canvas world-to-screen transform from m-1 is wired in
        // m-5 chunk 5 to honour the active viewport.
        //
        // The initial xdg configure is sent by smithay's xdg_shell
        // automatically when the first commit on an unconfigured
        // surface arrives — we don't call surface.send_configure
        // here.
        let wl_surface = surface.wl_surface().clone();
        let window = Window::new_wayland_window(surface);
        self.space.map_element(window, (0, 0), true);

        // m-4 chunk 4: give the new toplevel keyboard focus so
        // typing into it works immediately. Until we have a real
        // window-manager focus policy (m-7+), focus follows
        // most-recently-mapped — sufficient for the demo.
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let kbd = self.keyboard.clone();
        kbd.set_focus(self, Some(wl_surface), serial);
        tracing::info!("xdg: new toplevel surface mapped + focused");
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        tracing::info!("xdg: new popup surface");
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {
        // Phase 2 month-5+: route input to the popup until the grab
        // ends. Important for menus and dropdowns.
    }

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        // Phase 2 month-5+: re-anchor the popup with a new positioner.
    }
}

smithay::delegate_xdg_shell!(WaylandState);

// ===========================================================================
// wl_output — output advertisement
// ===========================================================================
//
// The output global is created in WaylandState::new (Output owns its
// own global lifetime). OutputHandler has only one method,
// `output_bound`, with a default impl — clients binding wl_output
// don't require any compositor reaction here. The trait still has
// to be implemented for the delegate macro's trait bounds.

impl OutputHandler for WaylandState {}

smithay::delegate_output!(WaylandState);
