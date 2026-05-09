# Research 07 — XWayland quirks

**Date:** 2026-05-09 · **Source:** server-side iteration during Phase 2 m-7
**Distilled to:** what to do (and not do) the next time someone touches `crates/comp/src/xwayland/`

Mirror of `06-smithay-0.7-gotchas.md` but specifically for the XWayland integration. Anything that bit us during m-7 implementation, with enough context that the next person can avoid it.

## smithay 0.7 doesn't expose `set_motif_hints` from the WM side

`X11Surface::is_decorated()` is a *reader* of the client's preference (the client wrote `_MOTIF_WM_HINTS` on its own window). There's no `set_motif_hints()` or equivalent — the WM can't tell the client "we'll handle decorations" via this property.

**Consequence:** X clients fall back to whatever decoration they default to. xterm / xclock draw nothing (they expect WM-side chrome); Firefox / Electron / GTK apps draw their own. This will look mixed until either:

- Smithay upstream adds the setter (we should file an issue), OR
- m-8 lands canvas chrome that overlays *every* entity uniformly, making client-drawn chrome a moot point.

What works as a partial workaround: `XwmHandler::property_notify(MotifHints)` lets us *react* to the client's choice and route it into our own state. We use this to log which clients are CSD vs SSD-expecting, but we can't change them.

## `surface_associated` fires after `new_window`, not at the same time

The X11 lifecycle is two-stage:

1. `XwmHandler::new_window(X11Surface)` — the X window exists. **No `wl_surface` yet** in most cases.
2. `XWaylandShellHandler::surface_associated(WlSurface, X11Surface)` — XWayland's `xwayland_shell_v1` protocol associates a wl_surface with the X window. From this point onwards `X11Surface::wl_surface()` returns Some.

**Implication for surface↔entity binding:** mint the EntityId at `surface_associated`, not at `new_window`. We tried it the other way in m-7.3 and the binding silently no-op'd because `wl_surface` was None at creation time.

## `X11Wm::start_wm` requires `XWaylandShellHandler` on the state

The trait bounds on `start_wm<D>` include both `XwmHandler` AND `xwayland_shell::XWaylandShellHandler`. If you only impl `XwmHandler`, the compile error is opaque (deep inside the macro-generated code). Add `XWaylandShellHandler` + `delegate_xwayland_shell!(WaylandState)` together — they always travel as a pair.

## Override-redirect detection: use `is_override_redirect()` everywhere

OR-windows look identical to regular toplevels until you check the flag. The flag is on `X11Surface`, accessible via `Window::x11_surface()` for windows that have an X11 backing.

**Pattern that works:**

```rust
// In any code that walks Space::elements():
if window
    .x11_surface()
    .map(|s| s.is_override_redirect())
    .unwrap_or(false)
{
    // OR-window — screen-fixed, top z-order, no canvas binding
}
```

Apply this in:
- `WaylandState::reapply_viewport_to_windows` — skip OR windows so pan/zoom doesn't move them.
- `XWaylandShellHandler::surface_associated` — skip the entity binding (OR windows don't get EntityIds; they're not canvas entities).
- `XwmHandler::configure_notify` — for OR windows, re-map to the client's new screen-pixel location.

## XWayland needs `DISPLAY=:N` set in our env, not just child env

When `XWaylandEvent::Ready` fires, we set `DISPLAY=:N` in our own process environment via `std::env::set_var`. Children we spawn (skills, applets) inherit this automatically; users who run `xclock` from another shell need to set DISPLAY themselves.

Rust 1.95 makes `set_var` `unsafe` due to multi-threaded environment races. Linux is safe at our usage point — startup, single-threaded, before any child spawn — but the `unsafe` block is required.

## XWayland binary must be on PATH

`XWayland::spawn` calls `Command::new("Xwayland")` (not the full path). On Fedora the binary ships in `xorg-x11-server-Xwayland`; on Ubuntu in `xwayland`; on the heliOS image already in `distro/mkosi.conf` Packages list.

If the binary is missing, `XWayland::spawn` returns an `io::Error` with `NotFound`. We surface this as `XwaylandError::Spawn`.

## The XWayland process exit gets eaten without `Stdio::null()`

`XWayland::spawn` lets you pass any `Stdio` for stdout/stderr. We pass `Stdio::null()` for both because:
1. XWayland's `-verbose` output is rarely useful and pollutes our trace logs.
2. If the parent process holds the pipe end and doesn't drain it, XWayland blocks on log writes.

## Test matrix snapshot

Verified during m-7.7:

| App | Status | Notes |
|---|---|---|
| `xclock` | ✓ renders | No client decoration; cleanly maps + closes. |
| `xterm` | ✓ renders, accepts input | Right-click → context menu appears as OR-window in screen space. |
| `xeyes` | ✓ renders | Useful for sanity-checking pointer event flow. |
| `firefox --no-remote` | ✓ renders | Draws its own decorations; will look out-of-place until m-8 chrome. |
| `chromium` | ✓ renders | Same caveat as Firefox. |
| `gimp` | ✓ renders | Multiple toolbox windows + dialogs; OR menus work. |
| `code` (VS Code Electron) | ✓ renders | Self-decorates; otherwise behaves. |

Apps not yet tested or known-broken:

| App | Status | Notes |
|---|---|---|
| Steam | not tested | Likely needs XInput2 (m-11) for full controller support. |
| Discord | not tested | Electron app; should work with Firefox-level support. |
| `xclip` / clipboard | broken | X selection handling deferred to m-12. |
| Drag-and-drop | broken | Same — m-12. |

## Things NOT to add to xwayland integration without ADR

- Multi-X-server (one per project / desktop) — v0.2 idea.
- X selection / clipboard — m-12.
- X drag-and-drop — m-12.
- XInput2 / XI2 — m-11.
- Screensaver / DPMS X protocols — out of scope.
- XSettings beyond what smithay's `set_xsettings` exposes for free.
