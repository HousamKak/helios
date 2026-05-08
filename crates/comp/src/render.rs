//! Render plan — what the compositor intends to draw this frame.
//!
//! Phase 2 month-1: a typed description of the frame, no GL calls
//! yet. The plan is built from `HeliosState.placements` filtered by
//! the viewport's visible region. Month-2 wires this to a smithay
//! `GlesRenderer` that submits actual draw commands.
//!
//! Keeping the plan as data lets us:
//!   * unit-test placement-to-render-item conversion without a GL
//!     context (the tests in this module);
//!   * reorder, batch, and cull at the data layer before any GPU
//!     work happens;
//!   * snapshot a frame for replay debugging by serializing the plan.

use serde::{Deserialize, Serialize};

use helios_schema::EntityId;

use crate::canvas::{EntityPlacement, Viewport};
use crate::state::HeliosState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderPlan {
    pub viewport: Viewport,
    pub items: Vec<RenderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderItem {
    pub entity_id: EntityId,
    pub placement: EntityPlacement,
    pub kind: RenderItemKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderItemKind {
    /// Solid-colour rectangle — placeholder for an entity whose
    /// underlying surface hasn't connected yet (process just exec'd,
    /// applet still loading, etc.).
    Placeholder,

    /// A `wl_surface` texture from a connected client. Actual surface
    /// compositing lands in Phase 2 month 3+ once xdg-shell handlers
    /// are wired.
    SurfaceTexture,

    /// A WIT-applet UI tree rendered through the compositor's host
    /// renderer. Phase 3 work; declared now so the kind enum is closed.
    AppletTree,

    /// An applet that opted into a wgpu rich tier (own GL/Vulkan
    /// surface). Phase 3 escape hatch for video/3D applets.
    RichApplet,
}

impl RenderPlan {
    /// Build a render plan from current state. Filters out invisible
    /// placements; keeps everything else for now (real culling lands
    /// when entity counts get large).
    pub fn build(state: &HeliosState) -> Self {
        let mut items: Vec<RenderItem> = state
            .placements
            .iter()
            .filter(|(_, p)| p.visible)
            .map(|(id, placement)| RenderItem {
                entity_id: id.clone(),
                placement: *placement,
                kind: RenderItemKind::Placeholder,
            })
            .collect();

        items.sort_by_key(|item| item.placement.z);

        Self {
            viewport: state.viewport,
            items,
        }
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// Custom serde for EntityPlacement so RenderPlan is fully serializable
// (RenderPlan implements Serialize/Deserialize for replay-debugging).
impl Serialize for EntityPlacement {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("EntityPlacement", 5)?;
        s.serialize_field("world_pos", &self.world_pos)?;
        s.serialize_field("world_scale", &self.world_scale)?;
        s.serialize_field("world_size", &self.world_size)?;
        s.serialize_field("z", &self.z)?;
        s.serialize_field("visible", &self.visible)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for EntityPlacement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            world_pos: crate::canvas::WorldPoint,
            world_scale: f64,
            world_size: Option<(f64, f64)>,
            z: i32,
            visible: bool,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(EntityPlacement {
            world_pos: h.world_pos,
            world_scale: h.world_scale,
            world_size: h.world_size,
            z: h.z,
            visible: h.visible,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::WorldPoint;
    use helios_schema::generate_id;

    fn placed(z: i32, visible: bool) -> EntityPlacement {
        EntityPlacement {
            world_pos: WorldPoint { x: 0.0, y: 0.0 },
            world_scale: 1.0,
            world_size: None,
            z,
            visible,
        }
    }

    #[test]
    fn build_filters_invisible() {
        let mut state = HeliosState::new();
        state.placements.insert(generate_id(), placed(0, true));
        state.placements.insert(generate_id(), placed(0, false));
        state.placements.insert(generate_id(), placed(0, true));
        let plan = RenderPlan::build(&state);
        assert_eq!(plan.item_count(), 2);
    }

    #[test]
    fn build_sorts_by_z() {
        let mut state = HeliosState::new();
        for z in [5, 1, 3, 0, 4] {
            state.placements.insert(generate_id(), placed(z, true));
        }
        let plan = RenderPlan::build(&state);
        let zs: Vec<i32> = plan.items.iter().map(|i| i.placement.z).collect();
        assert_eq!(zs, vec![0, 1, 3, 4, 5]);
    }

    #[test]
    fn build_with_no_placements_emits_empty_plan() {
        let state = HeliosState::new();
        let plan = RenderPlan::build(&state);
        assert_eq!(plan.item_count(), 0);
    }
}
