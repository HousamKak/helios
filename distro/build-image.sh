#!/usr/bin/env bash
# Build a bootable heliOS disk image.
#
# Steps:
#   1. cargo build --workspace --release
#   2. Stage the four heliOS binaries + systemd units into mkosi.skeleton/
#   3. Run mkosi build to produce distro/helios.raw
#
# Linux-only (mkosi requires Linux). Run on housam-server or any
# Fedora / Arch / Debian host.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DISTRO_DIR="$REPO_ROOT/distro"
SKEL="$DISTRO_DIR/mkosi.skeleton"

log() { printf '\n\033[1;36m[image]\033[0m %s\n' "$*"; }

# ---------------------------------------------------------------------------
# 1. Build heliOS userland (release)
# ---------------------------------------------------------------------------
log "Building heliOS userland (release)"
cd "$REPO_ROOT"
cargo build --workspace --release

# ---------------------------------------------------------------------------
# 2. Stage into mkosi.skeleton
# ---------------------------------------------------------------------------
log "Staging binaries + units into $SKEL"
mkdir -p "$SKEL/usr/local/bin"
mkdir -p "$SKEL/usr/lib/systemd/system"
mkdir -p "$SKEL/var/lib/helios"

for bin in helios-events helios-store helios-mcp helios-shell helios helios-comp; do
    install -m 0755 "$REPO_ROOT/target/release/$bin" "$SKEL/usr/local/bin/$bin"
done

install -m 0644 "$DISTRO_DIR/units/helios-events.service" \
    "$SKEL/usr/lib/systemd/system/helios-events.service"
install -m 0644 "$DISTRO_DIR/units/helios-store.service" \
    "$SKEL/usr/lib/systemd/system/helios-store.service"

# Auto-enable both services by symlink
mkdir -p "$SKEL/usr/lib/systemd/system/multi-user.target.wants"
ln -sf ../helios-events.service \
    "$SKEL/usr/lib/systemd/system/multi-user.target.wants/helios-events.service"
ln -sf ../helios-store.service \
    "$SKEL/usr/lib/systemd/system/multi-user.target.wants/helios-store.service"

# m-2.5.6: enable bluetooth.service so BT keyboards/mice work
# after boot. Pairing is interactive (`bluetoothctl pair <MAC>`)
# until we wire a helios_bluetooth_* MCP tool.
ln -sf /usr/lib/systemd/system/bluetooth.service \
    "$SKEL/usr/lib/systemd/system/multi-user.target.wants/bluetooth.service"

# ---------------------------------------------------------------------------
# 3. mkosi build
# ---------------------------------------------------------------------------
log "Running mkosi build"
cd "$DISTRO_DIR"
mkosi --repository-key-fetch=yes --force build

log "Image built: $DISTRO_DIR/helios.raw"
log "Boot it: cd distro && mkosi qemu"
