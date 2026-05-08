# ADR 0003 — Phase 2 begins: compositor scaffold

**Status:** Accepted · 2026-05-09
**Author:** Housam
**Related:** `PLAN.md` §6 Phase 2, `docs/research/01-compositor.md`,
`crates/comp/`

## Context

Phase 1 is done at runtime: heliOS observes the system, persists state, exposes it to Claude Code via MCP. The compositor crate (`crates/comp/`) has been a Linux-gated stub since the original scaffold. With Phase 1 verified end-to-end on housam-server and a real bootable image produced, Phase 2 begins.

## Decision

`crates/comp/` is now an *active* compositor crate, scaffolded ahead of full smithay integration so the architectural shape is locked in early. The scaffold compiles + clippy-passes + unit-tests in CI today; smithay integration lands incrementally as the development environment can support it.

### Boundaries of the scaffold

The crate currently contains:

- **`canvas.rs`** — `WorldPoint`, `CanvasTransform`, `Viewport`, `EntityPlacement`. The 2D affine math the compositor will use to map canvas-world coordinates to screen coordinates. Pan + zoom mutate the viewport; everything else is derived. Five unit tests cover identity, inversion round-trip, viewport centering, pan, and anchored zoom (the trick that keeps the cursor's world-point stable while zooming around it).
- **`state.rs`** — `HeliosState`, the single struct the event loop will own. Today it carries the viewport, the cached placement set, and the active desktop ID. Smithay protocol-handler state (`CompositorState`, `XdgShellState`, `ShmState`, `SeatState`, `Space`, `GlesRenderer`) is enumerated in commented-out fields so the eventual integration is unambiguous.
- **`render.rs`** — `RenderPlan`, `RenderItem`, `RenderItemKind`. A typed description of the frame to be drawn. Built from `HeliosState.placements` filtered by visibility and sorted by Z. No GL calls; just data. Three unit tests cover empty plan, visibility filter, and Z-order.
- **`main.rs`** — Linux-only `tokio::main` that constructs an empty state, builds a render plan, logs both, and exits. The smithay event loop's expected shape lives in commented-out pseudocode — a checklist for the next pass.

### What's deliberately NOT in the scaffold

- **Smithay deps.** Adding `smithay`, `wayland-server`, `calloop` to `Cargo.toml` brings system-library link-time requirements (`libdrm-dev`, `libgbm-dev`, `libinput-dev`, `libxkbcommon-dev`, `libegl1-mesa-dev`). Until CI's apt install carries those AND the housam-server host has them, smithay code lives behind feature gates.
- **Renderer code.** `GlesRenderer` setup, surface compositing, custom shaders. Each requires a working iteration loop under nested-Wayland with a real GPU. Phase 2 month-2 territory.
- **Protocol handlers.** `CompositorHandler`, `XdgShellHandler`, `SeatHandler`, etc. Each is mostly delegate boilerplate via smithay's `delegate_*!` macros — but each is several hundred lines of "real work" and must be tested against actual clients.
- **XWayland.** Phase 2 month-3 onwards. Lives in a future `xwayland.rs` module.

## Why this scaffold first

1. **Lock the canvas math early.** `Viewport::zoom_around` is non-trivial — the cursor-anchored-zoom calculation is one of the things you reach for ten times a day in a canvas UI and it has to be right. Writing it now, with tests, before smithay integration, means we verify the math in isolation.
2. **Force the state shape.** Smithay's protocol delegates require `HeliosState` to implement specific traits. If we'd added smithay first and let the integration drive the struct, we'd end up with a state shape optimized for smithay's needs rather than for the canvas. The scaffold puts the canvas first.
3. **Keep CI green.** Compositor work is iterative and hardware-dependent. A scaffold that compiles unconditionally means the rest of the workspace continues to pass CI as compositor changes land.
4. **Document the integration shape.** The commented-out pseudocode in `main.rs` and the `// Future fields` block in `state.rs` aren't placeholder noise — they're the contract for whoever picks up smithay integration. New contributors (human or agent) read those and know exactly where their changes go.

## Phase 2 sequence (as planned, subject to revision)

| Month | Deliverable | Notes |
|---|---|---|
| 1 | This scaffold | Done. |
| 2 | smithay deps in Cargo.toml + system libs in CI/host. Anvil-derived event loop draws a colored canvas background under nested-Wayland. | Requires NoMachine on housam-server for visual iteration. |
| 3 | World-to-screen transform applied per surface; pan/zoom gestures wired through. One XWayland app placeable on canvas. | The "canvas as a Wayland compositor" milestone. |
| 4 | Damage tracking; multi-monitor (deferred-by-default in QEMU); fractional scale + viewporter. | |
| 5 | Smooth-zoom GLSL shader between configure snap-points. 5–10 entities @ 60 fps. | |
| 6 | Compositor-drawn entity chrome (frame, hover, decoration policy). Subscribe to `helios-store` for canvas_entities; render real entities from real PIDs. | The "you're using heliOS" moment. |
| 12+ | Daily-driver territory. | |

Skip-list (first 6 months): HDR, color management, screen-share portals, accessibility, session lock, non-US keyboards, tablet/stylus, multi-GPU. Each is a multi-week rabbit hole; defer until the core is stable.

## Consequences

- The compositor binary `helios-comp` builds + ships in the image starting now, even though it does nothing yet. This avoids "is the compositor in the build pipeline?" questions later.
- The render pipeline is data-driven: `HeliosState` → `RenderPlan` → (eventual) GL submission. This makes testing the scenegraph layer trivial without a GL context.
- `EntityPlacement` is distinct from `helios_schema::CanvasEntity`. The schema row is the *persisted* form; `EntityPlacement` is the *runtime* form derived from it. Future store-side changes (additional entity kinds, decoration metadata, etc.) flow through the schema; runtime placement logic stays in-crate.

## Reversibility

Every choice in the scaffold is reversible until smithay code lands. The canvas math is the most-load-bearing decision; if a future Phase 2 iteration finds it inadequate, replacement is local to `canvas.rs` and propagates through the existing tests. The state and render shapes are explicitly framed as month-1 deliverables, not month-12 contracts.
