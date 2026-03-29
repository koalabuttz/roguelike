//! Compact-tier type aliases — GBA (i32 coords, u8 stats, 128 entities).
//!
//! Coords are i32 (ARM7-native). The ARM7TDMI is a 32-bit CPU; i16 requires
//! sign-extension (SXTH) on every arithmetic op while i32 is free. The storage
//! savings from i16 are negligible (0.2% of 256 KB EWRAM).

use crate::rules::balance;

/// Spatial coordinate (i32, ARM7-native — no sign-extension overhead).
pub type Coord = i32;

/// Combat/health statistic.
pub type Stat = u8;

/// Grid position.
pub type Pos = (Coord, Coord);

/// Maximum entity count (player + monsters).
pub const MAX_ENTITIES: usize = balance::COMPACT_MAX_ENTITIES as usize;

/// Default map width.
pub const MAP_WIDTH: Coord = balance::COMPACT_MAP_WIDTH as Coord;

/// Default map height.
pub const MAP_HEIGHT: Coord = balance::COMPACT_MAP_HEIGHT as Coord;
