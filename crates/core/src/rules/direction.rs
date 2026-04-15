//! Eight-way movement direction shared by all capability tiers.
//!
//! `Direction` is the canonical way to express movement throughout the engine.
//! `GameCommand::Move` and `LookCommand::Move` both carry a `Direction`.
//! The enum is `#[repr(u8)]` so the C64 tier can use discriminant values
//! directly without enum overhead.

use core::mem::size_of;

/// One of eight movement directions. `#[repr(u8)]` for C64/GBA compatibility.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Direction {
    North = 0,
    South = 1,
    East = 2,
    West = 3,
    NorthEast = 4,
    NorthWest = 5,
    SouthEast = 6,
    SouthWest = 7,
}

/// Number of Direction variants — matches `ALL_DIRECTIONS.len()`.
pub const DIRECTION_COUNT: usize = 8;

/// All eight directions, for iteration and property-testing.
pub const ALL_DIRECTIONS: [Direction; DIRECTION_COUNT] = [
    Direction::North,
    Direction::South,
    Direction::East,
    Direction::West,
    Direction::NorthEast,
    Direction::NorthWest,
    Direction::SouthEast,
    Direction::SouthWest,
];

impl Direction {
    /// Convert to `(dx, dy)` offset. North is `(0, -1)`.
    pub const fn to_offset(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::South => (0, 1),
            Direction::East => (1, 0),
            Direction::West => (-1, 0),
            Direction::NorthEast => (1, -1),
            Direction::NorthWest => (-1, -1),
            Direction::SouthEast => (1, 1),
            Direction::SouthWest => (-1, 1),
        }
    }

    /// Return the opposite direction (180° reversal).
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::NorthEast => Direction::SouthWest,
            Direction::NorthWest => Direction::SouthEast,
            Direction::SouthEast => Direction::NorthWest,
            Direction::SouthWest => Direction::NorthEast,
        }
    }

    /// Convert an `ALL_DIRECTIONS` index (0..7) back to a Direction.
    /// Returns `None` if `idx >= DIRECTION_COUNT`.
    pub const fn from_index(idx: u8) -> Option<Direction> {
        if idx < DIRECTION_COUNT as u8 {
            Some(ALL_DIRECTIONS[idx as usize])
        } else {
            None
        }
    }

    /// Convert a `(dx, dy)` offset back to a Direction (normalizes via sign).
    /// Returns `None` for `(0, 0)`.
    ///
    /// Uses branch-based sign extraction instead of `i32::signum()` to avoid
    /// the `G_SCMP` instruction that the 6502 LLVM backend can't legalize.
    pub const fn from_offset(dx: i32, dy: i32) -> Option<Direction> {
        let sx = if dx > 0 { 1 } else if dx < 0 { -1 } else { 0 };
        let sy = if dy > 0 { 1 } else if dy < 0 { -1 } else { 0 };
        match (sx, sy) {
            (0, -1) => Some(Direction::North),
            (0, 1) => Some(Direction::South),
            (1, 0) => Some(Direction::East),
            (-1, 0) => Some(Direction::West),
            (1, -1) => Some(Direction::NorthEast),
            (-1, -1) => Some(Direction::NorthWest),
            (1, 1) => Some(Direction::SouthEast),
            (-1, 1) => Some(Direction::SouthWest),
            _ => None,
        }
    }
}

// Compile-time guarantee: enum fits in a single byte on all tiers.
const _: () = assert!(size_of::<Direction>() == 1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_offset_roundtrips_with_from_offset() {
        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            assert_eq!(Direction::from_offset(dx, dy), Some(dir));
        }
    }

    #[test]
    fn from_offset_zero_is_none() {
        assert_eq!(Direction::from_offset(0, 0), None);
    }

    #[test]
    fn from_offset_normalizes_signum() {
        assert_eq!(Direction::from_offset(5, -3), Some(Direction::NorthEast));
        assert_eq!(Direction::from_offset(-2, 0), Some(Direction::West));
    }

    #[test]
    fn repr_u8_discriminants() {
        assert_eq!(Direction::North as u8, 0);
        assert_eq!(Direction::SouthWest as u8, 7);
    }

    #[test]
    fn opposite_is_involution() {
        for &dir in &ALL_DIRECTIONS {
            assert_eq!(dir.opposite().opposite(), dir);
        }
    }

    #[test]
    fn opposite_reverses_offset() {
        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let (ox, oy) = dir.opposite().to_offset();
            assert_eq!((dx + ox, dy + oy), (0, 0));
        }
    }

    #[test]
    fn from_index_roundtrips_with_discriminant() {
        for &dir in &ALL_DIRECTIONS {
            let idx = dir as u8;
            assert_eq!(Direction::from_index(idx), Some(dir));
        }
    }

    #[test]
    fn from_index_out_of_range() {
        assert_eq!(Direction::from_index(8), None);
        assert_eq!(Direction::from_index(255), None);
    }

    #[test]
    fn all_directions_covers_every_variant() {
        assert_eq!(ALL_DIRECTIONS.len(), 8);
        for &dir in &ALL_DIRECTIONS {
            match dir {
                Direction::North
                | Direction::South
                | Direction::East
                | Direction::West
                | Direction::NorthEast
                | Direction::NorthWest
                | Direction::SouthEast
                | Direction::SouthWest => {}
            }
        }
    }
}
