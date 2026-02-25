//! Compact-tier type aliases — GBA (i16 coords, u8 stats, 128 entities).

/// Spatial coordinate (i16 for 128x96 maps).
pub type Coord = i16;

/// Combat/health statistic.
pub type Stat = u8;

/// Grid position.
pub type Pos = (Coord, Coord);

/// Maximum entity count (player + monsters).
pub const MAX_ENTITIES: usize = 128;

/// Default map width.
pub const MAP_WIDTH: Coord = 128;

/// Default map height.
pub const MAP_HEIGHT: Coord = 96;
