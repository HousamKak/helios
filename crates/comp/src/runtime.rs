//! Compositor runtime files + child-process helpers.
//!
//! Phase 2.5 m-2.5.2. Two responsibilities, both small:
//!
//!   1. Write `/run/helios/wayland_display` and
//!      `/run/helios/x11_display` so other heliOS processes (the
//!      store, when handling `SpawnProcess`; the user, when running
//!      `WAYLAND_DISPLAY=$(cat …)`) can find the live socket names.
//!   2. Spawn a default terminal at startup so the user sees a
//!      Claude prompt the moment the canvas paints. Without this,
//!      heliOS boots to a blank canvas with no way in.
//!
//! Both are best-effort — file I/O failures and missing binaries log
//! and continue. The compositor must never refuse to run because of
//! a runtime-file write hiccup.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// Write a small one-line file (creating the parent dir if needed).
/// Best-effort: errors are logged and swallowed. Returns `true` on
/// success so the caller can decide whether to skip downstream
/// behaviour gated on a successful write.
pub fn write_runtime_file(path: &str, contents: &str) -> bool {
    let path = Path::new(path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(
            path = %parent.display(),
            ?err,
            "could not create runtime dir; skipping write",
        );
        return false;
    }
    let mut body = contents.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    match std::fs::write(path, body) {
        Ok(()) => {
            tracing::info!(path = %path.display(), value = %contents, "runtime file written");
            true
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), ?err, "runtime file write failed");
            false
        }
    }
}

/// Remove a runtime file we wrote earlier. Idempotent; best-effort.
pub fn remove_runtime_file(path: &str) {
    let path = Path::new(path);
    if path.exists()
        && let Err(err) = std::fs::remove_file(path)
    {
        tracing::debug!(path = %path.display(), ?err, "runtime file remove failed");
    }
}

/// Spawn the default terminal as a detached child of helios-comp.
/// Returns the child's pid on success.
///
/// Configuration via env (defaults match the m-2.5 brief):
///   `HELIOS_DEFAULT_TERMINAL`  default `"foot"`.
///   `HELIOS_DEFAULT_COMMAND`   default `"helios-shell"`.
///
/// The brief's intent is: foot starts, exec's helios-shell, which
/// configures Claude Code MCP and exec's `claude`. The result is one
/// canvas window showing a claude prompt.
///
/// Detach semantics mirror `helios-store::spawn` — stdio to /dev/null,
/// `setsid()` so the child survives the compositor's controlling tty
/// closing if any.
pub fn spawn_default_terminal(wayland_display: &str) -> Option<i32> {
    if std::env::var_os("HELIOS_DEFAULT_TERMINAL_DISABLED").is_some() {
        tracing::info!("default-terminal spawn disabled by env var");
        return None;
    }
    let terminal = std::env::var("HELIOS_DEFAULT_TERMINAL").unwrap_or_else(|_| "foot".to_string());
    let inner_command =
        std::env::var("HELIOS_DEFAULT_COMMAND").unwrap_or_else(|_| "helios-shell".to_string());

    // `foot -e helios-shell` is the canonical invocation; xterm and
    // most other terminals also accept `-e <cmd>`. We pass the inner
    // command as a single arg list rather than a shell string.
    let mut cmd = Command::new(&terminal);
    cmd.arg("-e").arg(&inner_command);
    cmd.env("WAYLAND_DISPLAY", wayland_display);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir("/tmp");
    // SAFETY: only async-signal-safe calls in the closure. See
    // helios-store::spawn for the matching pattern + rationale.
    unsafe {
        cmd.pre_exec(|| {
            let pid = libc::setsid();
            if pid < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id() as i32;
            std::mem::forget(child);
            tracing::info!(
                terminal = %terminal,
                inner = %inner_command,
                pid,
                "default terminal spawned",
            );
            Some(pid)
        }
        Err(err) => {
            tracing::warn!(
                terminal = %terminal,
                ?err,
                "default terminal spawn failed (PATH? binary missing?); skipping",
            );
            None
        }
    }
}
