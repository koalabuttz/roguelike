//! Compact capability tier — GBA (i32 coords, LFSR-32, fixed arrays).
//!
//! Built from standard tier patterns with ARM7-native i32 coordinates,
//! unpacked u8 tile storage, and no heap allocation. Phase 0 of the GBA port.

pub mod ai;
pub mod autorun;
pub mod combat;
pub mod entity;
pub mod fov;
pub mod game;
pub mod item_store;
pub mod map;
pub mod msglog;
pub mod pathfinding;
pub mod prng;
pub mod spawn;
pub mod types;
