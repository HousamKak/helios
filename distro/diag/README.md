# heliOS diag image

A **separate**, **stripped-down** image used purely to verify that a given
machine can boot a Linux kernel at all. Has no connection to the
production heliOS image except sharing the Fedora 41 base.

## Why this exists

Bare-metal bringup on unfamiliar hardware (Kamrui mini PCs, random
laptops, etc.) frequently fails for reasons that look like "heliOS
broke" but are actually firmware, BIOS, or GPU driver issues that any
Linux distribution would hit.

This image strips heliOS down to: **kernel + systemd-boot + bash**.

- ~80 MB compressed (vs ~1.2 GB for the production heliOS image)
- ~30 second download instead of ~5-8 minutes
- No GPU init (`nomodeset` + `i915.modeset=0`)
- Verbose kernel cmdline (`debug loglevel=7`, no `quiet`)
- Halts visibly on panic instead of silently rebooting

If the diag image boots and the production heliOS image doesn't, the
difference is in heliOS's config (mkosi packages, kernel cmdline, UKI
parameters). If neither boots, the machine itself has a hardware or
firmware problem unrelated to heliOS.

## What's NOT in this image

- No `linux-firmware` (~1 GB saved — skips GPU/WiFi/BT firmware blobs)
- No `mesa-*` (GPU userspace libraries)
- No `xorg-x11-server-Xwayland`
- No `foot` terminal
- No `NetworkManager`, `bluez`, `bluez-tools`
- No `htop`, `strace`, `socat`
- No heliOS binaries (helios-events, helios-store, helios-comp, …)

## Build (Linux host)

```sh
bash distro/diag/build-image.sh
# → produces distro/diag/helios-diag.raw (~250 MB)
# → and    distro/diag/helios-diag.raw.zst (~80 MB)
```

## Flash and boot

Flash `helios-diag.raw` to a USB drive with Rufus (DD mode) or `dd`.
Boot the target machine from the USB. Expected outcome:

1. systemd-boot menu briefly visible
2. Kernel boots, scrolling messages in plain text mode (no GPU)
3. Lands at `[root@fedora ~]#` with autologin
4. Done — hardware is confirmed bootable

If anything before step 4 fails, that's where the machine's actual
problem is.
