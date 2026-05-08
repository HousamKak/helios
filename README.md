# heliOS

> An AI-native Linux distribution. The OS is one infinite-zoom canvas where every process, file, network connection, agent, and applet is a visible entity. The system shell is Claude Code itself. Apps are WASM applets the agent generates on demand.

**Status:** v0.0.1 — scaffolding only. Phase 0 in progress (per [`PLAN.md`](./PLAN.md)).

**License:** AGPL-3.0-only (subject to revisit before first public release; see `PLAN.md` §11).

---

## What's here

| Path | Purpose |
|---|---|
| [`PLAN.md`](./PLAN.md) | Master plan. Read first. 18-month, four-phase build to v0.1. |
| [`Cargo.toml`](./Cargo.toml) | Cargo workspace. All Rust userland crates. |
| [`crates/schema/`](./crates/schema/) | Canonical entity types + SQL migrations. Ported from `H/db` and `H/types`. |
| [`crates/events/`](./crates/events/) | System-events bus daemon (eBPF + procfs + zbus + journal + netlink). |
| [`crates/store/`](./crates/store/) | Entity store daemon (SQLite-backed projection of the events bus). |
| [`crates/mcp/`](./crates/mcp/) | MCP gateway exposing OS services to Claude Code. |
| [`crates/applets/`](./crates/applets/) | Wasmtime-based applet runtime + capability gating. |
| [`crates/ui-wit/`](./crates/ui-wit/) | WIT widget world for declarative applet UIs. |
| [`crates/comp/`](./crates/comp/) | Wayland compositor (Smithay-based). The canvas. |
| [`crates/shell/`](./crates/shell/) | Login session — Claude Code as the user shell. |
| [`crates/agent-host/`](./crates/agent-host/) | Skills, hooks, plugins, autoDream — services Claude Code consumes. |
| [`crates/compat/`](./crates/compat/) | XWayland glue, Flatpak provisioning. |
| [`distro/`](./distro/) | mkosi configs, image build scripts, A/B partition layout. |
| [`docs/research/`](./docs/research/) | Component research from the planning phase. |
| [`docs/adr/`](./docs/adr/) | Architecture decision records. |

## Quickstart (dev)

> **Note:** The compositor and distro pieces only build and run on Linux. Windows + WSL2 works for cargo / schema / applets work but cannot run mkosi or test the compositor without nested virt.

```sh
# Check the workspace compiles
cargo check --workspace

# Run all tests
cargo test --workspace

# Build a bootable VM image (Linux only; needs mkosi installed)
just qemu
```

See [`justfile`](./justfile) for the full command list.

## Origin

heliOS is the long-form continuation of the **H system** (`D:/dev/H`), a TypeScript/Tauri AI orchestrator. ~70% of H's architecture survives in heliOS: the event bus, entity schema, memory/blackboard/sessions, MCP service surface. What dies is the delivery layer (Tauri, React SPA, Express). What replaces it is a Rust userland top to bottom and Claude Code as the native shell.

See [`docs/research/05-h-reuse-audit.md`](./docs/research/05-h-reuse-audit.md) for the per-package mapping.

## Working name

`heliOS`. Locked at scaffold time; see `PLAN.md` §13 for alternatives. Crate names use `helios-*` prefix; rebranding is a workspace-wide rename.
