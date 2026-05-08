//! heliOS applet runtime — library surface.
//!
//! Phase 0 stub. Per `docs/research/03-wasm-applets.md`:
//!   * Wasmtime with `PoolingAllocationConfig` + CoW memories
//!   * Pre-compiled `.cwasm` modules cached after first install
//!   * Epoch interruption for cooperative preemption
//!   * Capability injection per applet manifest (the WIT imports the host
//!     wires up are the capabilities — `helios_schema::AppletCapability`)
//!   * Applet emits a UI tree through the `host:ui/canvas` WIT world
//!   * Host (compositor via this daemon) renders the tree

pub const DEFAULT_APPLET_DIR: &str = "/var/lib/helios/applets";
pub const DEFAULT_CACHE_DIR: &str = "/var/cache/helios/applets";
pub const DEFAULT_SOCKET_PATH: &str = "/run/helios/applets.sock";

pub fn placeholder() -> &'static str {
    "helios-applets: phase-0 stub"
}
