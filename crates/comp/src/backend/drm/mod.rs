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
pub mod input;
pub mod output;
pub mod run;
pub mod session;

use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::renderer::Color32F;
use smithay::desktop::space::space_render_elements;
use thiserror::Error;

use self::device::{DeviceError, DrmRenderDevice};
use self::output::{OutputData, OutputError};
use self::session::{CompSession, SessionError};
use crate::WaylandState;

/// heliOS canvas background colour. Same constant as `run::CANVAS_COLOR`
/// — kept in this module so `tick` doesn't need a free-standing const.
const CANVAS_COLOR: [f32; 4] = [0.05, 0.06, 0.10, 1.0];

#[derive(Debug, Error)]
pub enum DrmBackendError {
    #[error("libseat session: {0}")]
    Session(#[from] SessionError),
    #[error("DRM device: {0}")]
    Device(#[from] DeviceError),
    #[error("DRM output: {0}")]
    Output(#[from] OutputError),
}

/// Owner type for the DRM backend. Each chunk fills in more fields:
///   m-6.4: `session`
///   m-6.5: + `device`
///   m-6.6: + `outputs: Vec<OutputData>` (this commit)
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
    /// Per-output bring-up bundles. v0.1 only ever populates one
    /// element here; multi-monitor (m-9) keys this off `crtc::Handle`.
    pub outputs: Vec<OutputData>,
}

impl DrmBackend {
    /// Initialise the DRM backend up to the point of being ready to
    /// run. The caller picks up from here by handing the backend to
    /// `run::run` along with a wayland Display + WaylandState.
    pub fn init() -> Result<Self, DrmBackendError> {
        let mut session = CompSession::open()?;
        let mut device = DrmRenderDevice::open(&mut session)?;
        let primary_output = OutputData::first_connected(&mut device)?;
        Ok(Self {
            session,
            device,
            outputs: vec![primary_output],
        })
    }

    /// Render and queue one frame for the primary output. Called
    /// once at startup and on every page-flip / vblank. m-6.9 entry
    /// point — replaces the chunk-7 single-frame paint with a
    /// continuous render-on-vblank loop driven by `WaylandState::space`.
    ///
    /// Sequence per call:
    ///   1. `frame_submitted()` — release the slot the kernel just
    ///      flipped to, so the next render has a buffer to write into.
    ///   2. `space_render_elements` — derive RenderElements from every
    ///      `Window` mapped on the space. Empty space → empty Vec →
    ///      DrmCompositor renders just the canvas-clear colour.
    ///   3. `render_frame` — composite into the primary plane swapchain.
    ///   4. `queue_frame` — submit if there's actual damage; skip
    ///      otherwise so we don't burn GPU on no-op flips.
    pub fn tick(&mut self, state: &mut WaylandState) -> anyhow::Result<()> {
        let Some(output_data) = self.outputs.first_mut() else {
            return Ok(());
        };

        // Step 1: kernel finished flipping to the previous frame, so
        // the swapchain slot it owned is free again. Idempotent on
        // the first call (no prior frame, returns Ok(None)).
        let _ = output_data.compositor.frame_submitted();

        if !self.device.drm.is_active() {
            // Session is paused (TTY switched away). Don't even try
            // to render — we'd hit DeviceInactive errors. The
            // ActivateSession path will tick again on resume.
            return Ok(());
        }

        // Step 2: walk Space → RenderElements. The wayland_frontend
        // feature pulls in the layer-map handling automatically.
        // Empty Vec produces a clear-color frame.
        let elements = match space_render_elements(
            &mut self.device.renderer,
            [&state.space],
            &output_data.output,
            1.0,
        ) {
            Ok(v) => v,
            Err(_) => {
                // OutputNoMode — the output hasn't been given a mode
                // yet. Skip this tick.
                return Ok(());
            }
        };

        // Step 3 + 4: render and (conditionally) queue.
        let render_result = output_data
            .compositor
            .render_frame::<_, _>(
                &mut self.device.renderer,
                &elements,
                Color32F::from(CANVAS_COLOR),
                FrameFlags::DEFAULT,
            )
            .map_err(|e| anyhow::anyhow!("render_frame failed: {e}"))?;
        if !render_result.is_empty {
            output_data
                .compositor
                .queue_frame(())
                .map_err(|e| anyhow::anyhow!("queue_frame failed: {e}"))?;
            // Decrement the full-redraw counter once we've actually
            // submitted a frame — keeps the same "two consecutive
            // full redraws after invalidation" pattern as the winit
            // path.
            state.full_redraw = state.full_redraw.saturating_sub(1);
        }
        Ok(())
    }

    /// Apply a session activate / pause transition. Pausing releases
    /// DRM master so the foreground compositor on the new TTY can
    /// take over; activating re-acquires it. Subsequent ticks paint
    /// again. Called from the `LibSeatSessionNotifier` calloop source.
    pub fn handle_session_event(&mut self, event: smithay::backend::session::Event) {
        use smithay::backend::session::Event;
        match event {
            Event::PauseSession => {
                tracing::info!("session: paused — releasing drm master");
                self.device.drm.pause();
            }
            Event::ActivateSession => {
                tracing::info!("session: activated — reacquiring drm master");
                if let Err(err) = self.device.drm.activate(true) {
                    tracing::error!(?err, "drm activate failed");
                }
            }
        }
    }
}
