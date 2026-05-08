#!/usr/bin/env bash
# heliOS Linux dev box bootstrap.
#
# Run on a fresh Linux box (Fedora, Ubuntu/Debian, or Arch) to get the
# heliOS development environment ready. Idempotent — re-running is safe.
#
# Installs: rustup + stable + clippy + rustfmt + rust-src + the
# `wasm32-wasip2` target, build tooling, mkosi, qemu, just, sccache,
# nodejs. Clones heliOS plus the reference compositors we'll read from
# during Phase 2 (niri, cosmic-comp, smithay).
#
# Does NOT install NoMachine. Run scripts/install-nomachine.sh
# separately when you actually want visual access from Windows.
#
# Override paths via env:
#   WORK_DIR      — where to clone repos (default: $HOME/code)
#   HELIOS_REMOTE — heliOS git remote (default: ssh form for HousamKak)

set -euo pipefail

WORK_DIR="${WORK_DIR:-$HOME/code}"
HELIOS_REMOTE="${HELIOS_REMOTE:-git@github.com:HousamKak/helios.git}"

log() { printf '\n\033[1;36m[bootstrap]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }

# ---------------------------------------------------------------------------
# 1. Detect distro and choose package commands
# ---------------------------------------------------------------------------
if command -v dnf >/dev/null 2>&1; then
    DISTRO=fedora
    PKG_INSTALL="sudo dnf install -y"
elif command -v apt-get >/dev/null 2>&1; then
    DISTRO=debian
    PKG_INSTALL="sudo apt-get install -y"
    sudo apt-get update -qq
elif command -v pacman >/dev/null 2>&1; then
    DISTRO=arch
    PKG_INSTALL="sudo pacman -S --noconfirm --needed"
else
    echo "Unsupported distro — install required packages manually" >&2
    exit 1
fi
log "Distro: $DISTRO"

# ---------------------------------------------------------------------------
# 2. System packages
# ---------------------------------------------------------------------------
log "Installing system packages"
case "$DISTRO" in
    fedora)
        $PKG_INSTALL git curl wget gcc gcc-c++ make pkg-config openssl-devel \
            qemu-system-x86 qemu-img mkosi systemd-ukify nodejs
        # `just` is in fedora repos
        $PKG_INSTALL just || warn "just package not found; will install via cargo"
        ;;
    debian)
        $PKG_INSTALL git curl wget build-essential pkg-config libssl-dev \
            qemu-system-x86 qemu-utils mkosi nodejs
        # `just` not packaged on older Ubuntus; install via cargo later
        ;;
    arch)
        $PKG_INSTALL git curl wget base-devel openssl qemu-base \
            mkosi just nodejs
        ;;
esac

# ---------------------------------------------------------------------------
# 3. rustup + stable toolchain + targets
# ---------------------------------------------------------------------------
if ! command -v rustup >/dev/null 2>&1; then
    log "Installing rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi
log "Configuring stable toolchain + components + targets"
rustup default stable
rustup component add rustfmt clippy rust-src
rustup target add x86_64-unknown-linux-gnu wasm32-wasip2

# ---------------------------------------------------------------------------
# 4. Cargo-installed tooling we couldn't get from the package manager
# ---------------------------------------------------------------------------
if ! command -v just >/dev/null 2>&1; then
    log "Installing just via cargo"
    cargo install --locked just
fi
if ! command -v sccache >/dev/null 2>&1; then
    log "Installing sccache via cargo"
    cargo install --locked sccache
fi

# Configure sccache as the rustc wrapper so subsequent builds cache.
mkdir -p "$HOME/.cargo"
if ! grep -q "rustc-wrapper" "$HOME/.cargo/config.toml" 2>/dev/null; then
    log "Wiring sccache into ~/.cargo/config.toml"
    cat >> "$HOME/.cargo/config.toml" <<'EOF'

[build]
rustc-wrapper = "sccache"
EOF
fi

# ---------------------------------------------------------------------------
# 5. Clone heliOS + read-only reference compositors
# ---------------------------------------------------------------------------
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

if [ ! -d helios ]; then
    log "Cloning heliOS into $WORK_DIR/helios"
    git clone "$HELIOS_REMOTE" helios
else
    log "heliOS already cloned; skipping"
fi

# Reference repos for Phase 2 reading. Shallow clones to save space.
# Hard-coded HTTPS URLs — these are read-only, no auth needed.
for url in \
    "https://github.com/YaLTeR/niri.git" \
    "https://github.com/pop-os/cosmic-comp.git" \
    "https://github.com/Smithay/smithay.git"
do
    name="$(basename "$url" .git)"
    if [ ! -d "$name" ]; then
        log "Cloning $name (read-only reference)"
        git clone --depth 50 "$url" "$name"
    fi
done

# ---------------------------------------------------------------------------
# 6. Validate the heliOS workspace builds
# ---------------------------------------------------------------------------
log "Running cargo check on heliOS"
cd "$WORK_DIR/helios"
cargo check --workspace --all-targets

# ---------------------------------------------------------------------------
# 7. Done
# ---------------------------------------------------------------------------
log "Bootstrap complete."
echo
echo "  helios:        $WORK_DIR/helios"
echo "  niri:          $WORK_DIR/niri          (Phase 2 reading)"
echo "  cosmic-comp:   $WORK_DIR/cosmic-comp   (Phase 2 reading)"
echo "  smithay:       $WORK_DIR/smithay       (Phase 2 reading)"
echo
echo "Next:"
echo "  cd $WORK_DIR/helios"
echo "  cargo run -p helios-events     # tail process exec/exit on this box"
echo "  cargo test --workspace         # run unit tests"
echo
echo "When you want visual access from Windows:"
echo "  scripts/install-nomachine.sh"
