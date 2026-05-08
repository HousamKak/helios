# Research 02 — Distro & build system

**Date:** 2026-05-08 · **Source:** background research agent
**Distilled to:** decisions actually used in `PLAN.md`

## Headline

**mkosi + systemd-repart + systemd-sysupdate on a Fedora 41/42 base.** Mutable rootfs for the first 6 months; switch to A/B + dm-verity in Phase 4. Cargo workspace with sccache, UKI + direct-kernel QEMU boot for the dev loop. Dev-loop target: under 90 s edit-to-boot for compositor changes.

## Substrate ranking

- **mkosi (v25+)** — winner. systemd-upstream image builder, TOML-driven, composes a rootfs from any distro + overlays, emits UKI / disk image / container, integrates natively with systemd-repart / systemd-sysupdate / systemd-boot / dm-verity. mkosi-initrd handles initramfs. What systemd itself uses for CI.
- **NixOS + flakes** — strongest reproducibility, declarative system, atomic rollbacks via generations. Downside: Nix is a second language; Nvidia is awkward. Revisit at v0.5.
- **Buildroot / Yocto** — embedded BSP-oriented. Multi-hour BitBake. Wrong for a desktop iteration loop.
- **Alpine, Void, Debian live-build, custom Arch** — all viable substrate-package systems but force you to build a distro framework around them.

## Comparable projects

- **SteamOS 3 (Holo)** — Arch-derived, immutable squashfs, A/B partitions, `steamos-atomupd`. Cleanest atomic-update reference. Copy directly.
- **Fedora bootc / Universal Blue (Bluefin)** — OCI-image-as-OS. The most modern atomic-distro pattern in 2026. Worth strong consideration once we go immutable.
- **System76 COSMIC** — Rust + iced + cosmic-comp. Ships as both a NixOS module and a Pop!_OS Debian package. Their packaging shows Rust userland slots cleanly into multiple distros.
- **postmarketOS** — Alpine-based. Good for low-RAM but musl breaks Nvidia and many proprietary blobs.

## Init system

**systemd 256+, locked.** sd-bus, unit-state API, journal querying, varlink IPC — all of which an LLM agent shell needs to introspect at machine speed. s6 / dinit / runit / OpenRC are leaner but give the agent unstructured logs to parse, which is the wrong direction.

## Hardware & firmware

- Stock upstream stable kernel (6.12 LTS or current Fedora kernel)
- `linux-firmware` package bundled
- Mesa stack from base distro
- Nvidia: `nvidia-open` modules (now default for Turing+); fall back to DKMS
- Don't curate kernel modules — pull from Fedora's package
- Don't ship a custom kernel until you must

## Rust binary packaging

Build dynamically linked against glibc (musl breaks Nvidia/CUDA/blobs). Distribute inside the OS image itself — heliOS binaries are *part of the base*, not packages. Updates ride the A/B image flow once we cross to immutable. Third-party apps ship via Flatpak (Phase 4).

## Dev loop, target sub-2-min

1. Cargo workspace with `sccache` + shared `target/` on tmpfs.
2. mkosi `--incremental` caches the base tree; only the overlay rebuilds.
3. UKI via `ukify`; boot directly with `qemu-system-x86_64 -kernel` + `virtiofsd` mounting build output.
4. Per `mkosi qemu` — bootable VM in 20-40s with warm cache.
5. Optional: `bcachefs` or `erofs` rootfs for fast image creation.

For compositor work specifically, prefer nested-Wayland over VM iteration during Phase 2.
