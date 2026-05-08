# ADR 0001 — Rust userland + WASM applets

**Status:** Accepted · 2026-05-08
**Author:** Housam
**Related:** `PLAN.md` §3 (stack decisions), `docs/research/03-wasm-applets.md`

## Context

heliOS is an AI-native Linux distribution. We use the Linux kernel and stock drivers as-is, but every layer above is custom: events bus, entity store, MCP gateway, applet runtime, compositor, login shell, agent host. Two language-stack questions need locking before any meaningful code lands:

1. What language drives the userland?
2. What is the applet primitive — native dynamic libraries, processes, scripted plugins, or WASM?

## Decision

### 1. The userland is Rust top to bottom

Every system service heliOS ships is written in Rust:

- Compositor (smithay)
- Events bus (aya, zbus, libsystemd)
- Entity store (rusqlite)
- MCP gateway
- Applet runtime (Wasmtime)
- Login shell
- Agent-host services (skills, hooks, plugins, compaction, autoDream)

Exceptions, all kept upstream and unmodified:

- Linux kernel — C
- systemd — C
- Mesa, GPU drivers — C
- Claude Code (the agent) — TypeScript runtime; we use it, do not rewrite
- Telegram bridge (kept from H) — TypeScript / Node, runs as a userland service

### 2. WASM is the applet primitive, declarative-tree UI mode

The applet runtime is **Wasmtime** with the pooling allocator + CoW memories + epoch interruption. Applets emit a **declarative UI tree** through the `host:ui/canvas` WIT world; the compositor renders it natively. Applets do NOT own GPU surfaces in the default tier. A separate "rich applet" tier with an explicit `wgpu` capability exists for the rare heavy-compute case (video player, 3D viewer) but is not on the v0.1 path.

Two applet origins are supported:

- **Installed applets** — Rust source compiled with `cargo component` to `wasm32-wasip2`. Slower compile (5-15s warm), best tooling, signed.
- **Agent-emitted ephemeral applets** — JavaScript via Javy or Rhai-in-WASM. Sub-second spawn, weaker static guarantees, capability-attenuated by default.

## Why not C++ for the userland

The principal reason is **ecosystem alignment**, not language merit:

- **smithay** (Wayland framework) is Rust. wlroots-rs (the Rust binding to the C wlroots) is dead. Picking C++ means coding against wlroots in C++ while every reference compositor (niri, cosmic-comp) is Rust.
- **aya** (eBPF) is Rust. The C/C++ alternative is libbpf with manual bindings; we'd recreate aya's CO-RE story.
- **Wasmtime** is Rust. C++ embedders exist but the cutting-edge component-model + WASI Preview 2 work happens in Rust first.
- **niri, cosmic-comp, dioxus** — three reference projects we lean on for blueprints. All Rust.

Picking C++ adds an estimated 12-18 months of bindings work before any application code lands. Memory safety is a secondary argument but real for a system service that runs as root and subscribes to every kernel event.

Performance is **not** a differentiator. Modern Rust generates equivalent machine code to modern C++ for syscall-heavy work; both bind to the same OpenGL/Vulkan/eBPF kernel surfaces. The "C++ is faster" intuition does not survive 2026 measurement for systems work.

## Why not native dynamic libraries / processes / scripts for applets

| Alternative | Why ruled out |
|---|---|
| Native shared libs (`.so`) | No sandboxing — a buggy applet crashes the OS as root. |
| Subprocess + seccomp | Per-applet process overhead, IPC cost on every UI tick. |
| Lua / Python | No type safety, weaker sandbox, slower than WASM. |
| V8 / JS engine | 100 MB+ per applet, cold-start in seconds. |
| Containers (gVisor, Kata) | Orders of magnitude too heavy for one applet. |

WASM is the only option that delivers capability-based sandboxing, sub-MB runtime per instance, microsecond cold start (with pooling allocator), single-binary multi-arch portability, and multi-language source — all required for "the agent writes a hundred applets a session."

## Why declarative-tree UI vs pixel rendering

Per `docs/research/03-wasm-applets.md`. With pixel rendering each applet ships its own font atlas, GPU surface, text shaper — 100 of those is misery in any language. With declarative-tree, the compositor batches glyph caching, hit-testing, accessibility, theming once for all applets; tree diffs serialize cheaply across the WIT boundary; hot-reload is data-not-state-machine. Same model as Figma plugins, Shopify Functions, Anthropic artifacts.

## Consequences

- All workspace crates are Rust. No C++ in the heliOS-authored userland.
- Cross-language interop happens at MCP, the events bus wire format (length-prefixed JSON), or the SQLite store — never via C++ bindings.
- Applets compile to `wasm32-wasip2`. The `rust-toolchain.toml` explicitly includes this target.
- The escape hatch for native heavy compute is the wgpu rich-applet tier; it must remain a small fraction of applets to preserve the architecture.
- If a future contributor strongly prefers C++, we accept the cost of language churn rather than reopening this decision.

## Reversibility

This decision is hard to reverse after Phase 1 ships (the events bus, store, and MCP gateway in Rust establish the in-process and IPC types). Reversing means starting over. Consequently, this ADR is **locked** until the v0.1 release.
