//! DRM/KMS backend — bare-metal compositor path.
//!
//! Phase 2 m-6 lands this module incrementally:
//!   m-6.3: scaffold — `init()` returned a not-yet-implemented error
//!          so the type lived in main's backend selection.
//!   m-6.4: libseat session init — opens a seat, gets DRM master and
//!          input device permissions from logind/seatd. (this commit)
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
//! of a small DRM compositor.

pub mod session;

use thiserror::Error;

use self::session::{CompSession, SessionError};

#[derive(Debug, Error)]
pub enum DrmBackendError {
    #[error("libseat session: {0}")]
    Session(#[from] SessionError),
    #[error("DRM device + renderer bring-up not yet implemented (m-6.5)")]
    DeviceNotImplemented,
}

/// Owner type for the DRM backend. Each chunk fills in more fields:
///   m-6.4: `session` (this commit)
///   m-6.5: + `gpu`, `gbm`, `egl`, `renderer`
///   m-6.6: + `outputs: HashMap<crtc::Handle, OutputData>`
///   m-6.8: + `libinput`
pub struct DrmBackend {
    /// libseat session — owns DRM master and input device opens.
    /// The notifier inside is `Some` until m-6.7 moves it into the
    /// calloop event loop.
    pub session: CompSession,
}

impl DrmBackend {
    /// Initialise the DRM backend. m-6.4 stops after the session
    /// opens and returns `DeviceNotImplemented` so end-to-end
    /// dispatch from `main.rs` is still verifiable on a real TTY:
    /// you should see "libseat session opened, seat=seat0" in the
    /// trace before the compositor exits.
    pub fn init() -> Result<Self, DrmBackendError> {
        let session = CompSession::open()?;
        // m-6.5 will continue here: open /dev/dri/card0 via
        // session.open, build the DrmDevice, GbmAllocator,
        // EGLDisplay, EGLContext, GlesRenderer.
        let _ = session;
        Err(DrmBackendError::DeviceNotImplemented)
    }
}
