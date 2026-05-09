# Research 06 — Smithay 0.7 gotchas

**Date:** 2026-05-09 · **Source:** server-side iteration during Phase 2 m-3, m-4, m-5
**Distilled to:** what to do (and not do) the next time someone touches `crates/comp/`

This is the working knowledge that fell out of actually building a smithay 0.7 compositor. Most of these aren't in the official docs or are buried in the anvil example, and a couple bit us hard during CI iteration. Capturing here so the next person picking up `crates/comp/` doesn't repeat the discoveries.

## Protocol delegate ordering matters

`smithay::delegate_*!` macros generate `Dispatch` and `GlobalDispatch` impls on the state struct. **Some delegates have transitive trait bounds on other handlers**, so adding them in the wrong order produces cryptic error messages.

Specifically:

- `delegate_xdg_shell!` requires `SeatHandler` to be implemented (popup grabs need a seat reference). Add `delegate_seat!` first.

In general: if `delegate_X!` produces an error like "the trait bound `WaylandState: SomeOtherHandler` is not satisfied" with errors pointing into the macro, the X delegate has a hidden dependency on the other handler. Land it first.

## XdgShellHandler is smaller than the docs imply

`XdgShellHandler` only **requires** 5 methods in smithay 0.7:

- `xdg_shell_state(&mut self) -> &mut XdgShellState`
- `new_toplevel(&mut self, surface: ToplevelSurface)`
- `new_popup(&mut self, surface: PopupSurface, positioner: PositionerState)`
- `grab(&mut self, surface: PopupSurface, seat: WlSeat, serial: Serial)`
- `reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32)`

Smithay supplies sensible defaults for `move_request`, `resize_request`, `maximize_request`, `unmaximize_request`, `fullscreen_request`, `unfullscreen_request`, `minimize_request`, `show_window_menu`, `app_id_changed`, `title_changed`, `client_pong`. Implement only the ones whose behaviour you need to override.

## OutputState doesn't exist

There's no `OutputState` global container in smithay 0.7. Outputs are individual `smithay::output::Output` instances; you call `output.create_global::<WaylandState>(&dh)` per output to advertise it. `OutputManagerState` *does* exist, but it's only for the optional `xdg_output` extension (provides logical_position, logical_size, name, description on top of the basic `wl_output`).

## render_output's RenderElement bound

`Space::render_output(...)` takes `custom_elements: &[C]` where `C: RenderElement<R>`. When you have no custom elements (just regular client surfaces), Rust can't infer `C`'s type. Pin it explicitly:

```rust
use smithay::backend::renderer::element::solid::SolidColorRenderElement;

let elements: &[SolidColorRenderElement] = &[];
space.render_output(&mut renderer, output, age, elements, ...);
```

`SolidColorRenderElement` is the simplest concrete type; the slice is empty so its choice doesn't matter as long as it implements the bound.

## Releasing `backend.bind()` borrow before `submit()`

`backend.bind()` returns a tuple of `(renderer, framebuffer)` that holds a mutable borrow on the backend. If you try to call `backend.submit(damage)` while that tuple is in scope, the borrow checker rejects it.

Pattern that works:

```rust
let damage = {
    let (mut renderer, mut framebuffer) = backend.bind()?;
    let frame_damage = render_frame(&mut renderer, &mut framebuffer, ...)?;
    frame_damage.to_vec() // copy out, drop the bind
};
backend.submit(Some(&damage))?;
```

Note the `to_vec()` to copy the damage rectangles out of the borrow before the bind result drops at the closing brace.

## `WindowElement::wl_surface()` import requirements

Calling `window.wl_surface()` requires both `smithay::desktop::WaylandFocus` and `smithay::reexports::wayland_server::Resource` in scope. The error is opaque ("no method named wl_surface") if you don't have the imports — looks like a missing method, but is actually a missing trait.

```rust
use smithay::desktop::WaylandFocus;
use smithay::reexports::wayland_server::Resource;
```

## Calloop can't drive winit's event pump

Winit's event loop has its own pump that needs to be called from the main thread. You can't insert it as a calloop event source. The pattern that works (from anvil):

```rust
loop {
    winit_backend.dispatch_new_events(|event| {
        // forward to seat / state
    })?;
    
    event_loop.dispatch(Some(Duration::from_millis(1)), &mut state)?;
    display_handle.flush_clients()?;
}
```

A 1 ms calloop dispatch timeout keeps the main loop responsive without busy-waiting.

## Full-redraw counter for invalidation

After resize / pan / zoom, the contents of previous frames in the swapchain (or the OutputDamageTracker's history) are no longer valid for damage-based incremental rendering. The simplest correct way to handle this is a saturating counter:

```rust
pub struct WaylandState {
    pub full_redraw: u8,
    // ...
}

// On resize / pan / zoom:
state.full_redraw = state.full_redraw.saturating_add(2);

// At top of each render iteration:
if state.full_redraw > 0 {
    // bypass damage tracking, draw the whole output
    state.full_redraw = state.full_redraw.saturating_sub(1);
}
```

The `+ 2 / − 1` pattern ensures at least two consecutive full redraws after the trigger event, giving the swapchain time to cycle through and stabilise.

## Disk space watch: cargo target + mkosi caches grow fast

A cold `cargo build --release` on `crates/comp/` plus mkosi image builds plus dnf package cache will fill ~10 GB on a fresh dev machine. If the disk fills mid-write, files can get **truncated**, including source files you're editing. (We hit this at chunk 5 — `wayland.rs` got truncated by ENOSPC; recovered with `git checkout`.)

Cleanup commands when you hit pressure:

```sh
rm -rf ~/dev/helios/target          # cargo intermediate artifacts
rm -rf /var/tmp/mkosi-*              # mkosi workspaces
rm -rf /var/cache/dnf                # Fedora package cache
sudo journalctl --vacuum-size=100M   # journald logs
```

Recommend: keep the dev machine at >5 GB free at all times. Run `df -h` before any `mkosi build`.

## When you're stuck, read anvil

Smithay's `examples/anvil/` is the canonical reference compositor. About 3000 lines, but well-organised:

- `anvil/src/state.rs` — full state shape with all fields
- `anvil/src/winit.rs` — winit backend setup, GLES renderer init
- `anvil/src/render.rs` — render loop, damage tracking
- `anvil/src/input.rs` — winit input forwarding to Seat
- `anvil/src/handlers/` — protocol handler modules

Use `grep -rn "<thing>" .` to find what you need rather than reading top-to-bottom.

## Architecture invariant that has held through m-1 → m-5

`canvas.rs` / `render.rs` / `state.rs` were not touched across any of the 8 m-4/m-5 chunks. Everything Smithay-shaped lives in `wayland.rs` / `handlers.rs` / `backend.rs` / `events_client.rs` and attaches alongside on `WaylandState`. The m-1 viewport math is now load-bearing every frame.

If you find yourself wanting to modify `canvas.rs` to accommodate smithay, stop and ask: is this really compositor-specific, or is it canvas-paradigm semantics that should be smithay-agnostic? The split is load-bearing.
