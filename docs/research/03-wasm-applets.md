# Research 03 — WASM applets

**Date:** 2026-05-08 · **Source:** background research agent
**Distilled to:** decisions actually used in `PLAN.md` and `ADR 0001`

## Headline

- **Wasmtime** + pooling allocator + CoW memories + epoch interruption + pre-compiled `.cwasm` cache.
- **Declarative UI tree**, not pixel rendering. Applet emits a tree; compositor renders.
- **Dioxus VirtualDOM** as the tree primitive, behind a host-defined WIT world (`host:ui/canvas`).
- **Hybrid origin model**: Rust+Component for installed applets; Javy / Rhai for ephemeral agent-emitted ones (sub-second spawn).

## Runtime

Wasmtime is the reference Component Model implementation; WIT, `wit-bindgen`, resource handles are first-class. Wasmer's component-model story lags; WasmEdge focuses on edge AI; `wasmi` is interpreter-only.

For hundreds of concurrent instances: `PoolingAllocationConfig` + memfd/madvise CoW (PR #3697). Steady-state instantiation = single `madvise(MADV_DONTNEED)`. Spin 2.0 reported up to 10× throughput vs default. Pre-compile to `.cwasm` so compile cost is paid once at install.

CVE-2026-34988 (data leakage between pool slots): pin a recent Wasmtime release.

## Capabilities = WIT imports

Every WIT `import` in the applet's component is a capability. wasmCloud + Cosmonic Control + Spin 2.0 all use this pattern. heliOS applet manifest follows: declare which `host:*/...@version` worlds the applet imports, plus per-import policy ("can call `bsky:client/publish` against account X"). The host injects an attenuated implementation. Cleaner than Figma's permission strings because the type system enforces wire shape.

## Pixels vs declarative tree

Tree wins decisively at heliOS scale:

- A pixel-rendering applet ships a font atlas + GPU surface + text shaper — Slint+FemtoVG WASM is ~2-4 MB before app code. 100 applets × that = RAM disaster.
- The host can batch glyph caching, hit-testing, accessibility, theming **once** for all applets.
- Tree diffs serialize cheaply across the WIT boundary.
- Hot-reload + state-migration tractable when UI is data, not GPU state.

Same model as Figma plugins (V8 isolates today, evaluating WASM), Shopify Functions, Claude artifacts (declarative React tree in iframe).

## Tree library choice

**Dioxus VirtualDOM** behind a custom WIT widget world. Why:

- VirtualDOM is literally a serializable tree.
- `dioxus-ssr` and LiveView already pass diffs over a wire.
- Cleanest fit for "applet emits patches, host renders."

Alternatives ranked: **Xilem/Masonry** (best architectural fit, alpha 2026 Q1, revisit late-2026), Iced (compositor uses it but expects to own rendering), Slint/egui (pixel-oriented, wrong tier), Leptos (web-DOM-shaped).

## Ephemeral / agent-emitted applets

A Rust applet via `cargo component` is 5-15 s warm-cache compile. Borderline for "agent writes an applet on demand." Escape hatches:

- **Javy** (Shopify QuickJS-in-WASM) — ~800 KB; sub-second iteration for JS applets. Best fit.
- **Roto** (NLnet Labs, 2025) — Cranelift-JIT'd embedded scripting language, hot-reloadable, designed exactly for this.
- **Rhai** — ~160 KB gzipped WASM.

heliOS hybrid: Javy for ephemeral; Rust+Component for user-installed.

## State, hot reload, communication

- Wasmtime epochs for cooperative preemption; fuel for hard limits.
- Serialize state through a WIT-defined `applet:state/snapshot` interface (`save() -> list<u8>` / `restore(list<u8>)`). Don't migrate raw linear memory — component resource handles aren't portable.
- Intra-process applet ↔ host: WIT calls — tens of ns for primitives, µs through canonical ABI.
- Applet ↔ applet: route through host-mediated WIT interfaces. Never link directly — capability checks must happen on every hop.

## Three projects to study

1. **Zed extensions** — closest analogue. WASM Component, `wasm32-wasip2`, WIT-versioned host API, wasmtime + sandboxed reload.
2. **Figma plugin system** — canonical "host renders, plugin emits scenegraph mutations." Battle-tested isolation.
3. **wasmCloud + Cosmonic Control** — capability-provider pattern at scale.
