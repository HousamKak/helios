//! DRM device + GBM allocator + EGL display + GLES renderer.
//!
//! Phase 2 m-6 chunk 5. Walks the chain that turns a `/dev/dri/card*`
//! node into a renderer the compositor can submit frames through:
//!
//! ```text
//! libseat → OwnedFd → DeviceFd → DrmDeviceFd
//!                                       │
//!                                       ├── DrmDevice (KMS / page-flip)
//!                                       └── GbmDevice
//!                                               │
//!                                               ├── GbmAllocator (scanout)
//!                                               └── EGLDisplay
//!                                                      └── EGLContext
//!                                                              └── GlesRenderer
//! ```
//!
//! The fd is shared by `Arc` clones at every layer; libseat owns the
//! "real" fd and revokes it on TTY switch. Each layer keeps it alive
//! while it has rendering / scanout work to do.
//!
//! v0.1 picks the first DRM-capable card (`/dev/dri/card0`). Multi-GPU
//! enumeration is deferred to m-9 per ADR 0004.
//!
//! Reference: smithay/anvil/src/udev.rs — search for `DrmDeviceFd::new`,
//! `GbmAllocator::new`, `EGLDisplay::new`.

use std::path::{Path, PathBuf};

use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmDeviceNotifier};
use smithay::backend::egl::context::EGLContext;
use smithay::backend::egl::display::EGLDisplay;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::Session;
use smithay::reexports::rustix::fs::OFlags;
use smithay::utils::DeviceFd;
use thiserror::Error;

use super::session::CompSession;

/// Default node when nothing else is specified. Multi-GPU + udev
/// enumeration is m-9.
pub const DEFAULT_DRM_NODE: &str = "/dev/dri/card0";

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("session.open({path}) failed: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("DrmDevice::new failed: {0}")]
    Drm(#[from] smithay::backend::drm::DrmError),
    #[error("GbmDevice::new failed: {0}")]
    Gbm(std::io::Error),
    #[error("EGLDisplay::new failed: {0}")]
    EglDisplay(smithay::backend::egl::Error),
    #[error("EGLContext::new failed: {0}")]
    EglContext(smithay::backend::egl::Error),
    #[error("GlesRenderer::new failed: {0}")]
    Gles(smithay::backend::renderer::gles::GlesError),
}

/// Fully-bring-up'd render device — DRM master held, EGL/GLES ready.
/// Each chunk attaches more behaviour:
///   m-6.5 (this commit): the device + a renderer that can clear-color.
///   m-6.6: per-connector DrmCompositor wrapped around DrmSurface.
///   m-6.7: the DrmDeviceNotifier inserted into calloop for page flips.
pub struct DrmRenderDevice {
    /// KMS device — used for `scan_connectors`, `create_surface`,
    /// page-flip notifications. Cheap to clone (Arc internally).
    pub drm: DrmDevice,
    /// Calloop event source for page-flip / vblank events. Held until
    /// m-6.7 inserts it into the loop.
    pub drm_notifier: Option<DrmDeviceNotifier>,
    /// Allocator for KMS scanout buffers (m-6.6 uses it).
    pub gbm_allocator: GbmAllocator<DrmDeviceFd>,
    /// Cloned GBM device retained for buffer import paths (dmabuf
    /// import is m-10; held now so device.rs is the single owner).
    pub gbm: GbmDevice<DrmDeviceFd>,
    /// EGL display backed by the GBM device. EGLContext / GlesRenderer
    /// share this display. Holding the display keeps the EGL platform
    /// alive across context destructions.
    pub egl_display: EGLDisplay,
    /// GLES renderer used to draw the output. Owned by the backend so
    /// the same renderer is reused across frames (cheap to render
    /// against, expensive to construct).
    pub renderer: GlesRenderer,
    /// Path the underlying device was opened at — purely for logs.
    pub node_path: PathBuf,
}

impl DrmRenderDevice {
    /// Open the default DRM node and bring up the full renderer
    /// stack on top of it. Allows callers to override the path via
    /// `HELIOS_COMP_DRM_NODE` env var (defaults to /dev/dri/card0).
    pub fn open(session: &mut CompSession) -> Result<Self, DeviceError> {
        let node_path: PathBuf = std::env::var_os("HELIOS_COMP_DRM_NODE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DRM_NODE));
        Self::open_at(session, &node_path)
    }

    /// Open a specific DRM node. Used by tests and by `open()` once
    /// it has resolved the path.
    pub fn open_at(session: &mut CompSession, path: &Path) -> Result<Self, DeviceError> {
        // O_RDWR is required for KMS modeset; CLOEXEC keeps the fd
        // out of the children we exec via the future runner; NONBLOCK
        // is what calloop wants for the page-flip event source in
        // m-6.7; NOCTTY prevents the open from acquiring a controlling
        // tty by accident.
        let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
        let owned_fd = session
            .session
            .open(path, flags)
            .map_err(|err| DeviceError::Open {
                path: path.to_path_buf(),
                source: std::io::Error::other(format!("{err:?}")),
            })?;
        let device_fd = DeviceFd::from(owned_fd);
        // DrmDeviceFd::new attempts to acquire DRM master. If that
        // fails we fall back to unprivileged mode with a warning —
        // acceptable on newer kernels that grant master implicitly.
        let drm_fd = DrmDeviceFd::new(device_fd);

        // false = leave the connectors enabled so we can scan them
        // in m-6.6. Anvil also passes false here.
        let (drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), false)?;

        // GbmDevice<DrmDeviceFd> — DrmDeviceFd is AsFd + Clone +
        // Send + 'static so EGLDisplay's GBM impl is satisfied.
        let gbm = GbmDevice::new(drm_fd).map_err(DeviceError::Gbm)?;
        let gbm_for_egl = gbm.clone();
        let gbm_allocator = GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING);

        // SAFETY: smithay's EGLDisplay::new is unsafe because it
        // tracks EGL display instances internally; we don't construct
        // EGLDisplay anywhere else, so the invariant holds.
        let egl_display =
            unsafe { EGLDisplay::new(gbm_for_egl) }.map_err(DeviceError::EglDisplay)?;

        let egl_context = EGLContext::new(&egl_display).map_err(DeviceError::EglContext)?;

        // SAFETY: GlesRenderer::new is unsafe because it issues GL
        // calls during construction; the EGLContext is freshly made
        // and bound to a real display, so it's valid.
        let renderer = unsafe { GlesRenderer::new(egl_context) }.map_err(DeviceError::Gles)?;

        tracing::info!(
            path = %path.display(),
            "DRM device opened, GBM allocator + EGL display + GLES renderer ready",
        );

        Ok(Self {
            drm,
            drm_notifier: Some(drm_notifier),
            gbm_allocator,
            gbm,
            egl_display,
            renderer,
            node_path: path.to_path_buf(),
        })
    }

    /// Take the DrmDeviceNotifier out for insertion into the calloop
    /// event loop (m-6.7).
    pub fn take_notifier(&mut self) -> Option<DrmDeviceNotifier> {
        self.drm_notifier.take()
    }
}
