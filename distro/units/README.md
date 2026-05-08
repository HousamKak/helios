# systemd units

Service files copied into the heliOS image at build time. Live at `/usr/lib/systemd/system/` inside the running OS.

| Unit | Purpose |
|---|---|
| `helios-events.service` | Phase 1 — events bus daemon. Owns `/run/helios/events.sock`. |
| `helios-store.service` | Phase 1 — entity store. Subscribes to events bus, persists into `/var/lib/helios/store.sqlite`, serves `/run/helios/store.sock`. |

`helios-mcp` is **not** a daemon — it's spawned per-session by Claude Code via the MCP config (`~/.config/claude-code/mcp.json`) that `helios-shell` writes on first login.

`helios-shell` is **not** a daemon either — it's the user's login shell. Configured via PAM / `/etc/passwd` so login execs it instead of bash.

## Startup ordering

```
network-online.target
        │
        ▼
helios-events.service  ──── creates /run/helios/events.sock
        │
        ▼
helios-store.service   ──── connects to events.sock, opens SQLite,
                            creates /run/helios/store.sock

# At login (per user):
helios-shell           ──── writes MCP config, exec claude
                            ↓
                            claude spawns helios-mcp on demand,
                            connects to /run/helios/store.sock
```

## Hardening posture

Per ADR 0002 (local-only telemetry), neither daemon makes outbound network calls. The systemd hardening directives reflect that:

- `ProtectSystem=strict` + explicit `ReadWritePaths` — no writes outside the runtime dir + state dir.
- `ProtectHome=yes` — no access to user homes (we read /proc, not files).
- `NoNewPrivileges=yes` — locks down setuid escalation.
- `CapabilityBoundingSet=CAP_DAC_READ_SEARCH CAP_SYS_PTRACE` (events) — only what `/proc` enrichment needs; future eBPF source will add `CAP_BPF` `CAP_PERFMON`.
