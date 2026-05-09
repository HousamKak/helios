//! Winit + GlesRenderer backend.
//!
//! Phase 2 month-4 chunk 1: opens a winit window, sets up EGL, builds
//! a GlesRenderer. The render loop in `main.rs` drives this — we own
//! the backend and event pump; main owns the calloop loop and state.
//!
//! `winit` here is *backend_winit* — running heliOS-comp inside an
//! existing Wayland session. The bare-metal DRM/KMS backend (`drm.rs`)
//! lands in m-6.

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{Error as WinitError, WinitEventLoop, WinitGraphicsBackend};

/// Owns the winit-side state — the GLES-backed graphics backend and
/// the winit event pump. The pump is polled each main-loop iteration
/// for Resized/Input/CloseRequested events.
pub struct CompBackend {
    pub backend: WinitGraphicsBackend<GlesRenderer>,
    pub winit: WinitEventLoop,
}

impl CompBackend {
    /// Initialise winit + EGL + GlesRenderer. Fails if the host
    /// environment can't open a Wayland window (e.g. no display
    /// server, no GL drivers).
    pub fn init() -> Result<Self, WinitError> {
        let (backend, winit) = smithay::backend::winit::init::<GlesRenderer>()?;
        Ok(Self { backend, winit })
    }
}
