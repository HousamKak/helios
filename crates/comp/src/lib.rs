//! heliOS compositor — library surface.
//!
//! Phase 2 month-1 (`canvas.rs` / `state.rs` / `render.rs`) locked the
//! canvas math and the architectural seams. Phase 2 month-2 (this
//! crate's current shape) adds a real smithay Wayland integration in
//! `wayland.rs` and the `main.rs` binary that opens a Wayland socket.
//!
//! Per `docs/adr/0003-phase-2-compositor-scope.md` and
//! `PLAN.md` §6 Phase 2.

pub mod canvas;
pub mod render;
pub mod state;

#[cfg(target_os = "linux")]
pub mod wayland;

pub use canvas::{CanvasTransform, EntityPlacement, Viewport, WorldPoint};
pub use render::{RenderItem, RenderItemKind, RenderPlan};
pub use state::HeliosState;

#[cfg(target_os = "linux")]
pub use wayland::WaylandState;
