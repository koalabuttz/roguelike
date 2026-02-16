use serde::{Deserialize, Serialize};

use crate::types::Coord;

/// A platform-independent game command.
///
/// Input adapters (keyboard, controller, replay, network) produce these;
/// game logic consumes them. No module outside `input` should match on
/// raw key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameCommand {
    Move {
        dx: Coord,
        dy: Coord,
    },
    /// Keep moving in a direction until something interesting happens.
    Autorun {
        dx: Coord,
        dy: Coord,
    },
    AutoExplore,
    Wait,
    Quit,
}
