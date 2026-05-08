//! heliOS compat layer — Phase 0 stub.
//!
//! Per `PLAN.md` §6, Phase 4:
//!   * XWayland integration in the compositor (the compositor crate
//!     handles the protocol; this crate handles the *entity registration*
//!     side: when a Chrome window appears on canvas, this is where the
//!     row in `canvas_entities` is built).
//!   * Flatpak provisioning — a curated set of legacy apps preinstalled,
//!     each registered as an `applet` row with `source = installed` even
//!     though they are not WASM. The compositor draws them as if they were
//!     applets but renders the underlying surface texture.
//!   * Decoration policy: server-side decorations always — the canvas
//!     draws the entity frame, hover affordances, title.

pub fn placeholder() -> &'static str {
    "helios-compat: phase-0 stub"
}
