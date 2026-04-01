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

/// Maximum room count.
pub const MAX_ROOMS: usize = balance::COMPACT_MAX_ROOMS as usize;

/// Total tile count for default map dimensions.
pub const MAP_SIZE: usize = (MAP_WIDTH as usize) * (MAP_HEIGHT as usize);

/// Sentinel: no entity at this position.
pub const NO_ENTITY: u8 = 0xFF;

/// Player is always entity index 0.
pub const PLAYER_IDX: u8 = 0;

/// Maximum floor items.
pub const MAX_ITEMS: usize = balance::COMPACT_MAX_ITEMS as usize;

/// Sentinel: no item at this position.
pub const NO_ITEM: u8 = 0xFF;

/// Bitfield size for visibility/explored sets (one bit per tile).
pub const BITFIELD_SIZE: usize = MAP_SIZE.div_ceil(8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_size_is_product() {
        assert_eq!(MAP_SIZE, (MAP_WIDTH as usize) * (MAP_HEIGHT as usize));
    }

    #[test]
    fn bitfield_covers_map() {
        assert!(BITFIELD_SIZE * 8 >= MAP_SIZE);
    }

    #[test]
    fn player_idx_is_zero() {
        assert_eq!(PLAYER_IDX, 0);
    }

    #[test]
    fn constants_match_balance() {
        assert_eq!(MAX_ENTITIES, balance::COMPACT_MAX_ENTITIES as usize);
        assert_eq!(MAP_WIDTH, balance::COMPACT_MAP_WIDTH as Coord);
        assert_eq!(MAP_HEIGHT, balance::COMPACT_MAP_HEIGHT as Coord);
        assert_eq!(MAX_ROOMS, balance::COMPACT_MAX_ROOMS as usize);
        assert_eq!(MAX_ITEMS, balance::COMPACT_MAX_ITEMS as usize);
    }
}
