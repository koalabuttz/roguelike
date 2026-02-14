use serde::{Deserialize, Serialize};

/// Position or dimension measured in tile units (x, y, width, height, dx, dy).
pub type Coord = i32;

/// A tile position as an (x, y) pair.
pub type Pos = (Coord, Coord);

/// Character or combat statistic (HP, attack, defense, damage).
pub type Stat = i32;

/// Platform-independent color for game entities.
///
/// Maps to `crossterm::style::Color` in the terminal renderer. Adding a variant
/// here is all that's needed when introducing new monster colors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameColor {
    Yellow,
    Green,
    DarkGreen,
    DarkRed,
}
