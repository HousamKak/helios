//! Compositor-level keybindings.
//!
//! Phase 2.5 m-2.5.3. The compositor intercepts a small set of
//! Super-modifier shortcuts before forwarding the keystroke to the
//! focused surface. Per the m-2.5 brief, the v0.1 set is intentionally
//! small:
//!
//!   * `Super+Space` — re-centre + re-focus the default terminal so
//!     the user can always reach the Claude prompt regardless of how
//!     they've panned the canvas.
//!   * `Super+Q`     — close the focused window. Bonus, off the brief
//!     ("Optional bonus: Super+Q to kill the focused window.").
//!
//! Anything broader (workspaces, tag/floating layouts, custom
//! launchers) is post-v0.1; the canvas paradigm doesn't model
//! "workspace" the same way a tiler does.
//!
//! `try_handle` returns `Some(action)` when the modifier+keysym
//! pattern matches one of our intercepts. The dispatcher in
//! `wayland.rs` consults this before forwarding the event through
//! the seat's keyboard input filter.

use smithay::input::keyboard::{Keysym, ModifiersState, keysyms};

/// Recognised compositor shortcuts. Kept as an enum (not direct
/// callbacks) so the dispatch site can apply them with the right
/// state-borrowing pattern — Rust's borrow checker doesn't like
/// "closure that takes &mut self" stored alongside other fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Pan to the default terminal entity and give it keyboard focus.
    /// No-op if the compositor never tagged a default terminal (e.g.
    /// the spawn failed or it was disabled by env var).
    FocusDefaultTerminal,
    /// Close the surface that currently has keyboard focus. Sends a
    /// graceful close request; the client may ignore it (rare).
    CloseFocused,
}

/// Returns `Some(action)` if the (modifier + keysym) pair matches a
/// recognised compositor shortcut. Otherwise `None`, and the caller
/// forwards the event to the focused surface.
///
/// Only the `logo` ("Super" / Windows) modifier is considered — we
/// don't want to swallow Ctrl+Q or Alt+F4 (those belong to
/// applications). The other modifiers (shift, caps_lock, num_lock,
/// iso_level_3_shift) are ignored; Super+Space with caps_lock on
/// is still Super+Space.
pub fn try_handle(modifiers: &ModifiersState, sym: Keysym) -> Option<KeyAction> {
    // Reject if Super isn't pressed; reject if either ctrl or alt is
    // also held (those combinations are reserved for apps and other
    // chord patterns).
    if !modifiers.logo || modifiers.ctrl || modifiers.alt {
        return None;
    }
    let space = Keysym::from(keysyms::KEY_space);
    let q_lower = Keysym::from(keysyms::KEY_q);
    let q_upper = Keysym::from(keysyms::KEY_Q);
    if sym == space {
        Some(KeyAction::FocusDefaultTerminal)
    } else if sym == q_lower || sym == q_upper {
        Some(KeyAction::CloseFocused)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifiers(logo: bool, ctrl: bool, alt: bool, shift: bool) -> ModifiersState {
        ModifiersState {
            ctrl,
            alt,
            shift,
            logo,
            ..Default::default()
        }
    }

    #[test]
    fn super_space_focuses_default_terminal() {
        assert_eq!(
            try_handle(
                &modifiers(true, false, false, false),
                Keysym::from(keysyms::KEY_space)
            ),
            Some(KeyAction::FocusDefaultTerminal)
        );
    }

    #[test]
    fn super_q_closes_focused() {
        assert_eq!(
            try_handle(
                &modifiers(true, false, false, false),
                Keysym::from(keysyms::KEY_q)
            ),
            Some(KeyAction::CloseFocused)
        );
    }

    #[test]
    fn space_alone_is_not_intercepted() {
        // Bare space goes to the focused surface — typing in a
        // terminal must work.
        assert_eq!(
            try_handle(
                &modifiers(false, false, false, false),
                Keysym::from(keysyms::KEY_space)
            ),
            None
        );
    }

    #[test]
    fn ctrl_super_space_is_not_intercepted() {
        // Don't swallow chord patterns that include other modifiers.
        assert_eq!(
            try_handle(
                &modifiers(true, true, false, false),
                Keysym::from(keysyms::KEY_space)
            ),
            None
        );
    }
}
