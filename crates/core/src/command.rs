// Re-export from canonical location so existing `use command::{Direction, ...}` paths work.
pub use crate::rules::direction::{ALL_DIRECTIONS, DIRECTION_COUNT, Direction};

/// A platform-independent game command.
///
/// Input adapters (keyboard, controller, replay, network) produce these;
/// game logic consumes them. No module outside `input` should match on
/// raw key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
}
