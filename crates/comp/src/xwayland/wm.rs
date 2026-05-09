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
//! Override-redirect (popups, menus) tracking lands in chunk 7.6;
//! chunk 7.3 logs them and otherwise treats them like regular
//! toplevels.
//!
//! Decoration policy (m-7.5): heliOS draws all entity chrome itself
//! (canvas paradigm — every entity gets the same frame). The
//! standard X11 way to express "WM handles decorations" is for the
//! client to read its own `_MOTIF_WM_HINTS` atom and observe
//! `MWM_DECOR == 0`. Smithay 0.7 doesn't publicly expose
//! `set_motif_hints` from the WM side (only `is_decorated()` for
//! reading the client's preference), so we can't actively *tell*
//! clients "we'll handle decorations" — they decide based on their
//! own defaults. Until either smithay adds the setter or the m-8
//! canvas chrome lands, X clients that draw their own decorations
//! (Firefox, Electron apps) will look slightly inconsistent next
//! to xdg-shell ones. Captured in the m-7.7 quirks doc.
//!
//! Reference: smithay/anvil/src/xwayland.rs `XwmHandler` impl.

use smithay::desktop::Window;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{WmWindowProperty, XwmId};
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

    fn surface_associated(&mut self, _xwm: XwmId, wl_surface: WlSurface, surface: X11Surface) {
        // m-7.4: an X11 window has been associated with a backing
        // wl_surface. This is the moment the surface↔entity binding
        // becomes possible: prior to association the X11Surface
        // exists at the X11 layer but has no wl_surface key. Mint an
        // entity_id here (mirrors the m-5.7 xdg_toplevel path) so the
        // canvas treats both producers identically.
        //
        // OR-windows skip this binding — they're tracked in screen
        // space (m-7.6), not on the canvas, so no entity_id is needed.
        if surface.is_override_redirect() {
            tracing::debug!("x11: surface_associated (override-redirect; not bound to entity)");
            return;
        }
        let entity_id = helios_schema::generate_id();
        self.surface_to_entity
            .insert(wl_surface.id(), entity_id.clone());
        self.entity_to_world
            .insert(entity_id.clone(), crate::WorldPoint::ORIGIN);
        // m-2.5.3: an X11 toplevel can also be the default terminal
        // (e.g. xterm fallback when foot isn't installed).
        self.mark_default_terminal_if_first(&entity_id);
        // m-7.5: log the client's decoration preference. heliOS
        // doesn't draw chrome yet (m-8), and we can't actively set
        // `_MOTIF_WM_HINTS` to NoDecoration via the smithay 0.7
        // public API, so this is informational. Clients drawing
        // their own decoration (`is_decorated() == false` from the
        // smithay perspective: client-side false → client draws
        // chrome) will look inconsistent next to xdg-shell apps
        // until canvas chrome lands.
        let csd = surface.is_decorated();
        // m-8.3: announce on the events bus. X11Surface::pid()
        // returns the X client's PID directly — better source than
        // wayland credentials (which would point to the XWayland
        // process, not the actual app).
        self.emit_event(helios_schema::EventPayload::SurfaceMapped {
            surface_id: entity_id.clone(),
            client_pid: surface.pid().map(|p| p as i32),
            kind: "x11".to_string(),
        });
        tracing::info!(
            %entity_id,
            pid = ?surface.pid(),
            csd,
            "x11: surface_associated; entity bound (csd={})",
            csd,
        );
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
        // Window exists but isn't mapped yet. The wl_surface binding
        // happens later via the xwayland_shell_v1 protocol — see
        // `XWaylandShellHandler::surface_associated` for the actual
        // entity_id mint.
        tracing::info!(pid = ?window.pid(), "x11: new toplevel window");
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
        // m-7.6: OR-windows are popups, menus, tooltips. Two
        // architectural rules per ADR 0004:
        //   1. They render in screen-pixel space, not canvas-world
        //      space — pan/zoom must NOT move them.
        //   2. They z-order above everything else on the same screen.
        //
        // Implementation: place them on Space at the requested
        // screen-pixel `geo.loc` (the X client gives us screen
        // coords, not world coords). Then `raise_element` lifts
        // them to the top of Space's stacking order.
        // `reapply_viewport_to_windows` skips OR-popups via
        // `is_override_redirect()`, so subsequent viewport changes
        // leave them where the client put them.
        let geo = window.geometry();
        let win = Window::new_x11_window(window);
        self.space.map_element(win.clone(), geo.loc, false);
        self.space.raise_element(&win, true);
        self.full_redraw = 4;
        tracing::debug!(?geo, "x11: override-redirect window mapped (screen-fixed)");
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        // m-7.6: OR-windows can re-position themselves at any time
        // (think: menu following the cursor, tooltip moving).
        // Re-map to the new screen-pixel location so the next render
        // shows them in the right spot. Non-OR windows ignore
        // configure_notify here — their positions are canvas-driven
        // (m-5/m-7), not X-protocol-driven.
        if !window.is_override_redirect() {
            return;
        }
        let target = self
            .space
            .elements()
            .find(|w| matches!(w.x11_surface(), Some(s) if s == &window))
            .cloned();
        if let Some(w) = target {
            self.space.map_element(w.clone(), geometry.loc, false);
            self.space.raise_element(&w, true);
            self.full_redraw = 4;
        }
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
            // m-8.3: emit before dropping the binding so subscribers
            // can correlate the unmap with their projection of the
            // canvas state.
            self.emit_event(helios_schema::EventPayload::SurfaceUnmapped {
                surface_id: entity_id.clone(),
            });
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

    fn property_notify(&mut self, _xwm: XwmId, window: X11Surface, property: WmWindowProperty) {
        // m-7.5: surface client-side metadata changes for diagnostics
        // and so the m-8 canvas chrome can react to title / class
        // changes when it lands. WindowType is useful for routing
        // dialogs / utility windows differently from main windows.
        match property {
            WmWindowProperty::Title => {
                tracing::debug!(pid = ?window.pid(), "x11: title changed");
            }
            WmWindowProperty::Class => {
                tracing::debug!(pid = ?window.pid(), "x11: class changed");
            }
            WmWindowProperty::MotifHints => {
                tracing::debug!(
                    pid = ?window.pid(),
                    csd = window.is_decorated(),
                    "x11: motif hints changed",
                );
            }
            // Protocols, Hints, NormalHints, TransientFor, WindowType,
            // StartupId, Pid — surface for m-8 canvas chrome to
            // consume. No-op now.
            _ => {}
        }
    }
}
