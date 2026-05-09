//! libseat session wrapper.
//!
//! Phase 2 m-6 chunk 4. Opens a logind / seatd session via libseat so
//! the compositor can grab DRM master and open input devices without
//! running as root. The session also tells us when the user switches
//! TTY (logind sends Activate / Deactivate signals) — pausing on
//! Deactivate is required so the new session foreground compositor
//! can take DRM master, and resuming on Activate restores rendering.
//!
//! Architecture: `Session` owns the `LibSeatSession` and exposes the
//! `LibSeatSessionNotifier` for insertion as a calloop event source
//! (m-6.7 wires the loop). Each device open (DRM, libinput) goes
//! through `session.open(path, flags)` so logind owns the fd and
//! revokes it on TTY switch automatically.
//!
//! Reference: smithay/anvil/src/udev.rs — search for
//! `LibSeatSession::new` and the `SessionEvent::ActivateSession`
//! handler block.

use smithay::backend::session::Session;
use smithay::backend::session::libseat::{LibSeatSession, LibSeatSessionNotifier};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("failed to open libseat session: {0}")]
    Open(#[from] smithay::backend::session::libseat::Error),
}

/// The compositor's slice of the seat. Holds the active session so
/// device opens go through libseat. The notifier is a calloop event
/// source emitting `SessionEvent::Activate / Pause` on TTY switch;
/// m-6.7 inserts it into the loop and m-6.9 routes the events to the
/// DRM device pause/resume API.
pub struct CompSession {
    /// Active seat session. Cheap to clone (the underlying `Seat` is
    /// stored in the notifier; `LibSeatSession` holds a Weak<…> to it).
    pub session: LibSeatSession,
    /// Calloop event source for activate / pause signals. Inserted
    /// into the loop in m-6.7.
    pub notifier: Option<LibSeatSessionNotifier>,
    /// Cached seat name (e.g. `"seat0"`) — pulled out so it can be
    /// logged after the notifier is moved into the loop.
    pub seat_name: String,
}

impl CompSession {
    /// Open the session. Fails if no logind / seatd is reachable
    /// (i.e. the process isn't running under a real session, or the
    /// XDG_SESSION_ID env var is missing).
    pub fn open() -> Result<Self, SessionError> {
        let (session, notifier) = LibSeatSession::new()?;
        let seat_name = session.seat();
        tracing::info!(seat = %seat_name, "libseat session opened");
        Ok(Self {
            session,
            notifier: Some(notifier),
            seat_name,
        })
    }

    /// Take the notifier out for insertion into the calloop event
    /// loop. Returns None if it has already been taken — calling more
    /// than once is a programming error.
    pub fn take_notifier(&mut self) -> Option<LibSeatSessionNotifier> {
        self.notifier.take()
    }
}
