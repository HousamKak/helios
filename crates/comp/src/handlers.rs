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
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Resource};
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
        // Wrap the toplevel in a `Window` and place it on the space
        // at the world origin transformed by the active viewport.
        // Default viewport (zoom=1.0, centre=(0,0)) means a window
        // at world (0,0) renders at screen-centre — m-5 chunk 5's
        // spawn policy from ADR 0004.
        //
        // The initial xdg configure is sent by smithay's xdg_shell
        // automatically when the first commit on an unconfigured
        // surface arrives — we don't call surface.send_configure
        // here.
        let wl_surface = surface.wl_surface().clone();
        let surface_id = wl_surface.id();
        let window = Window::new_wayland_window(surface);
        let screen_pos = self.world_to_screen(crate::WorldPoint::ORIGIN);
        self.space.map_element(window, screen_pos, true);

        // m-5 chunk 7: bind a fresh canvas EntityId to this surface
        // and record its world position. The entity_id is the
        // identifier external producers (skills, agents, applets)
        // use to address this window via helios-store's
        // canvas_entities table.
        let entity_id = helios_schema::generate_id();
        self.surface_to_entity
            .insert(surface_id.clone(), entity_id.clone());
        self.entity_to_world
            .insert(entity_id.clone(), crate::WorldPoint::ORIGIN);

        // m-8.3: announce the surface lifecycle on the events bus.
        // client_pid comes from the wayland-server credentials of the
        // owning client. None when the credentials aren't available
        // (the client disconnected mid-call, or wayland-server
        // doesn't surface them on this platform).
        let client_pid = wl_surface.client().and_then(|c| {
            c.get_credentials(&self.display_handle)
                .ok()
                .map(|cr| cr.pid)
        });
        self.emit_event(helios_schema::EventPayload::SurfaceMapped {
            surface_id: entity_id.clone(),
            client_pid,
            kind: "xdg_toplevel".to_string(),
        });

        // m-4 chunk 4: give the new toplevel keyboard focus so
        // typing into it works immediately. Until we have a real
        // window-manager focus policy (m-7+), focus follows
        // most-recently-mapped — sufficient for the demo.
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let kbd = self.keyboard.clone();
        kbd.set_focus(self, Some(wl_surface), serial);
        tracing::info!(?screen_pos, %entity_id, ?client_pid, "xdg: new toplevel mapped + focused");
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        tracing::info!("xdg: new popup surface");
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        // m-5 chunk 7: drop the canvas entity binding when the
        // client disconnects.
        let surface_id = surface.wl_surface().id();
        if let Some(entity_id) = self.surface_to_entity.remove(&surface_id) {
            // m-8.3: emit SurfaceUnmapped before dropping the binding
            // so subscribers see the entity_id while it's still
            // resolvable in their projection of the canvas.
            self.emit_event(helios_schema::EventPayload::SurfaceUnmapped {
                surface_id: entity_id.clone(),
            });
            self.entity_to_world.remove(&entity_id);
            tracing::info!(%entity_id, "xdg: toplevel destroyed; entity unbound");
        }
        // smithay's space.refresh() (called from CompositorHandler::commit)
        // prunes the dead window automatically; no explicit unmap_elem.
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
