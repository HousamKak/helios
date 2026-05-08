# Research 05 — H reuse audit

**Date:** 2026-05-08 · **Source:** background research agent reading `D:/dev/H/packages/*`
**Distilled to:** the per-package mapping driving `crates/` layout

## Headline

**~70% of H's architecture survives.** What dies is the delivery layer (Tauri, React SPA, Express API, the `h` CLI). What replaces it is a Rust userland top to bottom and Claude Code as the native shell.

## Per-package classification

| H package | Verdict | New role in heliOS |
|---|---|---|
| `events` | KEEP | system-events bus core (Rust port — semantics survive verbatim) → `crates/events` |
| `db` | KEEP | userland entity-store schema → `crates/schema/migrations/0001_h_origin.sql` |
| `types` | KEEP | canonical entity & event schema → `crates/schema/src/{entities,events,canvas}.rs` |
| `memory` | KEEP | userland memory service (working / short-term / episodic / semantic) |
| `tasks` | KEEP | userland task + DAG service |
| `session` | KEEP | session entity service, ties to compositor desktops |
| `mcp` | KEEP | OS-services-as-MCP-servers gateway → `crates/mcp` |
| `telegram` | KEEP | out-of-band remote-control bridge — runs as a userland Node service |
| `a2a` | TRANSFORM | Rust IPC router for spawned Claude Code workers |
| `tools` | TRANSFORM | system-tool surface exposed via MCP (CC ships most already) |
| `terminal` | TRANSFORM | thin Rust process/PTY service emitting lifecycle events to the bus |
| `agents` | TRANSFORM | Claude Code lifecycle host + skills/hooks/plugins/compaction/autoDream/memory-extractor → `crates/agent-host` |
| `orchestrator` | TRANSFORM | Rust userland service supervisor (god-object dies; wiring becomes systemd-managed daemons) |
| `api` | TRANSFORM | reference for bus topics; thin HTTP shim for non-OS clients only |
| `llm` | TRANSFORM | tiny Rust helper crate for non-CC LLM calls (memory-extractor); CC owns the model layer |
| `cli` | KILL | Claude Code IS the shell. No `h` CLI. |
| `desktop` | KILL | Compositor replaces the Tauri webview. |
| `web` | KILL | React SPA replaced by WASM applets — view *concepts* survive as applet specs, not code. |

## Highest-leverage Rust ports first

1. `events` — the bus is the spine; every other component publishes/subscribes through it.
2. `db` + `types` paired port — the schema is already the entity graph. **Done in scaffold** (`crates/schema/migrations/0001_h_origin.sql` + Rust types in `crates/schema/src/`).

## The biggest transformation

`agents`. H's custom `AgentRuntime` evaporates entirely (Claude Code IS the runtime). What survives are skills, hooks, plugins, compaction, memory-extractor, autoDream — re-projected as MCP-exposed services Claude Code consumes. They become `crates/agent-host`.

## The biggest surprise (positive)

**`mcp` is dramatically more reusable than expected.** H already exposes blackboard, tasks, sessions, memory, files, A2A as MCP tools over stdio — meaning the agent ↔ OS-services boundary is already drawn at exactly the line Claude-Code-as-native-agent needs.

## The biggest surprise (negative)

**`orchestrator/orchestrator.ts` is the most OS-incompatible thing in H.** A 30-field god-object that wires all 17 other packages by hand in a constructor. That pattern cannot survive when each capability becomes an independent userland service. The new "orchestrator" is the systemd unit graph plus the events bus — there is no central process.

## Reference paths in H (for the build phase agents)

- `D:/dev/H/packages/events/src/event-bus.ts`
- `D:/dev/H/packages/db/src/schema.sql`
- `D:/dev/H/packages/db/src/index.ts`
- `D:/dev/H/packages/types/src/index.ts`
- `D:/dev/H/packages/mcp/src/mcp-server.ts`
- `D:/dev/H/packages/agents/src/index.ts`
- `D:/dev/H/packages/orchestrator/src/orchestrator.ts` (anti-pattern reference)
- `D:/dev/H/packages/desktop/src-tauri/src/lib.rs` (KILLed)
- `D:/dev/H/packages/web/src/App.tsx` (KILLed)
