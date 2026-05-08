#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
SKEL="$REPO/distro/mkosi.skeleton"

echo "[1] Staging binaries..."
mkdir -p "$SKEL/usr/local/bin" "$SKEL/usr/lib/systemd/system" "$SKEL/var/lib/helios" "$SKEL/usr/lib/systemd/system/multi-user.target.wants"
for bin in helios-events helios-store helios-mcp helios-shell helios; do
    install -m 0755 "$REPO/target/release/$bin" "$SKEL/usr/local/bin/$bin"
done
install -m 0644 "$REPO/distro/units/helios-events.service" "$SKEL/usr/lib/systemd/system/"
install -m 0644 "$REPO/distro/units/helios-store.service" "$SKEL/usr/lib/systemd/system/"
ln -sf ../helios-events.service "$SKEL/usr/lib/systemd/system/multi-user.target.wants/"
ln -sf ../helios-store.service "$SKEL/usr/lib/systemd/system/multi-user.target.wants/"
echo "[1] Done."

echo "[2] Running mkosi build (needs sudo)..."
cd "$REPO/distro"
sudo mkosi --repository-key-fetch=yes --force build
echo "[2] Done: $REPO/distro/helios.raw"
