//! libinput device feed for the DRM backend.
//!
//! Phase 2 m-6 chunk 8. Builds a `LibinputInputBackend` from the
//! libseat session and inserts it as a calloop event source. Each
//! event the kernel emits (key press, pointer motion, button click,
//! scroll, touch, …) flows through `state.process_input_event`,
//! which is generic over `B: InputBackend` so the same handler
//! serves both the winit (nested) and libinput (bare-metal) paths.
//!
//! `LibinputSessionInterface::from(session)` wires the session's
//! `open` / `close` impls into libinput's open-restricted callback,
//! so libinput's device opens go through logind / seatd and survive
//! TTY switches without manual fd juggling.
//!
//! Reference: smithay/anvil/src/udev.rs `Libinput::new_with_udev` /
//! `udev_assign_seat` block.

use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::reexports::input::Libinput;

/// Build a libinput context bound to the given libseat session and
/// activate it for the session's seat. Returns the smithay
/// `LibinputInputBackend` ready to be inserted as a calloop event
/// source.
pub fn build_input_backend(session: &LibSeatSession) -> anyhow::Result<LibinputInputBackend> {
    // The session is cloneable (libseat's LibSeatSession is a Weak
    // handle internally; clones share state). LibinputSessionInterface
    // wraps it so libinput's open-restricted goes through logind.
    let interface = LibinputSessionInterface::from(session.clone());
    let mut context = Libinput::new_with_udev(interface);

    // Activate the libinput context for our seat. This populates the
    // device list — libinput emits "device added" events for every
    // input device the seat owns. The next process_events call in the
    // run loop will see them.
    use smithay::backend::session::Session;
    let seat_name = session.seat();
    context
        .udev_assign_seat(&seat_name)
        .map_err(|_| anyhow::anyhow!("libinput udev_assign_seat({seat_name}) failed"))?;
    tracing::info!(seat = %seat_name, "libinput seat assigned");
    Ok(LibinputInputBackend::new(context))
}
