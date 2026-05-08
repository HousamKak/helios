# ADR 0002 — Local-only telemetry

**Status:** Accepted · 2026-05-09
**Author:** Housam
**Related:** `PLAN.md` §11, `crates/events`, `crates/store`

## Context

heliOS is an AI-native OS where the agent reads everything happening on the machine — every process, file, network connection, syscall — to provide useful behavior. We need to record that activity so the agent can reason over it. We also need the user to trust that nothing leaves the machine.

The two pressures (rich data for the agent, zero exfiltration for the user) are compatible — but only if the architecture rules out remote sinks at the design level, not at a config-flag level.

## Decision

**All telemetry is local-only. No phone-home, by construction.**

- Every event the system observes is persisted in the local SQLite store at `/var/lib/helios/store.sqlite`.
- The agent (Claude Code, via MCP) reads from that store. The network sees nothing.
- No remote crash reporters, no opt-in analytics, no feature-flag service that calls home.
- Crash dumps stay on disk under `/var/crash/helios/`. The user uploads them by hand if they file an issue.

H's `AnalyticsRouter` survives in heliOS, but every sink it routes to is a local one. The PII-tagged routing concept — whitelisting events that are allowed to fan out — collapses, because the only fanout target is the on-disk store.

## Consequences

- The events bus and entity store are the *only* telemetry sinks. New sinks require superseding this ADR.
- Network-egress audit is trivial: heliOS userland services should make no outbound HTTP calls except those the user explicitly initiates (e.g. through the agent, or through legacy apps in the compat layer).
- "Send anonymous usage stats" as a future opt-in feature would require its own ADR and an explicit user consent flow.
- Open-source contributors get an unambiguous answer to "does heliOS spy on me?" — no, by construction.

## Reversibility

Reversible by superseding ADR. A future revision could add a single, named, opt-in sink with explicit consent and a clear data schema published in the ADR. The default — off — does not change.

## Out of scope

- The user's own Claude Code session may make calls to Anthropic's API as part of its normal operation. That's the user choosing to use Claude Code; it is not heliOS sending telemetry.
- Plugins, applets, or third-party MCP servers may call out to wherever they want. Capability gating (per-applet manifests) is what scopes this; this ADR governs the heliOS-authored userland only.
