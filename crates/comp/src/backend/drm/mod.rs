//! DRM/KMS backend — bare-metal compositor path.
//!
//! Phase 2 m-6 lands this module incrementally:
//!   m-6.3: scaffold — `init()` returned a not-yet-implemented error
//!          so the type lived in main's backend selection.
//!   m-6.4: libseat session init — opens a seat, gets DRM master and
//!          input device permissions from logind/seatd.
//!   m-6.5: DRM device + EGL + GlesRenderer — opens /dev/dri/card0,
//!          sets up GBM allocator, builds an EGLDisplay against it,
//!          binds a GlesRenderer. (this commit)
//!   m-6.6: Output + DrmCompositor — enumerates connectors, picks a
//!          mode, wraps the DrmSurface in a DrmCompositor for
//!          double-buffer + page-flip handling.
//!   m-6.7: page-flip event source on the calloop loop.
//!   m-6.8: libinput device feed → Seat infrastructure already in
//!          WaylandState.
//!   m-6.9: render loop closes the deal — first frame on framebuffer.
//!
//! Reference: smithay/anvil/src/udev.rs is the canonical example
//! of a small DRM compositor.

pub mod device;
pub mod session;

use thiserror::Error;

use self::device::{DeviceError, DrmRenderDevice};
use self::session::{CompSession, SessionError};

#[derive(Debug, Error)]
pub enum DrmBackendError {
    #[error("libseat session: {0}")]
    Session(#[from] SessionError),
    #[error("DRM device: {0}")]
    Device(#[from] DeviceError),
    #[error("output enumeration not yet implemented (m-6.6)")]
    OutputNotImplemented,
}

/// Owner type for the DRM backend. Each chunk fills in more fields:
///   m-6.4: `session`
///   m-6.5: + `device` (this commit)
///   m-6.6: + `outputs: HashMap<crtc::Handle, OutputData>`
///   m-6.8: + `libinput`
pub struct DrmBackend {
    /// libseat session — owns DRM master and input device opens.
    /// The notifier inside is `Some` until m-6.7 moves it into the
    /// calloop event loop.
    pub session: CompSession,
    /// KMS device + GBM allocator + EGL display + GLES renderer. The
    /// device's `drm_notifier` is `Some` until m-6.7 inserts it into
    /// the calloop event loop for page-flip / vblank events.
    pub device: DrmRenderDevice,
}

impl DrmBackend {
    /// Initialise the DRM backend. m-6.5 brings up the renderer
    /// stack but stops before output enumeration, returning
    /// `OutputNotImplemented` so the dispatch is end-to-end
    /// testable on a real TTY: trace should show both
    /// "libseat session opened" and "DRM device opened, GBM
    /// allocator + EGL display + GLES renderer ready" before
    /// `helios-comp` exits with the m-6.6 stub error.
    pub fn init() -> Result<Self, DrmBackendError> {
        let mut session = CompSession::open()?;
        let device = DrmRenderDevice::open(&mut session)?;
        // m-6.6 will continue here: scan_connectors, pick a mode,
        // build a DrmCompositor for that connector + crtc.
        let _backend = Self { session, device };
        Err(DrmBackendError::OutputNotImplemented)
    }
}
