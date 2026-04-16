//! No-std formatting helpers for GBA display.
//!
//! Pure re-export of the shared formatters in `roguelike_core::rules::format`.
//! Kept as a thin module so existing `crate::format::*` call sites don't need
//! to change.

pub use roguelike_core::rules::format::{format_event, write_str, write_u16, write_u32_hex};
