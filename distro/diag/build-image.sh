#!/usr/bin/env bash
# Build the heliOS diag image — a bare-metal hardware bringup test.
#
# Purpose: produce the smallest possible bootable image (~80 MB
# compressed) that proves a given machine can boot our kernel +
# systemd-boot + a shell. No heliOS userland, no GUI, no firmware.
#
# Distinct from distro/build-image.sh — that script builds the full
# production image. This one ONLY produces the diag variant.
#
# Linux-only (mkosi requires Linux).

set -euo pipefail

DIAG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log() { printf '\n\033[1;33m[diag]\033[0m %s\n' "$*"; }

log "Building heliOS diag image (no heliOS userland)"
cd "$DIAG_DIR"
mkosi --repository-key-fetch=yes --force build

log "Diag image built: $DIAG_DIR/helios-diag.raw"
log "Flash with Rufus DD mode and boot to verify hardware works."
