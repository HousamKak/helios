//! Process spawning for the heliOS store.
//!
//! Phase 2.5 m-2.5.1. Backs `StoreRequest::SpawnProcess` — the
//! agent's hook for launching apps from inside the canvas. The store
//! reads the compositor's display files (written in m-2.5.2) so the
//! spawned program inherits the correct `WAYLAND_DISPLAY` /
//! `DISPLAY`, and detaches the child so it outlives the request
//! that spawned it.
//!
//! Architecture per the m-2.5 brief: spawning lives in the store, not
//! the compositor. The store is the daemon with the existing
//! MCP-facing IPC surface; the compositor only writes the display
//! files so the store can find them.
//!
//! Process lifecycle is intentionally unsupervised. If the spawned
//! child dies, the agent (Claude Code) just spawns another. v0.1
//! doesn't need a respawn loop or a session manager.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// Read a small one-line text file written by the compositor. Trims
/// whitespace. Returns `None` on any error (file missing, unreadable,
/// empty, etc.) — the spawn proceeds without setting that env var.
fn read_display_file(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(Path::new(path)).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Build, configure, and spawn the child described by the request.
/// Returns the OS-assigned pid on success.
///
/// Detach semantics:
///   * stdin/stdout/stderr → /dev/null. The child must NOT inherit
///     the store's stdio or hold open the socket fd that received
///     the request.
///   * `setsid()` via `pre_exec` so the child becomes its own
///     session leader. Prevents the child from being killed when
///     the store's controlling tty closes (relevant when the store
///     is run interactively from a dev-iteration shell rather than
///     under systemd).
///
/// Env priority (highest to lowest):
///   1. The request's `env` map (caller intent wins).
///   2. `WAYLAND_DISPLAY` / `DISPLAY` from the compositor's display
///      files, only if not already set above.
///   3. The store's existing process env (`PATH`, `HOME`, etc.).
pub fn spawn_process(
    command: &str,
    args: &[String],
    env: Option<&HashMap<String, String>>,
) -> Result<i32, std::io::Error> {
    let mut cmd = Command::new(command);
    cmd.args(args);

    // Apply request env first so its values take precedence over the
    // display files we may add next.
    if let Some(map) = env {
        for (k, v) in map {
            cmd.env(k, v);
        }
    }

    // Pull the display values from the compositor's runtime files.
    // `Command::get_envs` doesn't exist, so check the request map
    // directly to decide whether to fill the env.
    let request_has = |key: &str| -> bool { env.map(|m| m.contains_key(key)).unwrap_or(false) };
    if !request_has("WAYLAND_DISPLAY")
        && let Some(value) = read_display_file(helios_schema::ipc::WAYLAND_DISPLAY_FILE)
    {
        cmd.env("WAYLAND_DISPLAY", value);
    }
    if !request_has("DISPLAY")
        && let Some(value) = read_display_file(helios_schema::ipc::X11_DISPLAY_FILE)
    {
        cmd.env("DISPLAY", value);
    }

    // Detach: own stdio, own session.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir("/tmp");

    // SAFETY: `setsid()` is async-signal-safe. The closure runs
    // post-fork, pre-exec, in the child only — no Rust state is
    // touched, no allocations, no locks. Returning Err propagates
    // as a spawn failure to the parent.
    unsafe {
        cmd.pre_exec(|| {
            // libc::setsid is signal-safe and the only thing we need.
            // It returns -1 / sets errno on failure; map to io::Error.
            let pid = libc::setsid();
            if pid < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    let pid = child.id() as i32;
    // Drop the Child handle without waiting — we don't want the
    // store to track this process. The procfs source will see the
    // exec / exit naturally.
    std::mem::forget(child);
    Ok(pid)
}
