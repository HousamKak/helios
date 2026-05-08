# heliOS distro

mkosi-driven bootable image build.

> **Linux-only.** mkosi runs on a Linux host (Fedora, Arch, openSUSE all work). Windows / WSL2 cannot run mkosi-the-binary against virtio-gpu without nested virt — set up a Linux dev box or VM for actual image work.

## Files

| Path | Purpose |
|---|---|
| `mkosi.conf` | The image recipe. Fedora 41 base, systemd 256+, UKI, virtio QEMU defaults. |
| `mkosi.skeleton/` | Files copied verbatim into the image rootfs (will be created at first build). |
| `mkosi.cache/` | Build cache (gitignored). |
| `mkosi.output/` | Built images (gitignored). |

## Quickstart

```sh
# From the repo root, on a Linux host with mkosi >= 25 installed:

# Build the heliOS userland (release)
cargo build --workspace --release

# Build the bootable image
cd distro
mkosi build

# Boot in QEMU
mkosi qemu
```

## Iteration loop

Per `docs/research/02-distro-build.md` and `PLAN.md` §7, the target dev
loop is sub-90-seconds edit-to-boot for compositor changes:

1. `cargo build -p helios-comp --release` (sccache-cached, ~10-20s incremental)
2. `mkosi --incremental build` rebuilds only the overlay layer (~20-40s warm)
3. `mkosi qemu` boots a fresh VM (~10-20s)

For pure compositor iteration during Phase 2, *prefer nested-Wayland* —
run `helios-comp` directly in your Linux dev session inside a smithay
nested-Wayland window. ~10-second loop, no VM needed. This is how niri
devs iterate.

## Image model

Phase 0 is **mutable rootfs** (no A/B, no dm-verity). Switch to A/B
immutable via `systemd-sysupdate` happens in Phase 4 once the userland
APIs are stable. See `PLAN.md` §6 Phase 4 deliverables.
