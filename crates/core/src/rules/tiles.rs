//! Pure tile display definitions for all capability tiers.
//!
//! This module defines tile display lookups (glyph, color) as `const fn`
//! with explicit exhaustive matches. The standard tier's `Tile` enum and
//! the micro tier's `u8` constants both map through these functions.

use super::color::GameColor;

/// Tile kind discriminant shared across tiers.
///
/// The standard tier's `map::Tile` enum and the micro tier's `TILE_*`
/// constants both correspond to these variants. Use `from_micro()` to
/// convert micro-tier `u8` tile values.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    Wall = 0,
    Structural = 1,
    Floor = 2,
    StairsDown = 3,
}

/// Display glyph for a tile kind.
pub const fn glyph(kind: TileKind) -> char {
    match kind {
        TileKind::Wall => ' ',
        TileKind::Structural => '#',
        TileKind::Floor => '.',
        TileKind::StairsDown => '>',
    }
}

/// Display color for a tile kind.
pub const fn color(kind: TileKind) -> GameColor {
    match kind {
        TileKind::Wall => GameColor::Black,
        TileKind::Structural => GameColor::White,
        TileKind::Floor => GameColor::DarkGrey,
        TileKind::StairsDown => GameColor::Cyan,
    }
}

/// Convert a micro-tier `u8` tile value to `TileKind`.
///
/// Returns `None` for unknown tile values (rendered as blank space).
pub const fn from_micro(tile: u8) -> Option<TileKind> {
    match tile {
        0 => Some(TileKind::Wall),       // TILE_WALL
        1 => Some(TileKind::Structural), // TILE_STRUCTURAL
        2 => Some(TileKind::Floor),      // TILE_FLOOR
        3 => Some(TileKind::StairsDown), // TILE_STAIRS_DOWN
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micro_constants_match_discriminants() {
        // TILE_WALL=0, TILE_STRUCTURAL=1, TILE_FLOOR=2, TILE_STAIRS_DOWN=3.
        assert_eq!(from_micro(0), Some(TileKind::Wall));
        assert_eq!(from_micro(1), Some(TileKind::Structural));
        assert_eq!(from_micro(2), Some(TileKind::Floor));
        assert_eq!(from_micro(3), Some(TileKind::StairsDown));
        assert_eq!(from_micro(255), None);
    }

    #[test]
    fn all_kinds_have_glyph_and_color() {
        for kind in [
            TileKind::Wall,
            TileKind::Structural,
            TileKind::Floor,
            TileKind::StairsDown,
        ] {
            let _ = glyph(kind);
            let _ = color(kind);
        }
    }
}
