//! X11 window manager — `XwmHandler` impl + lifecycle.
//!
//! Phase 2 m-7 chunk 3. Wires the WM-side of XWayland: when an X
//! client creates a top-level window, we wrap it in
//! `Window::new_x11_window` and map it onto `Space`. From the
//! renderer's perspective, X11 surfaces are indistinguishable from
//! native xdg_toplevels — both produce `wl_surface`s that the
//! commit pipeline imports into GLES textures.
//!
//! `XWaylandShellHandler` and `delegate_xwayland_shell!` are the
//! protocol-side glue: smithay's xwayland-shell-v1 wayland global
//! lets XWayland associate its X windows with their backing
//! `wl_surface`s.
//!
//! Override-redirect (popups, menus) tracking and decoration policy
//! land in chunks 7.5/7.6; chunk 7.3 logs them and otherwise
//! treats them like regular toplevels.
//!
//! Reference: smithay/anvil/src/xwayland.rs `XwmHandler` impl.

use smithay::desktop::Window;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::XwmId;
use smithay::xwayland::{X11Surface, X11Wm, XwmHandler};

use crate::WaylandState;

impl XWaylandShellHandler for WaylandState {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        // `expect` — main.rs constructs the global at startup
        // when the xwayland feature is enabled. If this fires, an
        // earlier guard is missing.
        self.xwayland_shell_state
            .as_mut()
            .expect("xwayland_shell_state not initialised")
    }
}

smithay::delegate_xwayland_shell!(WaylandState);

impl XwmHandler for WaylandState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        // Single XWayland instance in v0.1 — there's only ever one
        // X11Wm. Multi-instance support (one X server per project)
        // is a v0.2 idea per ADR 0004.
        self.xwm
            .as_mut()
            .expect("xwm accessed before X11Wm::start_wm completed")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // Window exists but isn't mapped yet. Most clients call
        // map_window_request right after this; some hide and show
        // multiple times across their lifecycle. Track the binding
        // here so the entity_id is stable from creation, not from
        // first map.
        let entity_id = helios_schema::generate_id();
        if let Some(wl_surface) = window.wl_surface() {
            self.surface_to_entity
                .insert(wl_surface.id(), entity_id.clone());
            self.entity_to_world
                .insert(entity_id.clone(), crate::WorldPoint::ORIGIN);
        }
        tracing::info!(%entity_id, pid = ?window.pid(), "x11: new toplevel window");
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // OR-windows are popups, menus, tooltips — m-7.6 tracks them
        // separately so pan/zoom doesn't move them. Until then, log
        // and leave them un-mapped (chunk 7.6's
        // mapped_override_redirect_window does the placement).
        tracing::debug!(pid = ?window.pid(), "x11: new override-redirect window");
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        // The client wants to be visible. Place it on the canvas at
        // the world origin (m-5 spawn policy). World→screen happens
        // via reapply_viewport_to_windows on the next commit.
        if let Err(err) = window.set_mapped(true) {
            tracing::warn!(?err, "x11: set_mapped(true) failed");
            return;
        }
        let screen_pos = self.world_to_screen(crate::WorldPoint::ORIGIN);
        let win = Window::new_x11_window(window);
        self.space.map_element(win, screen_pos, true);
        self.full_redraw = 4;
        tracing::info!(?screen_pos, "x11: toplevel mapped");
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // m-7.6 will track these in a separate Vec and render them
        // last, in screen space (not world space). For chunk 7.3 we
        // just place them on Space at their requested geometry so
        // they show up at all.
        let geo = window.geometry();
        let win = Window::new_x11_window(window);
        self.space.map_element(win, geo.loc, false);
        self.full_redraw = 4;
        tracing::debug!(?geo, "x11: override-redirect window mapped");
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        // Remove from Space if present. The window stays alive
        // (the X client may map it again) — only the visibility
        // changes. Keep the entity binding around for re-map.
        let target = self
            .space
            .elements()
            .find(|w| {
                matches!(
                    w.x11_surface(),
                    Some(s) if s == &window,
                )
            })
            .cloned();
        if let Some(w) = target {
            self.space.unmap_elem(&w);
            self.full_redraw = 4;
            tracing::info!("x11: window unmapped");
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(wl_surface) = window.wl_surface()
            && let Some(entity_id) = self.surface_to_entity.remove(&wl_surface.id())
        {
            self.entity_to_world.remove(&entity_id);
            tracing::info!(%entity_id, "x11: window destroyed; entity unbound");
        }
        // Space::refresh (called from CompositorHandler::commit)
        // prunes the dead window automatically.
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<smithay::xwayland::xwm::Reorder>,
    ) {
        // Honour the client's geometry request as-is. heliOS doesn't
        // do tiling — entity size is what the app asks for, and
        // window placement is canvas-driven (m-5/m-7) not WM-driven.
        let geo = window.geometry();
        let new_geo = Rectangle::<i32, Logical> {
            loc: smithay::utils::Point::from((x.unwrap_or(geo.loc.x), y.unwrap_or(geo.loc.y))),
            size: smithay::utils::Size::from((
                w.map(|v| v as i32).unwrap_or(geo.size.w),
                h.map(|v| v as i32).unwrap_or(geo.size.h),
            )),
        };
        if let Err(err) = window.configure(new_geo) {
            tracing::warn!(?err, "x11: configure failed");
        }
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: Rectangle<i32, Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        // Notification only — the client confirmed its new geometry.
        // Render path picks it up on the next frame via Space.
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: smithay::xwayland::xwm::ResizeEdge,
    ) {
        // heliOS canvas paradigm: window resize is a canvas-side
        // gesture (m-7+ canvas chrome), not an X-protocol grab.
        // Ignore; the client will fall back to its own resize logic.
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {
        // Same as resize_request — moves are canvas-driven.
    }
}
