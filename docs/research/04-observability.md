# Research 04 — System events firehose

**Date:** 2026-05-08 · **Source:** background research agent
**Distilled to:** decisions actually used in `PLAN.md` and the
`helios-events` crate

## Headline

A single Rust system service ingests every observable Linux event into one bus. Stack: **aya** for eBPF, **fanotify** or eBPF-LSM for files, **zbus** + **zbus_systemd** for D-Bus, **libsystemd journal tail**, **rtnetlink** + **netlink-packet-sock-diag** for networking. Fan into `tokio::sync::broadcast` (cap 16 384) with bounded mpsc fronts per producer; **postcard** wire format on Unix seqpacket. Budget: 10k events/sec sustained at 3-5% CPU on a 4-core host.

## Recommended stack

| Source | Crate | Version |
|---|---|---|
| Syscalls / exec / TCP / file-LSM | aya + aya-ebpf | 0.13.x |
| Procfs enrichment | procfs | 0.16 |
| File firehose (fallback / non-eBPF kernels) | nix fanotify | 0.29 |
| D-Bus broadcast | zbus | 5.x |
| systemd unit state | zbus_systemd | latest |
| Journal tail | systemd (libsystemd FFI) | 0.10 |
| Netlink route/addr | rtnetlink | 0.14 |
| Socket enumeration | netlink-packet-sock-diag | 0.4 |
| In-proc bus | tokio::sync::broadcast + bounded mpsc fronts | tokio 1.40+ |
| Wire format | postcard | 1.x |

## Key choices, briefly

- **aya over libbpf-rs**: pure-Rust, no libbpf/clang/bcc on target, statically linkable against musl, single CO-RE binary. libbpf-rs forces libelf/libbpf shared deps and a C toolchain.
- **`BPF_MAP_TYPE_RINGBUF`** (Linux ≥5.8), not perf_event_array. ringbuf wins below ~2M evt/s and gives ordered, single-copy delivery.
- **Skip cn_proc**; use eBPF on `sched_process_exec` / `sched_process_exit`. Same latency, no SCM_CREDENTIALS dance, no cn_proc burst-drop issues.
- **Don't poll all of /proc**. Event-drive from eBPF; lazily pull procfs only on first observation of a PID.
- **Files**: prefer eBPF LSM hooks (`security_file_open`, etc) for filesystem-wide, kernel-side filtering. Fanotify (`FAN_MARK_FILESYSTEM`, Linux ≥4.20) as the eBPF-incapable fallback.
- **D-Bus**: zbus (5.x), runtime-agnostic with tokio. dbus-rs is in maintenance mode. Subscribe with raw `MatchRule` + `MessageStream` for firehose mode; typed proxies decode types we don't need.
- **systemd**: `zbus_systemd` for `org.freedesktop.systemd1.Manager` introspection; `systemd::journal::Journal::seek_tail()` + `wait()` on a blocking thread for journald.
- **Network**: `rtnetlink` for link/route/addr; `netlink-packet-sock-diag` for socket enumeration (it's how `ss` works); aya kprobe on `tcp_connect` for low-latency events.

## Architecture for v0.1

One tokio runtime, N source tasks (aya loader, fanotify reader, zbus, journal tailer, rtnetlink, sock_diag). Each pushes a `SystemEvent` enum into a single `tokio::sync::broadcast` for fanout, with a bounded `tokio::sync::mpsc` in front per producer for backpressure (broadcast itself has none). Drop-oldest policy with a `dropped` counter exported as its own meta-event.

## Wire format

- **In-proc**: pass `Arc<SystemEvent>`; don't serialize.
- **IPC** (compositor / agent / applets): postcard over Unix seqpacket, ~150-400 ns/event encode, dependency-free. rkyv has higher raw throughput but its schema-evolution story is awkward for a long-lived bus. Avoid protobuf/prost (5-10× slower, overhead not worth it).

## Per-event budget (v0.1)

- 10k evt/s sustained, 50k burst
- broadcast capacity 16 384, mpsc front 4 096 per producer
- drop-oldest with metric event
- single ringbuf consumer thread per eBPF program — never share a `RingBuf` across tasks
- target 3-5% CPU on a 4-core box (most of that is procfs reads on new-PID enrichment; cache aggressively)

## State of the space, 2026

There is **no canonical "Linux system events bus" Rust crate**. Closest neighbours: Falco's libs (C/C++ with nascent Rust wrappers), Inspektor Gadget (Go core, Rust gadgets via WASM since May 2025), Bottlerocket's host-services (Rust, but device/update-focused). heliOS fills a real infrastructure gap.
