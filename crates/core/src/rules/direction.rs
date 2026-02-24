//! Eight-way movement direction shared by all capability tiers.
//!
//! `Direction` is the canonical way to express movement throughout the engine.
//! `GameCommand::Move` and `LookCommand::Move` both carry a `Direction`.
//! The enum is `#[repr(u8)]` so the C64 tier can use discriminant values
//! directly without enum overhead.

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

/// All eight directions, for iteration and property-testing.
pub const ALL_DIRECTIONS: [Direction; 8] = [
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

    /// Convert a signum'd `(dx, dy)` offset back to a Direction.
    /// Returns `None` for `(0, 0)` or non-unit offsets.
    pub const fn from_offset(dx: i32, dy: i32) -> Option<Direction> {
        // Manual sign extraction instead of signum() for const compatibility.
        let sx = if dx > 0 {
            1
        } else if dx < 0 {
            -1
        } else {
            0
        };
        let sy = if dy > 0 {
            1
        } else if dy < 0 {
            -1
        } else {
            0
        };
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
