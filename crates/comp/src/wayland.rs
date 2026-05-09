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

use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::desktop::{Space, Window};
use smithay::input::keyboard::{KeyboardHandle, XkbConfig};
use smithay::input::pointer::PointerHandle;
use smithay::input::{Seat, SeatState};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::reexports::wayland_server::backend::ClientData;
use smithay::utils::Transform;
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
        }
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

    /// Forward a winit input event into the seat's pointer / keyboard.
    /// Pointer position is read out of `pointer.current_location()`
    /// (smithay tracks it inside the handle); absolute motions are
    /// translated through the current output mode.
    ///
    /// This is the m-4 chunk 4 entry point: real input flows through
    /// the wayland protocol so weston-terminal sees keystrokes and
    /// mouse motion.
    pub fn process_winit_input(
        &mut self,
        event: smithay::backend::input::InputEvent<smithay::backend::winit::WinitInput>,
    ) {
        use smithay::backend::input::{
            AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent,
            PointerButtonEvent,
        };
        use smithay::input::keyboard::FilterResult;
        use smithay::input::pointer::{ButtonEvent, MotionEvent};
        use smithay::utils::SERIAL_COUNTER;

        match event {
            InputEvent::Keyboard { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let key_code = event.key_code();
                let key_state = event.state();
                // Forward without filtering — the surface that has
                // keyboard focus receives the keystroke. m-5 may
                // intercept canvas-control keystrokes (zoom, pan)
                // here before forwarding.
                let kbd = self.keyboard.clone();
                kbd.input::<(), _>(self, key_code, key_state, serial, time, |_, _, _| {
                    FilterResult::Forward
                });
                let _ = key_state;
                let _ = KeyState::Pressed;
            }
            InputEvent::PointerMotionAbsolute { event } => {
                // winit gives us absolute coordinates normalized to
                // the host window. Map them into output-logical
                // coordinates by transforming with the current mode.
                let output_size = match self.output.current_mode() {
                    Some(mode) => mode.size,
                    None => return,
                };
                // Output is at world (0, 0) on the space (chunk 2);
                // logical position equals output-relative position.
                // Scale=1 so physical→logical is a direct cast.
                let logical_size = output_size.to_logical(1);
                let pos = event.position_transformed(logical_size);
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
            InputEvent::PointerButton { event } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let button = event.button_code();
                let state = match event.state() {
                    ButtonState::Pressed => smithay::backend::input::ButtonState::Pressed,
                    ButtonState::Released => smithay::backend::input::ButtonState::Released,
                };
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
            _ => {
                // PointerMotion (relative), PointerAxis, Touch, Tablet,
                // GestureSwipe* — handled in m-5 (gestures) and m-6+
                // (touch / tablet).
            }
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
