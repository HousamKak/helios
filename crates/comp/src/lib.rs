//! heliOS compositor — library surface.
//!
//! Phase 2 scaffold. The compositor will eventually be a smithay-based
//! Wayland server that renders the heliOS canvas: every entity in the
//! store becomes a world-positioned scene-graph node, rendered through
//! a viewport that pans + zooms with infinite resolution.
//!
//! This crate is structured for the work that's coming, not the work
//! that's done. Smithay protocol handlers, the GLES renderer, libinput,
//! XWayland — all of those land in `state.rs` and a future `smithay/`
//! submodule once the host environment can build them. For now we lock
//! in the canvas math and the architectural seams.
//!
//! Per `docs/adr/0003-phase-2-compositor-scope.md` and `PLAN.md` §6
//! Phase 2.

pub mod canvas;
pub mod render;
pub mod state;

pub use canvas::{CanvasTransform, EntityPlacement, Viewport, WorldPoint};
pub use render::{RenderItem, RenderItemKind, RenderPlan};
pub use state::HeliosState;
