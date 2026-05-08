//! heliOS UI WIT world — Phase 0 stub.
//!
//! The actual WIT definition lives in `wit/canvas.wit`. This crate
//! re-exports the WIT source as a string so that build tooling and
//! documentation generators can find one canonical copy. Phase 3 wires
//! `wasmtime::component::bindgen!` against this file.

pub const CANVAS_WIT: &str = include_str!("../wit/canvas.wit");

pub fn placeholder() -> &'static str {
    "helios-ui-wit: phase-0 stub"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wit_is_loaded() {
        assert!(!CANVAS_WIT.is_empty());
        assert!(CANVAS_WIT.contains("world canvas-applet"));
    }
}
