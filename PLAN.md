# AI-Native Linux OS — Master Plan

> Working name: **heliOS** (placeholder — pick one before commit-zero. Other candidates: `H` (continuing the H lineage), `Hearth`, `Vista`-but-no, `Worldspace`. See §13.)

> **Author:** Housam · **Started:** 2026-05-08 · **Target v0.1 release:** 2027-11 (~18 months)
> **Test harness:** QEMU/KVM only until late phase 3.

---

## 0. The pitch in one paragraph

A Linux distribution where the userland is fully custom and Rust-native. The compositor renders one infinite-zoom 2D canvas instead of a desktop with windows; every running process, file in motion, network connection, agent, applet, and project is a visible entity on that canvas. The system shell is **Claude Code itself** — there is no `bash` waiting for you at login, only the agent. The new app primitive is the **applet**: a sandboxed WASM unit that emits a declarative UI tree the compositor renders directly on the canvas, generated on demand by the agent or installed by the user. Linux is the kernel; everything above it — init's user-space, the events firehose, the entity store, the compositor, the applet runtime, the agent shell — is ours.

This is not H extended into an app. **The OS itself is the canvas. H's architecture becomes the userland.**

---

## 1. Non-goals (v0.1)

Cut ruthlessly. Anything below is forbidden until v0.2:

- Microkernel / OS-from-scratch (use Linux)
- Multi-user simultaneous logins
- Mobile / phone / watch / tablet form factors
- HDR, advanced color management, wide-gamut
- Tablet / stylus / accessibility (keyboard + mouse + trackpad only)
- Game-grade Vulkan, VR
- Headless / server use
- Nvidia hybrid-GPU / Optimus, multi-GPU scenarios
- Native app store ecosystem (Flatpak handles legacy)
- Cluster / multi-machine orchestration (single-host only — networking surfaces as entities, but no fleet)

Everything above is technically feasible. None of it is the point of v0.1.

---

## 2. System architecture (top to bottom)

```
┌─────────────────────────────────────────────────────────────┐
│  L9  USER SKIN — themes, applet library, project workspaces  │
├─────────────────────────────────────────────────────────────┤
│  L8  AGENT SHELL — Claude Code as login session              │
├─────────────────────────────────────────────────────────────┤
│  L7  COMPOSITOR — smithay; renders canvas + entities + UI    │
├─────────────────────────────────────────────────────────────┤
│  L6  APPLET RUNTIME — Wasmtime + WIT widget world            │
├─────────────────────────────────────────────────────────────┤
│  L5  MCP GATEWAY — services exposed as MCP servers           │
├─────────────────────────────────────────────────────────────┤
│  L4  SYSTEM SERVICES — entity store, memory, tasks, sessions │
├─────────────────────────────────────────────────────────────┤
│  L3  EVENTS BUS — eBPF + procfs + fanotify + zbus + journal  │
├─────────────────────────────────────────────────────────────┤
│  L2  systemd 256+, varlink, sd-bus, journald, networkd       │
├─────────────────────────────────────────────────────────────┤
│  L1  Linux kernel 6.12 LTS, mesa, linux-firmware             │
├─────────────────────────────────────────────────────────────┤
│  L0  Hardware (generic x86_64, virtio for VM, plain UEFI)    │
└─────────────────────────────────────────────────────────────┘
```

Two cross-cutting buses connect everything: the **system-events bus** (broadcast, lossy, high-throughput, machine-truth) and the **entity store** (durable, queryable, the projection of those events into named rows). The compositor reads entities to know what to draw. The agent reads entities + events to know what's happening. Applets read whatever capabilities they were granted.

---

## 3. Stack decisions (locked unless explicitly revisited)

| Layer | Pick | Rationale |
|---|---|---|
| Kernel | Linux 6.12 LTS (Fedora's kernel package) | Stable, broad hardware. We do not build it. |
| Distro substrate | **mkosi** (TOML-driven), Fedora 41/42 base | Best 2026 image builder; native systemd integration; sub-2-min iteration with `--incremental` + UKI + virtiofsd. |
| Init | **systemd 256+** | Only system with a structured D-Bus / sd-bus / unit-state surface our agent can introspect. Non-negotiable. |
| Image model | Mutable for first 6 months → A/B immutable with dm-verity (SteamOS pattern + bootc) | Ship fast, harden later. |
| Compositor | **smithay 0.7+** (Rust) | Only real Rust Wayland framework. wlroots-rs is dead. |
| Compositor blueprint | **niri** (80%) + cosmic-comp (15%) + everything else (5%) | Niri already proves canvas-zoom + custom GLSL shaders. Cosmic shows scale-up patterns. |
| Renderer | **GLES via Smithay GlesRenderer**; revisit wgpu in 12-18mo | The path everyone ships. wgpu integration in smithay is still a discussion thread. |
| Agent | **Claude Code** (the CLI binary, pinned version) | The native agent IS Claude Code. We don't write an agent runtime. |
| Agent ↔ services | **MCP over stdio** | Claude Code already speaks it; H already exposes its services this way. |
| Applet runtime | **Wasmtime** + pooling allocator + CoW memories + epoch interruption | Reference Component Model implementation. Sub-ms cold start. |
| Applet UI model | **Declarative tree, not pixels** — applet emits a UI tree, host renders | Required to scale to hundreds of applets. Pattern is proven (Figma, Shopify, Anthropic artifacts). |
| Applet UI tree | **Dioxus VirtualDOM** behind a custom WIT `host:ui/canvas` widget world | Cleanest path from a serializable tree to compositor rendering. |
| Applet language | Rust + `cargo component` (installed) · Javy / Rhai (ephemeral, agent-emitted) | Trade compile latency vs sandbox tightness based on origin. |
| Events bus runtime | **aya** for eBPF · zbus for D-Bus · libsystemd journal · rtnetlink + sock_diag · fanotify or eBPF-LSM for files | All best-in-class 2026 Rust crates per category. |
| Events bus wire | **postcard** over Unix seqpacket; `tokio::sync::broadcast` in-proc | Sub-µs encode, ordered, bounded. |
| Entity store | **SQLite + rusqlite + FTS5**, schema lifted from `H/db` | Already an entity graph in H. Don't redesign. |
| Project IDs | Reuse **artifactflow's** project_id namespace | `system-map.html` already names this as the integration spine. |
| Dev host | Linux (Fedora 41+ or Arch). WSL2 acceptable for Rust work but **not** for compositor / VM testing. | Building a Linux OS from native Windows is brutal. |
| Test target | QEMU/KVM, virtio-gpu (gl mode), virtiofsd for source mounts, nested KVM where available | Bare metal only after v0.1 demo recording. |

---

## 4. Components

Twelve components, each ownable by one or more focused build-phase sub-agents. The **owner** column names the kind of agent profile that should drive that component during implementation (these match Claude Code subagent types like `feature-dev:code-architect` or `general-purpose`).

| # | Component | Crate / package name | Layer | Owner profile | First-pass deliverable |
|---|---|---|---|---|---|
| 1 | Distro & build | `helios-distro/` (mkosi config + scripts) | L1-L2 | distro-builder | `mkosi qemu` boots to TTY in <60s |
| 2 | Schema & types | `helios-schema/` Rust crate | L4 (foundation) | code-architect | All H entity types ported as Rust structs + SQL migrations |
| 3 | Events bus | `helios-events/` Rust daemon | L3 | systems-eng | 10k evt/s sustained, Unix-socket subscribers, CLI tail |
| 4 | Entity store | `helios-store/` Rust daemon | L4 | systems-eng | SQLite-backed, projects events into rows, exposes IPC + MCP |
| 5 | MCP gateway | `helios-mcp/` Rust binary | L5 | code-architect | Exposes entity CRUD + tool suite over MCP stdio for Claude Code |
| 6 | Compositor | `helios-comp/` Rust binary (Smithay) | L7 | graphics-eng | Boots on TTY, draws canvas, hosts XWayland apps as entities |
| 7 | Applet runtime | `helios-applets/` Rust daemon (Wasmtime) | L6 | code-architect | Loads `.cwasm`, sandboxes, renders UI trees through compositor |
| 8 | Widget WIT world | `helios-ui-wit/` (WIT defs + host bindings) | L6 | code-architect | `host:ui/canvas` widget interface stable enough to write 3 applets |
| 9 | Agent shell | `helios-shell/` (PAM + systemd-user + Claude Code wrapper) | L8 | systems-eng | Login session = Claude Code, with bus + MCP + tools wired |
| 10 | Skills/hooks/plugins host | `helios-agent-host/` Rust daemon | L8 | code-architect | Re-projects H's `agents` package surface (skills, hooks) as MCP tools |
| 11 | App compat | `helios-compat/` (XWayland glue + Flatpak provisioning) | L7 | systems-eng | Firefox + a terminal launchable, appear as canvas entities |
| 12 | VM test harness | `helios-vm/` (Make/Just + QEMU scripts) | dev | distro-builder | `just qemu` boots, `just qemu-clean` resets, `just demo` runs MVP demo |

Three baseline applets ship in v0.1 to prove the runtime:
- **terminal-applet** — wraps a PTY entity, ships keyboard input/output
- **file-inspector** — opens a file entity, shows metadata + preview
- **log-stream** — subscribes to one event-bus topic and renders as a scrolling list

That's the entire applet library at v0.1. Everything else is "Claude Code can write applets on demand."

---

## 5. H code reuse — full audit

| H package | Verdict | New role |
|---|---|---|
| `events` | **KEEP (concept)** | system-events bus core (Rust port — semantics survive verbatim) |
| `db` | **KEEP (schema)** | userland entity-store schema — SQL migrations port near-verbatim |
| `types` | **KEEP (schema)** | canonical entity & event schema — port to `helios-schema/` Rust crate |
| `memory` | **KEEP** | userland memory service (working / short-term / episodic / semantic) |
| `tasks` | **KEEP** | userland task + DAG service |
| `session` | **KEEP** | session entity service, ties to compositor workspaces |
| `mcp` | **KEEP** | OS-services-as-MCP-servers gateway. Highest reusability surprise. |
| `telegram` | **KEEP** | out-of-band remote-control bridge, runs as a userland Node service |
| `a2a` | **TRANSFORM** | Rust IPC router for spawned Claude Code workers (concept survives, scope shrinks) |
| `tools` | **TRANSFORM** | system-tool surface exposed via MCP (Claude Code already ships most of these) |
| `terminal` | **TRANSFORM** | thin Rust process/PTY service emitting lifecycle events to bus |
| `agents` | **TRANSFORM** | Claude Code lifecycle host + skills/hooks/plugins / compaction / autoDream / memory-extractor preserved as services |
| `orchestrator` | **TRANSFORM** | Rust userland service supervisor — the god-object dies, the wiring survives as systemd-managed daemons |
| `api` | **TRANSFORM** | reference for bus topics; thin HTTP shim for non-OS clients only |
| `llm` | **TRANSFORM** | tiny Rust helper crate for non-CC LLM calls (memory-extractor etc.); CC owns the model layer |
| `cli` | **KILL** | Claude Code IS the shell. No `h` CLI. |
| `desktop` | **KILL** | Compositor replaces Tauri webview. |
| `web` | **KILL** | React SPA replaced by WASM applets. View concepts survive as applet specs. |

**~70% of H's architecture survives.** The biggest transformation is `agents`: H's custom `AgentRuntime` evaporates (Claude Code is the runtime), but skills, hooks, plugins, compaction, memory extractor, and autoDream all become services Claude Code consumes via MCP. The `orchestrator` god-object explodes into per-service systemd units.

The two **highest-leverage Rust ports** to do first, before any compositor work, are:

1. **`events`** — the bus is the spine; every other component publishes/subscribes through it.
2. **`db` + `types`** as a paired port — the schema is already the entity graph.

Both are pure data-plane work. Both can be validated headless. Both unlock everything else.

---

## 6. Phasing — 18-month path to v0.1

Each phase ends with a runnable demo. No phase begins until its predecessor's demo passes.

### Phase 0 — Foundations (Months 1-2)

Goal: bootable image, dev loop working, schema frozen.

- mkosi config producing a Fedora-based bootable image
- Cargo workspace `helios/` with crates for every component (skeleton only)
- `just qemu` boots image, drops to TTY login under 60s
- `helios-schema` crate: every H entity ported to Rust + matching SQL migrations
- `helios-events` skeleton: procfs polling source only (no eBPF yet), broadcast bus, CLI subscriber

**Demo:** boot image, log in, run `helios-events tail` — see process exec/exit events stream.

### Phase 1 — Spine (Months 3-5)

Goal: full events bus, entity store, MCP, Claude Code as the user shell.

- `helios-events` full sources: aya eBPF (exec, file, tcp), zbus, journal, rtnetlink, sock_diag
- `helios-store` Rust daemon: subscribes to bus, projects into SQLite, FTS5 indexed
- `helios-mcp` gateway: entity CRUD + tools (memory, tasks, blackboard, file ops) over MCP stdio
- `helios-shell`: PAM-integrated login session that launches Claude Code with bus + MCP wired
- Pin a Claude Code version, vendor it as part of the image

**Demo:** boot image, log in → Claude Code prompt. Ask "what processes are running?" → it queries the entity store via MCP and answers. Spawn a process; the entity appears in the store and is queryable.

### Phase 2 — Compositor v0 (Months 6-9)

Goal: compositor boots, draws canvas, hosts legacy apps as entities.

- Fork `smithay/anvil`, study `niri` exhaustively, build canvas compositor
- Custom render-element with world-to-screen matrix (port niri's per-window shader pattern)
- Pan + zoom gestures (pointer-gestures-v1 + keyboard)
- XWayland support; legacy clients are entities with bounding boxes
- Compositor subscribes to entity store: place/move/remove triggered by store events
- Inverse-coordinate transform for input — write the test suite first

**Demo:** boot, log in, agent prompt drops you on the canvas. Ask "open firefox" — Claude Code spawns it; it appears as an entity. Pan the canvas with two-finger swipe; zoom with Ctrl-+. One xterm and one Firefox both render and stay interactive.

### Phase 3 — Applet runtime (Months 10-13)

Goal: WASM applets compile, run sandboxed, render on canvas, generated by agent.

- `helios-ui-wit` widget world (button, panel, list, text, scroll, input — minimal but principled)
- `helios-applets` Wasmtime daemon with pooling allocator, pre-compiled `.cwasm` cache, epoch interruption, capability injection per applet manifest
- Compositor renders WIT-tree UIs as native canvas elements
- Three baseline applets: terminal-applet, file-inspector, log-stream
- Agent integration: Claude Code can write a Rust applet, drive `cargo component build`, sign it, install it; agent-emitted-on-demand path uses Javy/Rhai for sub-second iteration

**Demo:** ask agent "show me a CPU graph applet" — it writes the applet, compiles, deploys; live applet appears on canvas, subscribed to events bus, updating in real time. Ask agent "kill firefox" — entity disappears, process gone.

### Phase 4 — Polish & v0.1 release (Months 14-18)

Goal: shippable v0.1 ISO that boots on a clean VM and survives 30 minutes of normal use without crashing.

- Multi-monitor support (defer until last; QEMU virtio-gpu can fake it)
- GPU-side smooth zoom shader between configure snap-points (the niri trick)
- Skills, hooks, plugins re-implemented as MCP-exposed services
- Multi-desktop: workspace entity kind containing other entities; pan-between-desktops UX
- App compat: Flatpak preinstalled, registry of installed apps as entities
- A/B immutable image switch (mkosi → systemd-sysupdate)
- Bootable ISO release; signed UKI
- One-command bare-metal install workflow (still not the day-one priority, but doable)

**v0.1 release demo (the 60-second one):**

1. Clean QEMU VM boots into the compositor — canvas visible, no traditional desktop chrome.
2. Auto-login. Claude Code prompt is the entire UI surface.
3. *"Show me firefox and a terminal."* → Two entities appear; Firefox loads, terminal is interactive.
4. *"Spawn a CPU usage applet."* → Agent generates Rust source, compiles to WASM, deploys. Live graph appears.
5. User pans/zooms canvas — entities transform smoothly, snap to crisp at zoom rest.
6. *"Kill firefox."* → Entity disappears; events bus log shows the exec→exit transition.
7. Cut. That's the OS.

---

## 7. The dev loop, concretely

```
edit Rust →
cargo build --target x86_64-unknown-linux-gnu (sccache cached) →
mkosi --incremental (only overlay rebuilds, ~10-20s) →
ukify combine kernel + initramfs + cmdline →
qemu-system-x86_64 -kernel <UKI> -drive virtiofs source mounted →
boot to TTY (~10s) →
test
```

Target end-to-end iteration: **under 90 seconds for compositor-only changes**. UKI + virtiofsd is the trick that avoids full image rebuilds. Failed boots don't cost anything because nothing is persisted.

For compositor work specifically, also support **nested-Wayland mode**: run `helios-comp` directly in your dev session inside a smithay nested-Wayland window, no VM needed. This is how niri devs iterate. ~10-second loop.

---

## 8. Risks & mitigations, ranked

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | Compositor work eats 12+ months and stalls everything else | High | Catastrophic | Strict scope: niri's patterns, no original graphics work. Phase 2 has a single-screen XWayland-only target. |
| 2 | Hardware support gaps (Nvidia, weird laptops) | High | High | Stay on Fedora's kernel + linux-firmware; defer Nvidia hybrid until v0.2; test on Intel iGPU + AMD discrete only |
| 3 | Decoration without substance — pretty canvas, useless workflows | Medium | Catastrophic | Every feature must have a workflow it makes faster. The MVP demo is the test, not "does it look cool" |
| 4 | Claude Code version drift breaks integration | Medium | High | Pin version in the image; have a "swap CC version" CI job; keep an MCP compatibility shim |
| 5 | Applet UX paradigm ("declarative tree") doesn't actually scale | Medium | Medium | Phase 3 budget includes one explicit "fall back to raw GL surface" path for rich applets |
| 6 | systemd 256+ APIs we depend on get reorganized | Low | Medium | Pin systemd version in image; keep zbus interfaces local; don't lean on bleeding-edge features |
| 7 | Agent-writes-applet loop too slow to feel magical | Medium | Medium | Hybrid path with Javy/Rhai for ephemeral applets — sub-second to spawn |
| 8 | mkosi changes break dev loop | Low | Low | Pin mkosi version; vendor a known-good config |
| 9 | Solo developer burnout from 18-month bottomless project | High | Catastrophic | Phases must each end with a demo. Every demo is shippable as "what I have." Even Phase 1 alone is publishable. |

Risk #9 is honestly the biggest. Mitigate by treating each phase's demo as a real artifact, not a milestone — record it, share it, get reactions. Compounds motivation.

---

## 9. What this OS *is* and what it *isn't* — sharp edges

It **is**:
- A daily-driver desktop Linux distribution for one user (you), where the agent is the primary interface and everything visible runs through one canvas.
- A research vehicle for "what does an OS look like if AI is native, not bolted on?"
- Releasable as an ISO; installable on a real machine eventually.
- Open-source from day one (or kept private until v0.1 — your call).

It **is not**:
- A general-purpose OS for everyone (not yet — v0.1 is one user, your workflow).
- Compatible with everything Linux runs on day one (Nvidia hybrid, weird laptops will break).
- A replacement for desktop environments inside other distros — installing `helios-comp` on stock Fedora is *not* a goal.
- A cluster / multi-machine product (single-host).
- A microkernel research project (Linux is the kernel, fixed).

The reason for these sharp edges is that almost every "AI OS" idea collapses into "AI launcher inside GNOME." The whole differentiator is integration depth — ergo, custom userland top to bottom. Compromise on integration depth and there's no project.

---

## 10. Build-phase sub-agent ownership

When implementation begins, work fans out across these specialist agent profiles, each owning one component and reporting back into the master plan. Claude Code on your dev box runs the agents.

| Profile | Owns | Purpose |
|---|---|---|
| **distro-builder** | Components 1, 12 | mkosi configs, image building, VM scripts, ISO release |
| **systems-eng** | Components 3, 4, 9, 11 | Daemon work, IPC, system bus, shell integration |
| **graphics-eng** | Component 6 | Compositor — solo deep specialist; the project's biggest single risk |
| **code-architect** | Components 2, 5, 7, 8, 10 | Schema, MCP, applet runtime, WIT world, agent host |

Sub-agent prompts for the build phase should each receive: this plan as context, their assigned component's requirements, the H source paths to port from, and an explicit "do not work outside your component" boundary.

---

## 11. Open questions to answer before commit-zero

1. **OS name.** Pick one. `heliOS`, `Hearth`, `H`, `Worldspace`, something else. Affects branding, repo name, internal namespacing (`helios-*` crates throughout this doc are placeholders).
2. **Repo strategy.** One mega-repo (`helios/`) or 12 separate repos with a meta-repo? Recommendation: mega-repo with cargo workspace, switch to multi-repo only if we hire.
3. **License.** AGPL? MIT? Source-available with a delayed open-source clause? Different answers depending on commercial intent.
4. **Hardware test target for v0.1 bare-metal demo.** Pick *one* laptop or desktop model now. Recommendation: a Framework 13 (AMD) or a ThinkPad T14 — Linux-friendly, AMD iGPU, no Nvidia drama.
5. **Where does Claude Code's authentication live?** Environment-baked API key in the image is wrong (security, distribution). systemd-creds or PAM-loaded keychain? Decide before Phase 1 ships.
6. **Image vs installer.** v0.1 ships as a bootable ISO (good for VM demo). Bare-metal install workflow (graphical or minimal text) is Phase 4 polish — confirm scope.
7. **Telemetry.** Collect anything about how you (the user) use it, to learn? If yes, where's the log? If no, document it as a stance.
8. **Public-or-private during build.** Build in public from day one (Twitter / blog posts of every phase demo) vs go heads-down until v0.1. The marketing answer differs from the engineering answer.

---

## 12. The first 30 days

If we go ahead, the first month's work — concrete and self-contained:

- Week 1: Set up Linux dev box (Fedora 41 or Arch); install mkosi, qemu, rustup, sccache, smithay deps; clone niri, cosmic-comp, smithay; read niri's `src/` end-to-end.
- Week 2: Cargo workspace `helios/` with stub crates for all 12 components; mkosi config that builds a Fedora image with a single `hello` binary running on TTY login; `just qemu` working.
- Week 3: Port `H/types` → `helios-schema` Rust crate. Port `H/db/schema.sql` to an `sqlx`-or-`rusqlite` migration set. Validate every entity round-trips.
- Week 4: `helios-events` skeleton with one source (procfs polling for exec/exit). Tokio broadcast bus. CLI tail. Image rebuild + boot validates events stream.

End-of-month deliverable: a Fedora-derived image that boots in QEMU, drops to TTY, runs the events daemon, lets you tail process exec events live. Tiny. Real. Foundational.

---

## 13. Naming — pick one

| Name | Pro | Con |
|---|---|---|
| **heliOS** | Latin "sun"; hints at central + warm + visible-everything; Greek roots play with the canvas-as-world metaphor | Slight clash with "Helios" used elsewhere (PSU brand, photo software) — manageable |
| **H** | Continuation of the H lineage (H grew up to be the OS); short, memorable; matches H's existing branding and the v0.2.216 codebase | Indistinguishable in search; impossible to brand |
| **Hearth** | Warm, home, "where the system lives"; metaphorically right for a personal OS | Slightly cute; fights against the "operating system" gravitas |
| **Worldspace** | Literal: the OS is one navigable world | Sounds like a metaverse product; aged out of fashion |
| **Aperture** | Visibility + opening + camera lens (the canvas as a lens onto the machine) | Portal reference unavoidable |

Recommendation: **heliOS**, with `H` as the namespace prefix (`H-comp`, `H-events`) so the H lineage stays visible in code. Also leaves room for "the H system" to remain the agent layer's sub-name even after the OS gets renamed.

---

## 14. Decision: do we go?

This is an 18-month, four-phase project. The phasing is honest. The risks are named. The H reuse is real (~70% of architecture survives). Each phase ends with a demo that's publishable on its own.

The single biggest determinant of success is whether you can defend Phase 2's compositor scope. Niri-equivalent in 6 months, solo, is hard but doable for someone who already builds graphics. Wider scope and the project hangs forever in compositor-land.

If yes — start with §12. The first 30 days produce something real and decide whether the rest is worth committing to.

---

*End of plan. Living document. Update on every phase boundary.*
