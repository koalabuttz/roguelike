/// Position or dimension measured in tile units (x, y, width, height, dx, dy).
pub type Coord = i32;

/// A tile position as an (x, y) pair.
pub type Pos = (Coord, Coord);

/// Character or combat statistic (HP, attack, defense, damage).
pub type Stat = i32;

/// Maximum number of entities (player + monsters) the engine supports.
///
/// Constrained platforms override this: GBA = 128, C64 = 16.
/// The wandering spawn system respects this cap alongside `max_wandering`.
pub const MAX_ENTITIES: usize = 1024;

// Re-export GameColor from its canonical home in rules/.
pub use crate::rules::color::GameColor;
