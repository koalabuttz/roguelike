//! Micro-tier type aliases — C64 (u8 coords, u8 stats, 16 entities).

use crate::rules::balance;

/// Spatial coordinate (u8 for 64x48 maps).
pub type Coord = u8;

/// Combat/health statistic.
pub type Stat = u8;

/// Grid position.
pub type Pos = (Coord, Coord);

/// Maximum entity count (player + monsters).
pub const MAX_ENTITIES: usize = balance::MICRO_MAX_ENTITIES as usize;

/// Map width in tiles.
pub const MAP_WIDTH: Coord = balance::MICRO_MAP_WIDTH;

/// Map height in tiles.
pub const MAP_HEIGHT: Coord = balance::MICRO_MAP_HEIGHT;

/// Total tile count.
pub const MAP_SIZE: usize = (MAP_WIDTH as usize) * (MAP_HEIGHT as usize);

/// Maximum rooms per level.
pub const MAX_ROOMS: usize = balance::MICRO_MAX_ROOMS as usize;

/// Sentinel value for "no entity found".
pub const NO_ENTITY: u8 = 0xFF;

/// Player is always slot 0.
pub const PLAYER_IDX: u8 = 0;

/// Bitfield size in bytes for MAP_SIZE bits.
pub const BITFIELD_SIZE: usize = MAP_SIZE.div_ceil(8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_fit_u8() {
        assert!(MAP_WIDTH > 0);
        assert!(MAP_HEIGHT > 0);
        assert!(MAX_ENTITIES <= 256);
        assert!(MAX_ROOMS <= 256);
    }

    #[test]
    fn map_size_is_product() {
        assert_eq!(MAP_SIZE, 64 * 48);
    }

    #[test]
    fn bitfield_covers_map() {
        assert!(BITFIELD_SIZE * 8 >= MAP_SIZE);
        assert!((BITFIELD_SIZE - 1) * 8 < MAP_SIZE);
    }

    #[test]
    fn player_idx_is_zero() {
        assert_eq!(PLAYER_IDX, 0);
    }
}
