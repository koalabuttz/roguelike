//! Backward-compatible re-export of `GameCommand` from its canonical location
//! in `rules::command`. External crates continue to use `roguelike_core::command::*`.

// Re-export Direction family for existing `use command::{Direction, ...}` paths.
pub use crate::rules::direction::{ALL_DIRECTIONS, DIRECTION_COUNT, Direction};

// Re-export GameCommand from its canonical location.
pub use crate::rules::command::*;
