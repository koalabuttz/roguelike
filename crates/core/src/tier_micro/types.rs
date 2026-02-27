//! Micro-tier type aliases — C64 (u8 coords, u8 stats, 64 entities).

use crate::rules::balance;

/// Spatial coordinate (u8 for up to 80x60 maps).
pub type Coord = u8;

/// Combat/health statistic.
pub type Stat = u8;

/// Grid position.
pub type Pos = (Coord, Coord);

/// Maximum entity count (player + monsters).
pub const MAX_ENTITIES: usize = balance::MICRO_MAX_ENTITIES as usize;

/// Maximum map width in tiles (array sizing).
pub const MAX_MAP_WIDTH: Coord = balance::MICRO_MAX_MAP_WIDTH;

/// Maximum map height in tiles (array sizing).
pub const MAX_MAP_HEIGHT: Coord = balance::MICRO_MAX_MAP_HEIGHT;

/// Default map width (C64 default).
pub const DEFAULT_MAP_WIDTH: Coord = balance::MICRO_MAP_WIDTH;

/// Default map height (C64 default).
pub const DEFAULT_MAP_HEIGHT: Coord = balance::MICRO_MAP_HEIGHT;

/// Total tile count at maximum dimensions.
pub const MAX_MAP_SIZE: usize = (MAX_MAP_WIDTH as usize) * (MAX_MAP_HEIGHT as usize);

/// Maximum rooms per level.
pub const MAX_ROOMS: usize = balance::MICRO_MAX_ROOMS as usize;

/// Sentinel value for "no entity found".
pub const NO_ENTITY: u8 = 0xFF;

/// Player is always slot 0.
pub const PLAYER_IDX: u8 = 0;

/// Bitfield size in bytes for MAX_MAP_SIZE bits.
pub const MAX_BITFIELD_SIZE: usize = MAX_MAP_SIZE.div_ceil(8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_fit_u8() {
        assert!(MAX_MAP_WIDTH > 0);
        assert!(MAX_MAP_HEIGHT > 0);
        assert!(MAX_ENTITIES <= 256);
        assert!(MAX_ROOMS <= 256);
    }

    #[test]
    fn max_map_size_is_product() {
        assert_eq!(MAX_MAP_SIZE, 80 * 60);
    }

    #[test]
    fn default_dims_within_max() {
        assert!(DEFAULT_MAP_WIDTH <= MAX_MAP_WIDTH);
        assert!(DEFAULT_MAP_HEIGHT <= MAX_MAP_HEIGHT);
    }

    #[test]
    fn bitfield_covers_map() {
        assert!(MAX_BITFIELD_SIZE * 8 >= MAX_MAP_SIZE);
        assert!((MAX_BITFIELD_SIZE - 1) * 8 < MAX_MAP_SIZE);
    }

    #[test]
    fn player_idx_is_zero() {
        assert_eq!(PLAYER_IDX, 0);
    }
}
