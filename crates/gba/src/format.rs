//! No-std formatting helpers for GBA display.
//!
//! Primitives and the `GameEvent` formatter live in `roguelike_core::rules::format`
//! so they're shared with other no-alloc ports (NDS). This module re-exports them
//! and keeps the GBA-only helpers (`write_hex` — panic-handler diagnostics).

pub use roguelike_core::rules::format::{format_event, write_str, write_u16};

/// Write a u32 as 8-digit uppercase hexadecimal into `buf` starting at `pos`.
/// Returns the new position after the 8 hex digits. Used by the GBA panic
/// handler to dump SP/LR values on screen.
pub fn write_hex(buf: &mut [u8], pos: usize, val: u32) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut p = pos;
    for i in (0..8).rev() {
        if p < buf.len() {
            buf[p] = HEX[((val >> (i * 4)) & 0xF) as usize];
        }
        p += 1;
    }
    p
}
