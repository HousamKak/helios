//! Compositor backends — winit (nested) and DRM (bare-metal).
//!
//! Phase 2 m-4/m-5 shipped only the winit backend. m-6 introduces
//! the DRM/KMS backend so heliOS-comp can render directly to a
//! framebuffer when no parent Wayland session is available (i.e.
//! booted on real hardware to a TTY).
//!
//! Backend selection lives in `main.rs` and dispatches to one of
//! these modules at startup. The two modules deliberately share no
//! types beyond what `WaylandState` and `CompBackend` already
//! expose; downstream code touches the backend only through narrow
//! traits.

pub mod winit;

#[cfg(target_os = "linux")]
pub mod drm;

pub use winit::CompBackend;
