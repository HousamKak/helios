//! Output enumeration + DrmCompositor wiring.
//!
//! Phase 2 m-6 chunk 6. Walks the DRM device's connectors, picks the
//! first one that's actually plugged into something, picks a mode
//! (preferred if marked, else first), allocates a CRTC for it, and
//! wraps the resulting `DrmSurface` in a `DrmCompositor`. The
//! `DrmCompositor` owns double-buffer + page-flip + per-plane damage
//! tracking; we just feed it render elements each vblank.
//!
//! v0.1 picks one output. Multi-monitor is m-9 per ADR 0004.
//!
//! Reference: smithay/anvil/src/udev.rs `connector_connected` /
//! `device_added` blocks.

use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::gbm::GbmAllocator;
use smithay::backend::drm::DrmDeviceFd;
use smithay::backend::drm::compositor::DrmCompositor;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::drm::control::{Device as _, ModeTypeFlags, connector, crtc};
use smithay::utils::Size;
use thiserror::Error;

use super::device::DrmRenderDevice;

/// Concrete `DrmCompositor` specialisation we use everywhere. Generic
/// parameters:
///   * A = `GbmAllocator<DrmDeviceFd>` — primary plane swapchain.
///   * F = `GbmFramebufferExporter<DrmDeviceFd>` — turns swapchain
///     buffers into KMS framebuffers.
///   * U = `()` — no per-frame user data (m-6.7 may revisit this for
///     page-flip correlation).
///   * G = `DrmDeviceFd` — fd used for cursor-plane buffer allocation.
pub type HeliosDrmCompositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("drm resource_handles failed: {0}")]
    Resources(std::io::Error),
    #[error("drm get_connector failed: {0}")]
    Connector(std::io::Error),
    #[error("drm get_encoder failed: {0}")]
    Encoder(std::io::Error),
    #[error("no connector is currently connected")]
    NoConnector,
    #[error("connector {0} has no usable crtc")]
    NoCrtc(String),
    #[error("connector {0} has no modes")]
    NoModes(String),
    #[error("create_surface failed: {0}")]
    Surface(#[from] smithay::backend::drm::DrmError),
    #[error("DrmCompositor::new failed: {0}")]
    Compositor(String),
}

/// Per-output bring-up bundle. Multi-monitor (m-9) keys a HashMap of
/// these by `crtc::Handle`; for v0.1 there's exactly one.
pub struct OutputData {
    /// Smithay-side output. Advertised to clients via
    /// `output.create_global::<WaylandState>(&dh)`. Holds the active
    /// mode and is the `OutputModeSource` for the compositor's
    /// damage tracker.
    pub output: Output,
    /// CRTC bound to the connector. Held for diagnostic logging
    /// (smithay's DrmCompositor owns the surface internally).
    pub crtc: crtc::Handle,
    /// Connector handle — same diagnostic role as `crtc`.
    pub connector: connector::Handle,
    /// The compositor itself. Render-frame calls go through here.
    pub compositor: HeliosDrmCompositor,
}

impl OutputData {
    /// Bring up the first connected non-cursor connector on this DRM
    /// device. m-6.6 picks just one; multi-monitor enumeration is m-9.
    pub fn first_connected(device: &mut DrmRenderDevice) -> Result<Self, OutputError> {
        let resources = device
            .drm
            .resource_handles()
            .map_err(OutputError::Resources)?;

        for connector_handle in resources.connectors() {
            // `force_probe = false` — let the kernel cache; a fresh
            // probe runs at modeset time. Newer kernels skip the
            // probe if the cache is fresh, so this is cheap.
            let info = device
                .drm
                .get_connector(*connector_handle, false)
                .map_err(OutputError::Connector)?;
            if info.state() != connector::State::Connected {
                tracing::debug!(connector = %info, "skipping disconnected connector");
                continue;
            }

            let connector_name = format!("{}", info);

            // Pick the preferred mode if the driver flagged one,
            // else fall back to the first mode the connector reports.
            // ADR 0004: "first connected connector wins, preferred
            // mode if available".
            let mode = info
                .modes()
                .iter()
                .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
                .or_else(|| info.modes().first())
                .copied()
                .ok_or_else(|| OutputError::NoModes(connector_name.clone()))?;

            // Walk the connector's encoders, looking for one whose
            // `possible_crtcs` mask names a CRTC we can use. We don't
            // try to be clever about claiming — only one connector is
            // active in v0.1, so any free crtc works.
            let mut chosen: Option<crtc::Handle> = None;
            for encoder_handle in info.encoders() {
                let encoder = device
                    .drm
                    .get_encoder(*encoder_handle)
                    .map_err(OutputError::Encoder)?;
                if let Some(c) = resources.filter_crtcs(encoder.possible_crtcs()).first() {
                    chosen = Some(*c);
                    break;
                }
            }
            let crtc = chosen.ok_or_else(|| OutputError::NoCrtc(connector_name.clone()))?;

            tracing::info!(
                connector = %connector_name,
                ?crtc,
                w = mode.size().0,
                h = mode.size().1,
                refresh = mode.vrefresh(),
                "DRM output: connector + mode + crtc selected",
            );

            let surface = device
                .drm
                .create_surface(crtc, mode, &[*connector_handle])?;

            // Build the smithay-side Output. The wl_output global is
            // created by the m-6.9 integration step (we only build the
            // typed Output here so the DrmCompositor has a
            // mode-source reference).
            let (mw, mh) = mode.size();
            let smithay_mode = OutputMode {
                size: Size::from((mw as i32, mh as i32)),
                refresh: (mode.vrefresh() as i32) * 1000,
            };
            let physical_size_mm = info.size().unwrap_or((0, 0));
            let output = Output::new(
                connector_name.clone(),
                PhysicalProperties {
                    size: Size::from((physical_size_mm.0 as i32, physical_size_mm.1 as i32)),
                    make: "heliOS".into(),
                    model: format!("{:?}", info.interface()),
                    subpixel: Subpixel::Unknown,
                },
            );
            output.set_preferred(smithay_mode);
            output.change_current_state(Some(smithay_mode), None, None, None);

            // Cursor plane allocation goes through the same GBM device
            // as the primary plane. The cursor_size query reports the
            // hardware cursor dimensions (commonly 64×64).
            let exporter = GbmFramebufferExporter::new(device.gbm.clone(), None);
            use smithay::backend::allocator::Fourcc as DrmFourcc;
            let color_formats = [DrmFourcc::Argb8888, DrmFourcc::Xrgb8888];
            // GlesRenderer reports the formats it can render to via
            // its EGL display. The intersection with the connector's
            // scanout-capable formats is computed inside DrmCompositor.
            let renderer_formats: FormatSet = device.egl_display.dmabuf_render_formats().clone();
            let cursor_size = device.drm.cursor_size();

            let compositor = DrmCompositor::new(
                &output,
                surface,
                None,
                device.gbm_allocator.clone(),
                exporter,
                color_formats,
                renderer_formats,
                cursor_size,
                Some(device.gbm.clone()),
            )
            .map_err(|e| OutputError::Compositor(format!("{e}")))?;

            return Ok(Self {
                output,
                crtc,
                connector: *connector_handle,
                compositor,
            });
        }

        Err(OutputError::NoConnector)
    }
}
