# Research 08 — The agent → canvas loop

**Date:** 2026-05-09 · **Source:** Phase 2 m-8 server-side iteration
**Distilled to:** how the v0.1 demo flow works end-to-end and how to drive it manually

When all six m-8 chunks land, the heliOS architecture has its first complete bidirectional loop: an agent (Claude Code) calls a tool, and a window visibly slides on the screen. Every Phase 2 milestone before this added a piece. m-8 closes the circuit.

## The flow

```text
Claude Code (agent)
  │  "Move the firefox window to (300, 400)."
  ▼
helios-mcp (MCP gateway, tools.rs)
  │  helios_move_entity(id, 300, 400)
  │  Tool args validated, mapped to StoreRequest::MoveEntity.
  ▼
helios-store (entity store, server.rs handle_move_entity)
  │  ① UPDATE canvas_entities SET x=300, y=400, updated_at=? WHERE id=?
  │  ② Build SystemEvent { source=Tool, payload=EntityPlaced{id,x,y,scale} }
  │  ③ EventsPublisher.publish(&event) → events-ingress.sock
  ▼
helios-events (events bus, socket_ingress.rs serve_ingress)
  │  Reads [u32 BE length][JSON]; pushes onto broadcast channel.
  │  Subscribers (incl. helios-comp's events_client) all receive it.
  ▼
helios-comp (compositor, events_client.rs)
  │  EntityMove { id, world: WorldPoint{x,y} }
  │  → events_rx.try_recv → state.move_entity(id, world)
  │  → Space::map_element at the new world_to_screen position.
  ▼
Next vblank
  │  DrmCompositor::render_frame walks Space, paints firefox at (300,400).
  │  Page flip; the user sees the window at its new location.
```

That's the demo arc. Real OS doing real work driven by the agent.

## Components and their files

| Layer | Crate / file | Role |
|---|---|---|
| Agent surface | `helios-mcp/src/tools.rs` | `helios_move_entity` ToolDef + dispatcher |
| Store request | `helios-schema/src/ipc.rs` | `StoreRequest::MoveEntity` + `StoreResponse::Moved` |
| Store dispatch | `helios-store/src/server.rs::handle_move_entity` | UPDATE + emit |
| Bus producer (store) | `helios-store/src/publisher.rs` | Wraps `EventsPublisher`, shared via Arc |
| Bus client lib | `helios-events/src/publisher.rs` | `EventsPublisher::connect/publish` |
| Bus ingress | `helios-events/src/socket_ingress.rs` | `serve_ingress` accepts publishers |
| Compositor consumer | `helios-comp/src/events_client.rs` | Already existed as of m-5.8 |
| Compositor renderer | `helios-comp/src/wayland.rs::move_entity` | Repositions Window in Space |

## Wire formats

| Hop | Wire format |
|---|---|
| MCP → gateway | JSON-RPC over stdio (`tools/call` request) |
| Gateway → store | Line-delimited JSON over Unix socket |
| Store → bus ingress | `[u32 BE length][JSON SystemEvent]` |
| Bus → subscribers | `[u32 BE length][JSON SystemEvent]` (same encoding, opposite direction) |

## Driving it manually (no image build needed)

Three terminals, plus optionally a fourth as the agent.

```bash
# Terminal 1 — events daemon (the bus)
HELIOS_EVENTS_SOCKET=/tmp/events.sock \
HELIOS_EVENTS_INGRESS_SOCKET=/tmp/events-in.sock \
    cargo run -p helios-events
```

```bash
# Terminal 2 — store daemon (subscribes, projects, accepts moves, relays)
HELIOS_EVENTS_SOCKET=/tmp/events.sock \
HELIOS_EVENTS_INGRESS_SOCKET=/tmp/events-in.sock \
HELIOS_STORE_SOCKET=/tmp/store.sock \
HELIOS_STORE_DB=/tmp/helios-store.sqlite \
    cargo run -p helios-store
```

```bash
# Terminal 3 — compositor (consumes EntityPlaced; emits SurfaceMapped)
HELIOS_EVENTS_SOCKET=/tmp/events.sock \
HELIOS_EVENTS_INGRESS_SOCKET=/tmp/events-in.sock \
HELIOS_XWAYLAND_ENABLED=1 \
    cargo run -p helios-comp
```

Once all three are up:

```bash
# Insert a known entity to move (the demo skips spawning a real client
# because we just want to exercise the loop):
sqlite3 /tmp/helios-store.sqlite "
  INSERT INTO desktops (id, name) VALUES ('d-demo', 'demo');
  INSERT INTO canvas_entities (id, desktop_id, entity_kind, entity_id, x, y, scale)
    VALUES ('e-demo', 'd-demo', 'process', '1234', 0, 0, 1.0);
"

# Issue the move via the store API:
echo '{"op":"move_entity","id":"e-demo","x":300,"y":400}' \
    | socat - UNIX-CONNECT:/tmp/store.sock
# Expected response: {"kind":"moved","ok":true}

# In the compositor's trace: an EntityMove arrives, state.move_entity
# fires, the next frame has an updated screen position for the entity.
```

For a real demo with a visible window, run the compositor with a real Wayland client connected, then resolve its EntityId via:

```bash
echo '{"op":"list_canvas_entities"}' \
    | socat - UNIX-CONNECT:/tmp/store.sock
```

Pick the id of the surface you want to move, then send `move_entity` with that id.

## Running the agent end of the demo

On heliOS (or any host with `claude` available):

```text
> List the canvas entities.
[Claude Code calls helios_list_canvas_entities]

> Move the firefox window (id "01HABC...") to canvas position (300, 400).
[Claude Code calls helios_move_entity(id, 300, 400)]
```

The Firefox window slides on the canvas. That's the demo.

## Architecture invariants that held

Seven milestones in, two more sustained through m-8:

1. **`canvas.rs` / `render.rs` / `state.rs` were not touched.** The m-1 abstractions are the same shape they were when they were committed. m-8 only added new files alongside.
2. **No new daemon.** The publisher is a library helper imported by helios-comp and helios-store; `helios-events` is still the single broadcast hub.

## What's deferred (post-v0.1)

Per the m-8 brief and ADR 0004:

- **Per-publisher capability gating** — anyone with access to the ingress socket can publish. v0.1 = single trusted user.
- **Permissions on the move tool** — the agent can move any entity. Real authorization (which agent owns which window) lands when the project/agent hierarchy is wired through.
- **Compositor-emitted EntityPlaced from user gestures.** Pan/zoom doesn't change per-entity world coords (it's a viewport mutation, no per-entity emission needed). When a future drag-window gesture lands, it'll emit on user-originated moves so the store sees it as the source of truth.
- **Multi-monitor / dmabuf import / canvas chrome / touch / VRR** — explicitly out of m-8 scope; m-9+ polish.

## Tests

| Location | Coverage |
|---|---|
| `helios-events/src/publisher.rs::tests` | Roundtrip from `EventsPublisher::publish` through `serve_ingress` to a broadcast subscriber. |
| `helios-store/tests/integration.rs` | Existing: full DB-side projection + query roundtrip (already passed before m-8). |
| `helios-store/tests/integration_move.rs` | New for m-8.6: `MoveEntity` request → row update → bus emission → subscriber observes EntityPlaced with correct id/x/y/scale. Plus the unknown-id no-emit case. |
| `helios-mcp/src/tools.rs::tests` | `build_store_request("helios_move_entity", …)` happy-path + missing-arg cases. |

The compositor side of the loop is tested manually on a real desktop because reproducing the smithay GLES + wayland-server stack in CI without a display is more infrastructure than the test value justifies.
