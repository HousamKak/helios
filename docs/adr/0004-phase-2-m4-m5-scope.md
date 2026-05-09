# ADR 0004 — Phase 2 m-4 + m-5: Renderer & Canvas Integration

**Status:** Accepted · 2026-05-09
**Author:** Housam
**Related:** `PLAN.md` §6 Phase 2, `docs/adr/0003-phase-2-compositor-scope.md`,
`docs/research/01-compositor.md`, `crates/comp/`

## Context

Phase 2 m-3 closed with a complete Wayland protocol surface: clients can connect and bind every global they need (`wl_compositor`, `wl_subcompositor`, `wl_shm`, `wl_seat`, `xdg_wm_base`, `wl_output`), the calloop event loop dispatches their requests, and surfaces are tracked in protocol state. What's missing is anything happening *visually* — buffers attach but never paint, input devices don't exist, and the canvas math from m-1 is unused.

m-4 and m-5 close that gap. m-4 produces the first rendered frame. m-5 wires the canvas math into surface placement so windows actually live in the world.

## Decisions

### Renderer

**`smithay::backend::renderer::gles::GlesRenderer` (GLES2/3) for v0.1.** Wgpu integration in smithay is still a discussion thread; GLES is the path everyone ships (cosmic-comp, niri, anvil). Revisit wgpu in 12-18 months once the discussion lands.

### Backend

**`backend_winit` for nested-Wayland iteration; `backend_drm` deferred to m-6+.** Winit lets us run `helios-comp` inside an existing Wayland session on housam-server (with NoMachine forwarding to Windows for visual access). DRM/KMS is the bare-metal path, but it requires session management, GPU device selection, multi-GPU support, and DRM master handoff — all of which are multi-week rabbit holes. Defer.

### Input

**`backend_winit` provides input events directly during nested testing.** Once we move to bare-metal in m-6+, `backend_libinput` takes over. The seat-side abstraction (`smithay::input::Seat::add_keyboard`, `add_pointer`) is the same regardless of which backend feeds events.

### Damage tracking

**`smithay::backend::renderer::damage::OutputDamageTracker`.** Smithay provides this; it tracks per-surface damage regions and produces minimal redraw rectangles each frame. Without it, every commit redraws the full output.

### Buffer import

**`smithay::backend::renderer::utils::on_commit_buffer_handler` in `CompositorHandler::commit`.** Smithay handles SHM buffer import to GLES textures internally. Dmabuf import (for hardware-accelerated clients) is deferred to m-6+ because it requires explicit-sync protocol negotiation.

### Surface ↔ canvas-entity mapping

**Runtime-only `HashMap<SurfaceId, EntityId>` in `WaylandState`. Not persisted.** Wayland surfaces are ephemeral — they die on client disconnect. Persisting `wl_surface_id → canvas_entity_id` would create stale rows on every client crash. The compositor maintains the map at runtime; the events bus emits `SurfaceMapped` / `SurfaceUnmapped` events that the store projects into transient rows in `canvas_entities` (created with `entity_kind = 'process'` if there's a known PID, or a transient kind otherwise).

### Window manager policy

**Centred-spawn-with-stagger for v0.1.** New `xdg_toplevel` surfaces land at the active desktop's centre, offset by a small per-window stagger so subsequent windows don't fully overlap. m-7+ will add real placement policies (snap-to-grid, project-zone-based, manual placement). For now, the canvas math from m-1 means panning/zooming makes the stagger irrelevant anyway — the user moves the world, not the windows.

### XWayland

**Deferred to m-6+.** XWayland needs its own DISPLAY setup, X11 protocol handling, and the X11Surface ↔ Window adapter. Each is a multi-day chunk. m-4 and m-5 are about getting *native* Wayland clients drawn correctly first; XWayland is "more clients" not "the demo arc lands".

### Multi-monitor

**Deferred to m-6+.** A single `wl_output` advertised at 1920×1080 is enough for the v0.1 demo. Multi-output adds output enumeration, per-output damage tracking, output configuration (xdg-output-manager), and surface-on-which-output bookkeeping. None of those affect the canvas paradigm; they're infrastructure for an edge case.

### Performance budget

Per the compositor research: 5–10 entities at 60fps is the m-5 target. 60fps on a single 1920×1080 output is 16.6ms/frame. With smithay's damage tracking and SHM-only clients, that's achievable on housam-server's iGPU.

## Scope

### m-4 deliverables (in order, each its own commit)

1. **GlesRenderer + winit backend bootstrap** — `helios-comp` opens a winit window, initializes EGL, creates a `GlesRenderer`, runs an empty render loop that clears to a heliOS-canvas background colour each frame.
2. **Surface texture rendering** — on each `CompositorHandler::commit`, the surface's buffer is imported via `smithay::backend::renderer::utils::on_commit_buffer_handler`. The render loop walks `space.elements()`, draws each window's texture at its `Space` position. Centred fullscreen for now (no canvas transform yet).
3. **Damage-tracked redraws** — `OutputDamageTracker` integrated. Idle compositor draws zero frames after the initial paint until something commits.
4. **Input events from winit** — pointer + keyboard events forwarded to `Seat::motion`, `Seat::button`, `Seat::keyboard`. Surface focus tracked. Test with `weston-terminal`: typing produces visible characters.

### m-5 deliverables (in order, each its own commit)

5. **World-to-screen transform applied per surface** — replace fullscreen-centre placement with `viewport.world_to_screen_transform()`. Surfaces draw at their world-space `Space` position transformed by the active viewport. Default viewport puts (0,0) at screen centre with zoom=1.0.
6. **Pan + zoom gestures** — pointer scroll wheel triggers `viewport.zoom_around(cursor_pos, 1.1)` (or 0.9 with shift). Two-finger trackpad gestures or middle-mouse-drag pan via `viewport.pan_by_screen_pixels`. Cursor-anchored zoom uses the m-1 math directly.
7. **Surface ↔ entity mapping + events emission** — `HashMap<SurfaceId, EntityId>` in `WaylandState`. On `xdg_toplevel.new_toplevel`, generate an `EntityId`, place the window in `Space` at the active desktop's centre, emit `SurfaceMapped { surface_id, client_pid, kind }` on the events bus. On client disconnect, emit `SurfaceUnmapped` and remove from `Space`. helios-events readers (helios-store among them) project these into the universal events log.
8. **Subscribe to helios-store for canvas_entities updates** — when an external producer (skill, agent, applet) updates a canvas_entity row, the compositor receives the update via the events bus and re-positions the corresponding window in `Space`. This is the bidirectional flow: compositor publishes its positions, store persists, agent reads, agent re-positions, compositor receives the update.

### Out of scope for m-4 + m-5

- XWayland integration (m-6+)
- Multi-monitor (m-6+)
- DRM/KMS bare-metal backend (m-6+)
- Hardware-accelerated client buffers (dmabuf import, m-6+)
- Custom GLSL shaders for canvas effects (m-7+)
- Smooth-zoom-between-snaps shader trick from niri (m-7+)
- Per-entity decoration / chrome (m-8+)
- XDG decoration negotiation (m-6+, currently SSD assumed)
- Cursor theme rendering (m-6+, currently default cursor)
- Touch input (m-6+)
- Tablet / stylus (m-9+)

## Architecture

### Data flow during a single client commit

```
client                                                                 helios-comp
─────                                                                  ───────────
weston-terminal sends wl_surface.commit
                                              ──→  smithay's wayland-server hands
                                                   off to CompositorHandler::commit
                                              ──→  smithay::backend::renderer::utils::
                                                   on_commit_buffer_handler imports
                                                   shm buffer → GLES texture
                                              ──→  surface marked damaged in the
                                                   OutputDamageTracker
                                              ──→  calloop scheduler wakes the render
                                                   timer (or immediate redraw)
                                              ──→  render loop walks space.elements(),
                                                   each window draws its texture at
                                                   viewport.world_to_screen_transform
                                                   applied to its Space position
                                              ──→  GlesRenderer.render_output, swap
                                                   buffers, frame done
                                              ──→  send_frame_callback to client
client receives frame_done → renders next frame
```

### Data flow during a viewport pan

```
user                                                                   helios-comp
────                                                                   ───────────
trackpad two-finger drag right                ──→  winit emits pointer
                                                   relative-motion events
                                              ──→  WaylandState pan handler:
                                                   viewport.pan_by_screen_pixels(dx, dy)
                                              ──→  every output marked damaged
                                              ──→  next frame renders with new
                                                   world-to-screen transform
                                              ──→  every surface visibly shifts
```

### Data flow during a store-driven entity move

```
agent / skill / applet                                                 helios-comp
──────────────────────                                                 ───────────
helios-mcp tool call: move_entity(id, x, y)
helios-store.canvas_entities row updated
helios-store emits EntityPlaced event       ──→  WaylandState's
                                                   events-bus subscriber receives
                                              ──→  finds Window in Space by
                                                   wl_surface_id → entity_id map
                                              ──→  space.map_element(window, (x, y))
                                              ──→  output marked damaged
                                              ──→  next frame renders new position
```

## Consequences

- After m-5, the compositor is *load-bearing*: real Wayland clients connect and draw, the canvas math is exercised every frame, the events bus is bidirectional, and the agent (via MCP→store→events bus→compositor) can place windows on the canvas.
- The architecture invariant from ADR 0003 holds: `canvas.rs` / `state.rs` / `render.rs` are unchanged through m-4 and m-5. Smithay state continues to attach alongside on `WaylandState`.
- The Phase 2 demo lands when m-5 closes: `weston-terminal` running on heliOS, panned around on a canvas, zoomed in via cursor wheel, position read by `helios get-process` via MCP. That's a real OS doing real work.

## Reversibility

m-4 is hard to back out once GlesRenderer + winit are wired — touches `WaylandState`, the calloop loop, and adds GL context lifetime to the daemon. Reversal means losing the renderer entirely.

m-5 is easier to back out: viewport and gestures are additive. Removing the world-to-screen transform call falls back to centred-fullscreen placement.

XWayland deferral is reversible at any time. So is multi-monitor.

## Notes captured from m-3 server-side iteration

(Worth preserving so the next person picking up smithay doesn't repeat the discoveries.)

- `XdgShellHandler` only requires 5 methods in smithay 0.7: `xdg_shell_state`, `new_toplevel`, `new_popup`, `grab`, `reposition_request`. `move_request`, `resize_request`, `maximize_request`, etc. all have sensible defaults. The Phase-2 m-3 first attempt over-implemented the trait.
- There's no `OutputState` in smithay 0.7. Outputs are individual `Output` instances. `OutputManagerState` exists only for the optional `xdg_output` extension.
- `delegate_xdg_shell!` has a transitive trait bound on `SeatHandler` (popup grabs need a seat). Order matters: `delegate_seat!` lands before `delegate_xdg_shell!` or the latter fails to compile with cryptic Dispatch errors.
- Calloop replaces the sleep-poll with FD-readiness. `ListeningSocketSource` for accept, `Generic<Display>` for client request dispatch, `flush_clients()` after each iteration. Latency drops from up-to-50 ms to scheduler microseconds.
