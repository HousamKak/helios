//! heliOS compositor — library surface.
//!
//! Phase 2 month-1 (`canvas.rs` / `state.rs` / `render.rs`) locked the
//! canvas math and the architectural seams. Phase 2 month-2 added a
//! real smithay Wayland integration. Phase 2 month-3 (current shape)
//! advertises the `wl_compositor` global; clients can now bind it and
//! create surfaces.
//!
//! Per `docs/adr/0003-phase-2-compositor-scope.md` and
//! `PLAN.md` §6 Phase 2.

pub mod canvas;
pub mod render;
pub mod state;

#[cfg(target_os = "linux")]
pub mod handlers;

#[cfg(target_os = "linux")]
pub mod wayland;

pub use canvas::{CanvasTransform, EntityPlacement, Viewport, WorldPoint};
pub use render::{RenderItem, RenderItemKind, RenderPlan};
pub use state::HeliosState;

#[cfg(target_os = "linux")]
pub use wayland::{ClientState, WaylandState};
