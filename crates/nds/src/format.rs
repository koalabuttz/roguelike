//! No-std formatting helpers for NDS HUD display.
//!
//! Re-exports the shared formatter from `roguelike_core::rules::format`. The NDS
//! screen is 32 columns wide; callers pass `&mut [u8; 32]` buffers which coerce
//! to the `&mut [u8]` slice the shared functions accept.

pub use roguelike_core::rules::format::{format_event, write_str, write_u16};
