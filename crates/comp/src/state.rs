//! Compositor state — owned by the event loop.
//!
//! `HeliosState` is the single struct the event loop holds. Smithay's
//! protocol handlers (`CompositorHandler`, `XdgShellHandler`, etc.)
//! will be implemented for it directly via smithay's `delegate_*!`
//! macros once the smithay deps are wired in.
//!
//! Phase 2 month-1 shape: only the canvas-specific fields exist.
//! Smithay protocol-handler state arrives in month 2 once
//! the wayland-server display is actually started.

use std::collections::HashMap;

use helios_schema::EntityId;

use crate::canvas::{EntityPlacement, Viewport};

pub struct HeliosState {
    /// What's visible on screen. Pan/zoom gestures mutate this.
    pub viewport: Viewport,

    /// Cached entity placements keyed by `CanvasEntity.id`. Refreshed
    /// when the events bus signals canvas changes (or when the store
    /// emits a snapshot delta).
    pub placements: HashMap<EntityId, EntityPlacement>,

    /// The desktop the viewport is currently centred on. Pan-between-
    /// desktops swaps this and animates the viewport translation.
    pub active_desktop_id: Option<EntityId>,

    // ---------------------------------------------------------------
    // Future fields, landing as smithay integration progresses:
    //
    // pub display_handle: wayland_server::DisplayHandle,
    // pub compositor_state: smithay::wayland::compositor::CompositorState,
    // pub xdg_shell_state: smithay::wayland::shell::xdg::XdgShellState,
    // pub shm_state: smithay::wayland::shm::ShmState,
    // pub seat_state: smithay::wayland::seat::SeatState<Self>,
    // pub output_state: smithay::wayland::output::OutputState,
    // pub data_device_state: smithay::wayland::selection::data_device::DataDeviceState,
    // pub space: smithay::desktop::Space<smithay::desktop::Window>,
    // pub renderer: smithay::backend::renderer::gles::GlesRenderer,
    // pub events_subscriber: tokio::sync::mpsc::Receiver<helios_schema::SystemEvent>,
    // pub store_client: helios_store::StoreClient,
    // pub xwayland: Option<smithay::xwayland::XWayland>,
}

impl HeliosState {
    pub fn new() -> Self {
        Self {
            viewport: Viewport::default(),
            placements: HashMap::new(),
            active_desktop_id: None,
        }
    }

    /// Replace the placement cache with a fresh snapshot from the store.
    /// Called when the compositor reads `canvas_entities` rows.
    pub fn set_placements_from_rows(&mut self, rows: &[helios_schema::CanvasEntity]) {
        self.placements.clear();
        for row in rows {
            self.placements
                .insert(row.id.clone(), EntityPlacement::from_row(row));
        }
    }

    pub fn placement_count(&self) -> usize {
        self.placements.len()
    }
}

impl Default for HeliosState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_schema::{CanvasEntity, EntityKind, generate_id, now};

    fn fake_row(kind: EntityKind, x: f64, y: f64) -> CanvasEntity {
        CanvasEntity {
            id: generate_id(),
            desktop_id: generate_id(),
            entity_kind: kind,
            entity_id: generate_id(),
            x,
            y,
            scale: 1.0,
            rotation: 0.0,
            z: 0,
            width: Some(200.0),
            height: Some(150.0),
            pinned: false,
            visible: true,
            relevance: 0.5,
            attached_applet_ids: vec![],
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn new_state_is_empty() {
        let s = HeliosState::new();
        assert_eq!(s.placement_count(), 0);
        assert!(s.active_desktop_id.is_none());
    }

    #[test]
    fn set_placements_replaces_cache() {
        let mut s = HeliosState::new();
        let rows = vec![
            fake_row(EntityKind::Process, 10.0, 20.0),
            fake_row(EntityKind::Process, 30.0, 40.0),
            fake_row(EntityKind::Applet, 50.0, 60.0),
        ];
        s.set_placements_from_rows(&rows);
        assert_eq!(s.placement_count(), 3);

        // Subsequent call replaces, not appends.
        let smaller = vec![fake_row(EntityKind::File, 0.0, 0.0)];
        s.set_placements_from_rows(&smaller);
        assert_eq!(s.placement_count(), 1);
    }
}
