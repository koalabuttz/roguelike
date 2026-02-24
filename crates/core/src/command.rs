use serde::{Deserialize, Serialize};

/// One of eight movement directions. `#[repr(u8)]` for C64/GBA compatibility.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub fn from_offset(dx: i32, dy: i32) -> Option<Direction> {
        match (dx.signum(), dy.signum()) {
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

/// A platform-independent game command.
///
/// Input adapters (keyboard, controller, replay, network) produce these;
/// game logic consumes them. No module outside `input` should match on
/// raw key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameCommand {
    Move(Direction),
    /// Keep moving in a direction until something interesting happens.
    Autorun(Direction),
    AutoExplore,
    Descend,
    Look,
    Wait,
    Quit,
    Help,
}

impl GameCommand {
    /// Convert an `(dx, dy)` offset to `Move(dir)`, or `Wait` if `(0, 0)`.
    /// Handles the Direction/Wait boundary so callers don't need `unwrap_or`.
    pub fn move_or_wait(dx: i32, dy: i32) -> GameCommand {
        match Direction::from_offset(dx, dy) {
            Some(dir) => GameCommand::Move(dir),
            None => GameCommand::Wait,
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
        // Larger offsets get normalized via signum
        assert_eq!(Direction::from_offset(5, -3), Some(Direction::NorthEast));
        assert_eq!(Direction::from_offset(-2, 0), Some(Direction::West));
    }

    #[test]
    fn repr_u8_discriminants() {
        assert_eq!(Direction::North as u8, 0);
        assert_eq!(Direction::SouthWest as u8, 7);
    }

    #[test]
    fn move_or_wait_returns_move_for_direction() {
        assert_eq!(
            GameCommand::move_or_wait(1, 0),
            GameCommand::Move(Direction::East)
        );
        assert_eq!(
            GameCommand::move_or_wait(-1, -1),
            GameCommand::Move(Direction::NorthWest)
        );
    }

    #[test]
    fn move_or_wait_returns_wait_for_zero() {
        assert_eq!(GameCommand::move_or_wait(0, 0), GameCommand::Wait);
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
