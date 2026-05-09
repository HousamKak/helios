//! Canvas-world coordinates and 2D affine transforms.
//!
//! These are the compositor-internal types every entity is positioned
//! in. The compositor renders by mapping world coords to screen coords
//! through the active `Viewport`'s transform. Pan and zoom mutate the
//! viewport; everything else is derived.
//!
//! Distinct from `helios_schema::CanvasEntity` (a persisted row in the
//! entity store): the compositor *reads* CanvasEntity rows and builds
//! an `EntityPlacement` per row each frame.

use serde::{Deserialize, Serialize};

/// One position in canvas-world coordinates. `(0, 0)` is the origin
/// of the currently-active desktop. Coordinates are unbounded — pan
/// can take you anywhere, zoom can take you to any scale.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WorldPoint {
    pub x: f64,
    pub y: f64,
}

impl WorldPoint {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };
}

/// A 2D affine transform — translate, then rotate, then uniform scale.
/// Stored as four scalars (not a 4×4 matrix) because every compositor
/// transform we apply is 2D affine; the verbose form just costs cache
/// lines without telling the GPU anything more.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CanvasTransform {
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale: f64,
    pub rotation: f64,
}

impl CanvasTransform {
    pub const IDENTITY: Self = Self {
        translate_x: 0.0,
        translate_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    };

    /// World point → screen point.
    pub fn transform_point(&self, p: WorldPoint) -> WorldPoint {
        let (sin, cos) = self.rotation.sin_cos();
        let scaled_x = p.x * self.scale;
        let scaled_y = p.y * self.scale;
        WorldPoint {
            x: scaled_x * cos - scaled_y * sin + self.translate_x,
            y: scaled_x * sin + scaled_y * cos + self.translate_y,
        }
    }

    /// Screen point → world point. Critical for input: cursor lands at
    /// pixel `(sx, sy)`; which entity is under it? Hit-test in world.
    pub fn invert_point(&self, p: WorldPoint) -> WorldPoint {
        let (sin, cos) = (-self.rotation).sin_cos();
        let dx = p.x - self.translate_x;
        let dy = p.y - self.translate_y;
        let unrotated_x = dx * cos - dy * sin;
        let unrotated_y = dx * sin + dy * cos;
        WorldPoint {
            x: unrotated_x / self.scale,
            y: unrotated_y / self.scale,
        }
    }
}

impl Default for CanvasTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// What's currently visible on screen. Pan/zoom updates this; the
/// renderer derives its world→screen transform from `world_to_screen_transform`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Viewport {
    /// World point currently centred on screen.
    pub center: WorldPoint,
    /// 1.0 = native scale; > 1.0 = zoomed in (entities bigger);
    /// < 1.0 = zoomed out.
    pub zoom: f64,
    pub screen_width: u32,
    pub screen_height: u32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            center: WorldPoint::ORIGIN,
            zoom: 1.0,
            screen_width: 1920,
            screen_height: 1080,
        }
    }
}

impl Viewport {
    /// Build the world → screen transform for this viewport. The
    /// world point at `self.center` lands at screen-centre; world
    /// units scale by `self.zoom` to become screen pixels.
    pub fn world_to_screen_transform(&self) -> CanvasTransform {
        CanvasTransform {
            translate_x: self.screen_width as f64 / 2.0 - self.center.x * self.zoom,
            translate_y: self.screen_height as f64 / 2.0 - self.center.y * self.zoom,
            scale: self.zoom,
            rotation: 0.0,
        }
    }

    /// Pan the viewport by a delta in screen pixels (e.g. trackpad two-
    /// finger drag). Screen movement maps to inverse world movement at
    /// the current zoom.
    pub fn pan_by_screen_pixels(&mut self, dx: f64, dy: f64) {
        self.center.x -= dx / self.zoom;
        self.center.y -= dy / self.zoom;
    }

    /// Zoom around a screen-anchor point (typically the cursor) so
    /// the world point under that anchor stays put. Multiplier > 1.0
    /// zooms in, < 1.0 zooms out.
    ///
    /// Math: after the zoom change, the world point P that *was* at
    /// `anchor_screen` now lands at `new_screen`. The required
    /// correction to the centre is `(new_screen - anchor_screen) /
    /// zoom_new`. Since `pan_by_screen_pixels(dx)` *subtracts*
    /// `dx / zoom` from the centre (a "drag-the-paper" convention),
    /// we pass `(anchor - new)` to add the positive correction.
    pub fn zoom_around(&mut self, anchor_screen: WorldPoint, multiplier: f64) {
        let world_under_anchor = self
            .world_to_screen_transform()
            .invert_point(anchor_screen);
        self.zoom *= multiplier;
        self.zoom = self.zoom.clamp(0.05, 64.0);
        let new_screen = self
            .world_to_screen_transform()
            .transform_point(world_under_anchor);
        let dx = anchor_screen.x - new_screen.x;
        let dy = anchor_screen.y - new_screen.y;
        self.pan_by_screen_pixels(dx, dy);
    }
}

/// One entity's per-frame placement on the canvas. Built from a
/// `helios_schema::CanvasEntity` row each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityPlacement {
    pub world_pos: WorldPoint,
    pub world_scale: f64,
    pub world_size: Option<(f64, f64)>,
    pub z: i32,
    pub visible: bool,
}

impl EntityPlacement {
    /// Build from a CanvasEntity row from the store.
    pub fn from_row(row: &helios_schema::CanvasEntity) -> Self {
        Self {
            world_pos: WorldPoint { x: row.x, y: row.y },
            world_scale: row.scale,
            world_size: row.width.zip(row.height),
            z: row.z,
            visible: row.visible,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_points_through() {
        let t = CanvasTransform::IDENTITY;
        let p = WorldPoint { x: 10.0, y: 20.0 };
        let r = t.transform_point(p);
        assert!((r.x - p.x).abs() < 1e-9);
        assert!((r.y - p.y).abs() < 1e-9);
    }

    #[test]
    fn invert_round_trips() {
        let t = CanvasTransform {
            translate_x: 5.0,
            translate_y: -3.0,
            scale: 2.0,
            rotation: 0.5,
        };
        let p = WorldPoint { x: 7.0, y: 11.0 };
        let r = t.transform_point(p);
        let back = t.invert_point(r);
        assert!((back.x - p.x).abs() < 1e-6, "x: {} vs {}", back.x, p.x);
        assert!((back.y - p.y).abs() < 1e-6, "y: {} vs {}", back.y, p.y);
    }

    #[test]
    fn viewport_centers_origin_at_screen_center() {
        let vp = Viewport::default();
        let t = vp.world_to_screen_transform();
        let origin_screen = t.transform_point(WorldPoint::ORIGIN);
        assert_eq!(origin_screen.x, 960.0);
        assert_eq!(origin_screen.y, 540.0);
    }

    #[test]
    fn pan_moves_world_under_screen() {
        let mut vp = Viewport::default();
        vp.pan_by_screen_pixels(100.0, 0.0);
        assert_eq!(vp.center.x, -100.0);
    }

    #[test]
    fn zoom_around_keeps_anchor_stable() {
        let mut vp = Viewport::default();
        let anchor = WorldPoint { x: 800.0, y: 400.0 };
        let world_under_anchor_before = vp.world_to_screen_transform().invert_point(anchor);
        vp.zoom_around(anchor, 2.0);
        let world_under_anchor_after = vp.world_to_screen_transform().invert_point(anchor);
        assert!((world_under_anchor_before.x - world_under_anchor_after.x).abs() < 1e-3);
        assert!((world_under_anchor_before.y - world_under_anchor_after.y).abs() < 1e-3);
        assert_eq!(vp.zoom, 2.0);
    }

    #[test]
    fn zoom_clamps_to_sane_range() {
        let mut vp = Viewport::default();
        for _ in 0..1000 {
            vp.zoom_around(WorldPoint::ORIGIN, 2.0);
        }
        assert!(vp.zoom <= 64.0);
        for _ in 0..1000 {
            vp.zoom_around(WorldPoint::ORIGIN, 0.5);
        }
        assert!(vp.zoom >= 0.05);
    }
}
