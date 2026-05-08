# heliOS — task runner
# Install `just`: cargo install just
# List tasks: just --list

set windows-shell := ["pwsh", "-NoLogo", "-NoProfile", "-Command"]

# Default: list available recipes
default:
    @just --list

# Compile-check the entire workspace
check:
    cargo check --workspace --all-targets

# Build all crates in release mode
build:
    cargo build --workspace --release

# Run all unit + integration tests
test:
    cargo test --workspace --all-targets

# Format all Rust code
fmt:
    cargo fmt --all

# Lint with clippy, treat warnings as errors
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Clean build artifacts
clean:
    cargo clean

# Boot the heliOS image in QEMU (Linux-only — requires mkosi)
qemu:
    @echo "Linux-only. Run from a Fedora/Arch host:"
    @echo "  cd distro && mkosi qemu"

# Build the bootable image (Linux-only)
image:
    @echo "Linux-only. Run from a Fedora/Arch host:"
    @echo "  cd distro && mkosi build"

# Run the v0.1 demo script in a fresh VM (later phases)
demo:
    @echo "Demo not yet wired. See PLAN.md §6 — Phase 4 deliverable."

# Open the master plan
plan:
    @code PLAN.md || notepad PLAN.md
