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

use std::collections::HashMap;

use helios_schema::EntityId;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::desktop::{Space, Window};
use smithay::input::keyboard::{KeyboardHandle, XkbConfig};
use smithay::input::pointer::PointerHandle;
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_server::backend::{ClientData, ObjectId};
use smithay::reexports::wayland_server::{DisplayHandle, Resource};
use smithay::utils::Transform;
use smithay::wayland::compositor::{CompositorClientState, CompositorState};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;

use crate::WorldPoint;

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

    /// The single output advertised to clients. Phase 2 month-3
    /// fakes one 1920x1080@60 output called "output-0"; real
    /// multi-output handling lands with the DRM backend in month-5+.
    /// Held on the state so the global stays alive (dropping the
    /// `Output` un-advertises the global to clients).
    pub output: Output,

    /// Mapped windows, the canonical scenegraph for rendering. Each
    /// `xdg_toplevel` becomes a `Window` here and is given a position
    /// via `space.map_element(...)`. The render loop walks
    /// `space.elements()` each frame.
    pub space: Space<Window>,

    /// Tracks per-surface damage rectangles between frames. m-4 chunk 2
    /// uses it via `space::render_output` which always reports damage
    /// as full-output (age=0). m-4 chunk 3 wires `backend.buffer_age()`
    /// so idle frames report no damage and skip submission.
    pub damage_tracker: OutputDamageTracker,

    /// The seat itself — owns the keyboard/pointer/touch capabilities.
    /// We keep it on state to add capabilities later (e.g. touch in
    /// m-6+) without having to look it up via SeatState.
    pub seat: Seat<Self>,

    /// Cloneable handle to the seat's pointer. Used by the input
    /// dispatcher in `process_winit_input` to forward winit motion
    /// and button events into the wayland protocol.
    pub pointer: PointerHandle<Self>,

    /// Cloneable handle to the seat's keyboard. Used by the input
    /// dispatcher to forward keystrokes, and by XdgShellHandler to
    /// move keyboard focus when a new toplevel arrives.
    pub keyboard: KeyboardHandle<Self>,

    /// Last-known pointer location in output-logical coordinates.
    /// Updated on every PointerMotionAbsolute event. m-5 chunk 6 uses
    /// it as the anchor for cursor-centred zoom and as the previous-
    /// position reference for middle-drag pan deltas.
    pub pointer_pos: smithay::utils::Point<f64, smithay::utils::Logical>,

    /// True while the user is holding middle mouse button to pan
    /// the canvas. Each subsequent PointerMotionAbsolute while this
    /// is set updates `viewport.center` by the pixel delta divided
    /// by zoom.
    pub pan_dragging: bool,

    /// Saturating counter for full-output redraws. Bumped to 4 on
    /// viewport changes (pan/zoom) and on output mode changes
    /// (resize). While > 0, the render loop passes age=0 to
    /// `space::render_output`, which forces a full redraw and
    /// clears stale pixels. main.rs decrements this each frame.
    pub full_redraw: u8,

    /// `wl_surface ObjectId` → canonical `EntityId` map. Each
    /// xdg_toplevel becomes a canvas entity at construction time.
    /// The map is runtime-only (per ADR 0004): wayland surfaces are
    /// ephemeral, so persisting `surface_id → entity_id` would
    /// produce stale rows on every client crash. Removed on
    /// `toplevel_destroyed`.
    pub surface_to_entity: HashMap<ObjectId, EntityId>,

    /// Per-entity world position. Used by
    /// `reapply_viewport_to_windows` so each window draws at its own
    /// world coordinate transformed by the active viewport (rather
    /// than every window collapsing onto the world origin like in
    /// chunk 6). Updated by m-5 chunk 8 when an external producer
    /// (skill / agent / applet) writes a new position.
    pub entity_to_world: HashMap<EntityId, WorldPoint>,

    /// XWayland integration state. `None` when XWayland is disabled
    /// (default) or hasn't yet emitted `Ready`. The
    /// `xwayland::spawn::spawn` calloop callback fills this in on
    /// `XWaylandEvent::Ready`. m-7.3 picks up the X11 socket from
    /// here to start the X11Wm.
    pub xwayland: Option<crate::xwayland::XwmState>,

    /// Active X11 window manager. Constructed in the
    /// `XWaylandEvent::Ready` handler once we have the privileged X11
    /// socket. `None` when XWayland is disabled or not yet ready.
    pub xwm: Option<smithay::xwayland::X11Wm>,

    /// `xwayland_shell_v1` global state. Required by smithay's
    /// `XWaylandShellHandler` impl on `WaylandState`. Created at
    /// startup when the xwayland feature is enabled (whether or not
    /// the env var triggers an actual spawn — the global is cheap and
    /// is the protocol vehicle XWayland uses to associate X windows
    /// with their backing wl_surfaces).
    pub xwayland_shell_state: Option<smithay::wayland::xwayland_shell::XWaylandShellState>,
}

impl WaylandState {
    /// Construct fresh state. Requires the display handle to register
    /// the compositor and shm globals.
    pub fn new(display_handle: &DisplayHandle) -> Self {
        let mut seat_state = SeatState::<Self>::new();
        // Register the seat-0 global so clients can bind wl_seat,
        // and attach pointer + keyboard capabilities so the input
        // dispatcher in main.rs has somewhere to forward events.
        // Repeat parameters (200 ms delay, 25 char/s) match the
        // Linux defaults; clients can override per-key.
        let mut seat = seat_state.new_wl_seat(display_handle, "seat-0");
        let pointer = seat.add_pointer();
        let keyboard = seat
            .add_keyboard(XkbConfig::default(), 200, 25)
            .expect("default xkb config should be supported by xkbcommon");

        // One fake output, "output-0", 1920x1080 @ 60 Hz. Real
        // outputs come from the DRM backend (month-5+) or the winit
        // backend (month-4). The output global is advertised to
        // clients so they can pick a scale + mode; subsequent
        // `change_current_state` calls live-update connected clients.
        let output = Output::new(
            "output-0".to_string(),
            PhysicalProperties {
                size: (340, 190).into(),
                subpixel: Subpixel::Unknown,
                make: "heliOS".to_string(),
                model: "virtual-output".to_string(),
            },
        );
        let mode = Mode {
            size: (1920, 1080).into(),
            refresh: 60_000,
        };
        let _output_global = output.create_global::<Self>(display_handle);
        output.change_current_state(
            Some(mode),
            Some(Transform::Normal),
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);

        // Bring up the desktop scenegraph and damage tracker. The
        // single output we just registered is mapped at the world
        // origin; subsequent windows are placed relative to that.
        let mut space: Space<Window> = Space::default();
        space.map_output(&output, (0, 0));
        let damage_tracker = OutputDamageTracker::from_output(&output);

        Self {
            canvas: CanvasState::new(),
            compositor_state: CompositorState::new::<Self>(display_handle),
            shm_state: ShmState::new::<Self>(display_handle, Vec::new()),
            seat_state,
            xdg_shell_state: XdgShellState::new::<Self>(display_handle),
            output,
            space,
            damage_tracker,
            seat,
            pointer,
            keyboard,
            pointer_pos: smithay::utils::Point::from((0.0f64, 0.0f64)),
            pan_dragging: false,
            full_redraw: 1,
            surface_to_entity: HashMap::new(),
            entity_to_world: HashMap::new(),
            xwayland: None,
            xwm: None,
            xwayland_shell_state: Some(
                smithay::wayland::xwayland_shell::XWaylandShellState::new::<Self>(display_handle),
            ),
        }
    }

    /// Look up a window in the space by its `wl_surface` ObjectId.
    /// Returns the cloned Window so the caller can call `space.map_element`
    /// on it. Cheap — Window is a thin Arc-clone.
    pub fn window_by_surface(&self, id: &ObjectId) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.wl_surface().map(|s| s.id() == *id).unwrap_or(false))
            .cloned()
    }

    /// Move the entity associated with `id` to a new world position.
    /// Re-maps the corresponding window so the change is visible
    /// next frame. m-5 chunk 8 will call this on every EntityPlaced
    /// event from the bus / store.
    pub fn move_entity(&mut self, id: &EntityId, world: WorldPoint) {
        self.entity_to_world.insert(id.clone(), world);
        // Find the surface bound to this entity, if any, and re-map.
        let surface_id = self
            .surface_to_entity
            .iter()
            .find(|(_, eid)| *eid == id)
            .map(|(sid, _)| sid.clone());
        if let Some(sid) = surface_id
            && let Some(window) = self.window_by_surface(&sid)
        {
            let screen_pos = self.world_to_screen(world);
            self.space.map_element(window, screen_pos, false);
            self.full_redraw = 4;
        }
    }

    /// Re-map every (non-OR) window on the space using the current
    /// viewport transform applied to each window's per-entity world
    /// position. m-5 chunk 7: each window has an authoritative world
    /// position in `entity_to_world`; if a window has no entity
    /// binding (rare race during destroy), it falls back to the
    /// world origin.
    ///
    /// m-7.6: X11 override-redirect windows (popups, menus,
    /// tooltips) live in screen-pixel space, not canvas-world space
    /// — pan/zoom must NOT move them. Skip them here; their position
    /// is owned by the X client via `configure_notify`.
    pub fn reapply_viewport_to_windows(&mut self) {
        // Snapshot windows + their world positions so we can mutate
        // the space inside the loop.
        let mut targets: Vec<(Window, (i32, i32))> = Vec::new();
        for window in self.space.elements().cloned().collect::<Vec<_>>() {
            // Skip override-redirect: their screen-pixel positions
            // are authoritative.
            if window
                .x11_surface()
                .map(|s| s.is_override_redirect())
                .unwrap_or(false)
            {
                continue;
            }
            let world = window
                .wl_surface()
                .and_then(|s| self.surface_to_entity.get(&s.id()).cloned())
                .and_then(|eid| self.entity_to_world.get(&eid).copied())
                .unwrap_or(WorldPoint::ORIGIN);
            targets.push((window, self.world_to_screen(world)));
        }
        for (window, screen_pos) in targets {
            self.space.map_element(window, screen_pos, false);
        }
        self.full_redraw = 4;
    }

    /// Pan the viewport by a screen-pixel delta (e.g. middle-mouse
    /// drag) and re-map windows so the change is visible next frame.
    pub fn pan_screen(&mut self, dx: f64, dy: f64) {
        self.canvas.viewport.pan_by_screen_pixels(dx, dy);
        self.reapply_viewport_to_windows();
    }

    /// Zoom the viewport around the current cursor position.
    /// `multiplier > 1.0` zooms in, `< 1.0` zooms out.
    pub fn zoom_at_cursor(&mut self, multiplier: f64) {
        let anchor = crate::WorldPoint {
            x: self.pointer_pos.x,
            y: self.pointer_pos.y,
        };
        self.canvas.viewport.zoom_around(anchor, multiplier);
        self.reapply_viewport_to_windows();
    }

    /// Project a world-space point onto the screen using the active
    /// viewport's transform. m-5 chunk 5 entry point — anywhere we
    /// need to know "where on the screen does this world position
    /// land right now?" goes through here.
    pub fn world_to_screen(&self, world: crate::WorldPoint) -> (i32, i32) {
        let t = self.canvas.viewport.world_to_screen_transform();
        let p = t.transform_point(world);
        (p.x.round() as i32, p.y.round() as i32)
    }

    /// Forward an input event into the seat's pointer / keyboard.
    /// Generic over the backend so winit (m-4 chunk 4) and libinput
    /// (m-6 chunk 8) share one handler. The backend-specific bits
    /// (e.g. winit's normalized 0..1 absolute coordinates) are
    /// already abstracted by smithay's `InputBackend` trait —
    /// `event.position_transformed(size)` returns logical-pixel
    /// coordinates regardless of the source backend.
    pub fn process_input_event<B: smithay::backend::input::InputBackend>(
        &mut self,
        event: smithay::backend::input::InputEvent<B>,
    ) {
        use smithay::backend::input::{
            AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyboardKeyEvent,
            PointerButtonEvent, PointerMotionEvent,
        };
        use smithay::input::keyboard::FilterResult;
        use smithay::input::pointer::{ButtonEvent, MotionEvent, RelativeMotionEvent};
        use smithay::utils::SERIAL_COUNTER;

        match event {
            InputEvent::Keyboard { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let key_code = event.key_code();
                let key_state = event.state();
                let kbd = self.keyboard.clone();
                kbd.input::<(), _>(self, key_code, key_state, serial, time, |_, _, _| {
                    FilterResult::Forward
                });
            }
            InputEvent::PointerMotionAbsolute { event } => {
                // Winit emits this for nested-Wayland mouse motion
                // (normalized 0..1, transformed to logical pixels by
                // smithay). Touchscreens via libinput also emit
                // this variant.
                let output_size = match self.output.current_mode() {
                    Some(mode) => mode.size,
                    None => return,
                };
                let logical_size = output_size.to_logical(1);
                let pos = event.position_transformed(logical_size);

                if self.pan_dragging {
                    let dx = pos.x - self.pointer_pos.x;
                    let dy = pos.y - self.pointer_pos.y;
                    if dx != 0.0 || dy != 0.0 {
                        self.pan_screen(dx, dy);
                    }
                }
                self.pointer_pos = pos;

                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let ptr = self.pointer.clone();
                ptr.motion(
                    self,
                    None,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time,
                    },
                );
                ptr.frame(self);
            }
            InputEvent::PointerMotion { event } => {
                // Relative-motion path used by libinput for real mice.
                // Accumulate into pointer_pos, clamp to the active
                // output's logical bounds, then deliver as absolute
                // motion so smithay's pointer handle picks the right
                // focus surface.
                let output_size = match self.output.current_mode() {
                    Some(mode) => mode.size,
                    None => return,
                };
                let logical_size = output_size.to_logical(1);
                let delta = event.delta();
                let mut x = self.pointer_pos.x + delta.x;
                let mut y = self.pointer_pos.y + delta.y;
                x = x.clamp(0.0, (logical_size.w - 1) as f64);
                y = y.clamp(0.0, (logical_size.h - 1) as f64);
                let new_pos = smithay::utils::Point::from((x, y));

                if self.pan_dragging && (delta.x != 0.0 || delta.y != 0.0) {
                    self.pan_screen(delta.x, delta.y);
                }
                self.pointer_pos = new_pos;

                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let ptr = self.pointer.clone();
                ptr.motion(
                    self,
                    None,
                    &MotionEvent {
                        location: new_pos,
                        serial,
                        time,
                    },
                );
                ptr.relative_motion(
                    self,
                    None,
                    &RelativeMotionEvent {
                        delta,
                        delta_unaccel: event.delta_unaccel(),
                        utime: event.time(),
                    },
                );
                ptr.frame(self);
            }
            InputEvent::PointerButton { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let button = event.button_code();
                let state = match event.state() {
                    ButtonState::Pressed => smithay::backend::input::ButtonState::Pressed,
                    ButtonState::Released => smithay::backend::input::ButtonState::Released,
                };

                const BTN_MIDDLE: u32 = 0x112;
                if button == BTN_MIDDLE {
                    self.pan_dragging =
                        matches!(state, smithay::backend::input::ButtonState::Pressed);
                }

                let ptr = self.pointer.clone();
                ptr.button(
                    self,
                    &ButtonEvent {
                        button,
                        state,
                        serial,
                        time,
                    },
                );
                ptr.frame(self);
            }
            InputEvent::PointerAxis { event } => {
                use smithay::backend::input::Axis;
                use smithay::backend::input::PointerAxisEvent;
                if let Some(amount) = event.amount(Axis::Vertical) {
                    if amount < 0.0 {
                        self.zoom_at_cursor(1.1);
                    } else if amount > 0.0 {
                        self.zoom_at_cursor(0.9);
                    }
                }
            }
            _ => {
                // Touch, Tablet, GestureSwipe* — m-6+ work.
            }
        }
    }

    /// Backwards-compatible alias for the winit path. Kept so the
    /// winit main-loop callsite doesn't need to thread the type
    /// parameter explicitly.
    pub fn process_winit_input(
        &mut self,
        event: smithay::backend::input::InputEvent<smithay::backend::winit::WinitInput>,
    ) {
        self.process_input_event(event);
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
