#!/usr/bin/env bash
# Install NoMachine server on a Linux box. Detects rpm/deb, downloads
# the latest free-tier package, installs it, and reports the listening
# port (default 4000).
#
# Run after bootstrap-linux-dev.sh, only when you actually want
# graphical access from your Windows machine. Skipped by default
# because NoMachine adds a daemon you don't need until Phase 2.
#
# After install, on Windows:
#   1. Download "NoMachine for Windows" client from nomachine.com
#   2. Add this host (its IP + port 4000)
#   3. Connect with your Linux user credentials
#
# NoMachine free tier is good for one personal user and 6 named hosts.

set -euo pipefail

# Adjust if a newer version is released. Latest as of 2026-05.
NM_VERSION="${NM_VERSION:-9.0.187}"
NM_BUILD="${NM_BUILD:-1}"

log() { printf '\n\033[1;36m[nomachine]\033[0m %s\n' "$*"; }

if command -v nxserver >/dev/null 2>&1 || [ -d /usr/NX ]; then
    log "NoMachine already installed at /usr/NX. Restarting service."
    sudo /etc/NX/nxserver --restart || true
    log "Listening on port 4000."
    exit 0
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64) ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

if command -v rpm >/dev/null 2>&1; then
    PKG_FILE="nomachine_${NM_VERSION}_${NM_BUILD}_x86_64.rpm"
    URL="https://download.nomachine.com/download/${NM_VERSION%.*}/Linux/${PKG_FILE}"
    INSTALL="sudo rpm -i"
elif command -v dpkg >/dev/null 2>&1; then
    PKG_FILE="nomachine_${NM_VERSION}_${NM_BUILD}_amd64.deb"
    URL="https://download.nomachine.com/download/${NM_VERSION%.*}/Linux/${PKG_FILE}"
    INSTALL="sudo dpkg -i"
else
    echo "Need rpm or dpkg" >&2; exit 1
fi

cd /tmp
log "Downloading $URL"
wget -q "$URL"

log "Installing $PKG_FILE"
$INSTALL "$PKG_FILE"

log "Done."
echo
echo "  Server listening on port 4000."
echo "  From Windows, install the NoMachine client and add this host."
echo "  Username: $USER  (use your Linux password)"
echo
echo "Verify the daemon:"
echo "  systemctl status nxserver  # or"
echo "  sudo /etc/NX/nxserver --status"
