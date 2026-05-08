# Research 01 — Compositor

**Date:** 2026-05-08 · **Source:** background research agent
**Distilled to:** decisions actually used in `PLAN.md`

## Headline

**smithay 0.7+** is the only realistic Rust foundation for the heliOS compositor. wlroots-rs is dead. We learn from **niri** (~80%) and cosmic-comp (~15%); everything else informs <5%. Render path: **GLES via `Smithay::GlesRenderer`** for v1; revisit wgpu in 12-18 months.

## Library state

- `smithay` 0.7.0 (June 2024) is published; master tracks toward 0.8 with XDG toplevel-tag, cursor-shape v2, GBM scanout filtering, refined XWayland keyboard-grab.
- Production users: **cosmic-comp** (System76, Pop!_OS 24.04 LTS, COSMIC 1.x), **niri** (YaLTeR, monthly releases), MagmaWM, several research compositors.
- Mature: protocol layer, libinput backend, DRM/GBM/EGL, `desktop::Space`, render-element + damage-tracker pipeline.
- Rough: no wgpu/Vulkan renderer (only `GlesRenderer` and `PixmanRenderer` ship; discussion #431 has not landed); occasional XDG popup positioner edge cases.
- **wlroots-rs is dead** (Way Cooler abandonment, 2019). Treat C++ binding as not-an-option.

## Compositors to study, in order

1. **niri** (`YaLTeR/niri`) — highest educational value. Already proves canvas-zoom (Overview mode), per-window GLSL shaders for transitions, infinite-strip layout. Read `src/render_helpers/`, `src/animation/`, `src/layout/`, `src/render_helpers/shader_element.rs`.
2. **cosmic-comp** (`pop-os/cosmic-comp`) — best reference for scale. Iced-via-`iced-dyrend` for compositor-drawn UI. Pop maintains a Smithay fork with patches worth diffing.
3. Hyprland (C++) — read for renderer ideas only (dual-Kawase blur, animation curves). Don't port.
4. river / Sway — orthogonal; protocol-correctness sanity-checking only.

## Where Wayland fights us, and the fixes

- **Surface scale.** Use fractional-scale-v1 + viewporter; bucket scales (1.0, 1.5, 2.0, 3.0) and let the shader handle between-bucket zoom.
- **Input coordinates.** Maintain a scenegraph, hit-test in world space, apply inverse-of-canvas-transform per entity. Write tests first.
- **Continuous zoom of clients.** Apply a GPU-side scale matrix between configures; snap to a real configure on gesture release. Generalises niri's resize trick. Visually perfect, crisp at snap points.
- **xdg_output / wp_presentation.** Lie about output geometry productively (cosmic and niri both do).
- **viewporter** for clipping legacy-app surfaces inside an entity bounding box without renegotiating size.

## Realistic solo timeline

- Month 1: anvil fork drawing canvas background + one solid quad. Read niri end-to-end.
- Month 3: world-to-screen matrix per surface. Pan/zoom gestures wired. One XWayland app placeable. Damage tracking mostly working.
- Month 6: multi-monitor (deferred), fractional-scale + viewporter, smooth-zoom shader, 5-10 entities @ 60fps, compositor-drawn entity chrome.
- Month 12+: daily-driver territory.

Skip-list (first 6 months): HDR, color management, screen-share portals, accessibility, session lock, non-US keyboards, tablet/stylus, multi-GPU. Each is a multi-week rabbit hole.

## Highest-leverage repos to clone

1. `https://github.com/YaLTeR/niri` — the blueprint
2. `https://github.com/Smithay/smithay` — `anvil/`, `src/desktop/space.rs`, render-element traits
3. `https://github.com/pop-os/cosmic-comp` — `src/shell/`, the iced-dyrend integration, the pop-os Smithay fork
