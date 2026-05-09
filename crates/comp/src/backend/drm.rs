//! DRM/KMS backend — bare-metal compositor path.
//!
//! Phase 2 m-6 lands this module incrementally:
//!   m-6.3: this scaffold — `init()` returns a not-yet-implemented
//!          error so the type lives in main's backend selection
//!          even though no real DRM work happens yet.
//!   m-6.4: libseat session init — opens a seat, gets DRM master
//!          and input device permissions from logind.
//!   m-6.5: DRM device + EGL + GlesRenderer — opens /dev/dri/card0,
//!          sets up GBM allocator, builds an EGLDisplay against it,
//!          binds a GlesRenderer.
//!   m-6.6: Output + DrmCompositor — enumerates connectors, picks a
//!          mode, wraps the DrmSurface in a DrmCompositor for
//!          double-buffer + page-flip handling.
//!   m-6.7: page-flip event source on the calloop loop.
//!   m-6.8: libinput device feed → Seat infrastructure already in
//!          WaylandState.
//!   m-6.9: render loop closes the deal — first frame on framebuffer.
//!
//! Reference: smithay/anvil/src/udev.rs is the canonical example
//! of a small DRM compositor. Read it top-to-bottom (rare exception
//! to the "grep don't read" rule).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DrmBackendError {
    #[error("DRM backend not yet implemented (m-6 in progress)")]
    NotImplemented,
}

/// Placeholder owner type for the DRM backend. Future fields will be:
///   * `session: smithay::backend::session::libseat::LibSeatSession`
///   * `primary_gpu: smithay::backend::drm::DrmDevice`
///   * `gbm: smithay::backend::allocator::gbm::GbmAllocator<...>`
///   * `egl: smithay::backend::egl::display::EGLDisplay`
///   * `renderer: smithay::backend::renderer::gles::GlesRenderer`
///   * `outputs: HashMap<crtc::Handle, OutputData>`
///   * `libinput: smithay::backend::libinput::LibinputInputBackend`
pub struct DrmBackend;

impl DrmBackend {
    /// Initialise the DRM backend. m-6.3 returns NotImplemented; the
    /// real implementation lands in m-6.4 onwards.
    pub fn init() -> Result<Self, DrmBackendError> {
        Err(DrmBackendError::NotImplemented)
    }
}
