//! Micro capability tier — C64 (u8 coords, LFSR-16, Bresenham FOV).
//!
//! A complete no_std game engine in fixed-size arrays, suitable for
//! 8-bit platforms. All state is owned by `MicroGameState` — no
//! static muts, no allocations.

pub mod ai;
pub mod autorun;
pub mod combat;
pub mod entity;
pub mod fov;
pub mod game;
pub mod item_store;
pub mod map;
pub mod msglog;
pub mod prng;
pub mod spawn;
pub mod types;
