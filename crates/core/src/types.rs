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

/// Platform-independent color for game rendering.
///
/// Named variants use sequential `#[repr(u8)]` discriminants so constrained
/// platforms (C64, GBA) can store colors as a single byte. The terminal
/// renderer maps these to `crossterm::style::Color` via `palette_color()`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum GameColor {
    Black = 0,
    White = 1,
    Grey = 2,
    DarkGrey = 3,
    Red = 4,
    DarkRed = 5,
    Green = 6,
    DarkGreen = 7,
    Yellow = 8,
    DarkBlue = 9,
    Cyan = 10,
    /// Arbitrary RGB color for cases that don't fit a named variant.
    /// Standard-tier only — constrained platforms never construct this.
    Rgb(u8, u8, u8),
}
