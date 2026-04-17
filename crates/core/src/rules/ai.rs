//! Pure AI decision logic shared by all capability tiers.
//!
//! Like [`combat`](super::combat), this module takes scalar inputs and returns
//! small enums. Callers extract positions and map state from their tier-specific
//! storage, call the shared function, and apply results back.

use super::direction::{DIRECTION_COUNT, Direction};
use super::monster_table::AiPersonality;

/// AI behavior mode for a single monster turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMode {
    /// Do nothing (dead, out of sight for Aggressive, or AiPersonality::Player).
    Idle,
    /// Execute chase logic (greedy step toward player).
    Chase,
    /// Execute wander logic (random walk).
    Wander,
    /// Transition from Wander to Chase: caller emits EntityNotice, sets
    /// behavior to Chase, then executes Chase logic.
    WakeUp,
    /// Execute flee logic (greedy step away from player, no attack).
    Flee,
}

/// Result of the chase decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChaseResult {
    /// Monster is adjacent to player — attack.
    Attack,
    /// Move in this direction.
    Move(Direction),
    /// All three candidates are blocked — do nothing.
    Blocked,
}

/// Result of the flee decision. Like [`ChaseResult`] but cannot Attack — a
/// cornered coward stands still rather than trade blows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleeResult {
    /// Move in this direction (away from player).
    Move(Direction),
    /// All three retreat candidates are blocked — cower in place.
    Blocked,
}

/// Decide the AI mode for this turn.
///
/// `aware`: whether the monster can see the player this turn.
/// `hp_low`: whether the monster is below its flee threshold — only consulted
/// for the `Coward` personality, other personalities ignore it.
pub const fn ai_mode(personality: AiPersonality, aware: bool, hp_low: bool) -> AiMode {
    match personality {
        AiPersonality::Aggressive => {
            if aware {
                AiMode::Chase
            } else {
                AiMode::Idle
            }
        }
        AiPersonality::Patrol => {
            if aware {
                AiMode::WakeUp
            } else {
                AiMode::Wander
            }
        }
        AiPersonality::Coward => {
            if aware && hp_low {
                AiMode::Flee
            } else if aware {
                AiMode::Chase
            } else {
                AiMode::Wander
            }
        }
        AiPersonality::Player => AiMode::Idle,
    }
}

/// Chase decision: greedy step toward the player.
///
/// `sx`, `sy`: signum of the delta from monster to player. The caller computes
/// these using their native coord type (i8 on micro, i32 on compact/standard).
///
/// `adjacent`: whether the monster is within 1 tile of the player in both axes.
/// The caller computes this as `|dx| <= 1 && |dy| <= 1`.
///
/// `candidates_passable`: three booleans for the chase candidates in priority
/// order: \[diagonal, horizontal-only, vertical-only\]. The caller computes
/// each by checking `is_walkable(nx, ny) && !is_occupied(nx, ny)` at:
///   - `(mx + sx, my + sy)` — diagonal
///   - `(mx + sx, my)` — horizontal (skipped when `sx == 0`)
///   - `(mx, my + sy)` — vertical (skipped when `sy == 0`)
pub fn chase_step(sx: i32, sy: i32, adjacent: bool, candidates_passable: [bool; 3]) -> ChaseResult {
    if adjacent {
        return ChaseResult::Attack;
    }

    let offsets: [(i32, i32); 3] = [(sx, sy), (sx, 0), (0, sy)];

    for (i, &(cx, cy)) in offsets.iter().enumerate() {
        if cx == 0 && cy == 0 {
            continue;
        }
        if candidates_passable[i] {
            // from_offset on a signum pair always succeeds (non-zero).
            if let Some(dir) = Direction::from_offset(cx, cy) {
                return ChaseResult::Move(dir);
            }
        }
    }

    ChaseResult::Blocked
}

/// Flee decision: greedy step *away* from the player.
///
/// Mirrors [`chase_step`] with offsets negated — candidates are
/// \[`(-sx, -sy)`, `(-sx, 0)`, `(0, -sy)`\]. `adjacent` is not accepted as a
/// parameter because a cornered fleer never attacks; it simply returns
/// `Blocked` when every retreat square is impassable.
///
/// `sx`, `sy`: signum of the delta from monster to player (same as chase).
/// `candidates_passable`: three booleans matching the inverted offsets above.
pub fn flee_step(sx: i32, sy: i32, candidates_passable: [bool; 3]) -> FleeResult {
    let offsets: [(i32, i32); 3] = [(-sx, -sy), (-sx, 0), (0, -sy)];

    for (i, &(cx, cy)) in offsets.iter().enumerate() {
        if cx == 0 && cy == 0 {
            continue;
        }
        if candidates_passable[i]
            && let Some(dir) = Direction::from_offset(cx, cy)
        {
            return FleeResult::Move(dir);
        }
    }

    FleeResult::Blocked
}

/// Wander decision: pick a random passable neighbor.
///
/// `passable_mask`: bit `i` set means `ALL_DIRECTIONS[i]` is a valid target
/// (walkable, unoccupied, not the player tile). The caller builds this mask.
///
/// `roll`: random value in `[0, popcount(passable_mask) - 1]`, generated by
/// the caller's RNG. Ignored when the mask is empty.
///
/// Returns the direction to move, or `None` if no passable neighbors.
pub fn wander_step(passable_mask: u8, roll: u8) -> Option<Direction> {
    if passable_mask == 0 {
        return None;
    }

    let mut remaining = roll;
    for i in 0..DIRECTION_COUNT as u8 {
        if passable_mask & (1 << i) != 0 {
            if remaining == 0 {
                return Direction::from_index(i);
            }
            remaining -= 1;
        }
    }

    // Fallback: shouldn't happen with valid inputs, but return first set bit.
    for i in 0..DIRECTION_COUNT as u8 {
        if passable_mask & (1 << i) != 0 {
            return Direction::from_index(i);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::direction::ALL_DIRECTIONS;

    // -- ai_mode tests --

    #[test]
    fn aggressive_aware_returns_chase() {
        assert_eq!(
            ai_mode(AiPersonality::Aggressive, true, false),
            AiMode::Chase
        );
        // hp_low is ignored for aggressive personalities.
        assert_eq!(
            ai_mode(AiPersonality::Aggressive, true, true),
            AiMode::Chase
        );
    }

    #[test]
    fn aggressive_unaware_returns_idle() {
        assert_eq!(
            ai_mode(AiPersonality::Aggressive, false, false),
            AiMode::Idle
        );
        assert_eq!(
            ai_mode(AiPersonality::Aggressive, false, true),
            AiMode::Idle
        );
    }

    #[test]
    fn patrol_aware_returns_wakeup() {
        assert_eq!(ai_mode(AiPersonality::Patrol, true, false), AiMode::WakeUp);
    }

    #[test]
    fn patrol_unaware_returns_wander() {
        assert_eq!(ai_mode(AiPersonality::Patrol, false, false), AiMode::Wander);
    }

    #[test]
    fn player_always_idle() {
        assert_eq!(ai_mode(AiPersonality::Player, true, false), AiMode::Idle);
        assert_eq!(ai_mode(AiPersonality::Player, false, false), AiMode::Idle);
        assert_eq!(ai_mode(AiPersonality::Player, true, true), AiMode::Idle);
    }

    #[test]
    fn coward_healthy_aware_chases() {
        assert_eq!(ai_mode(AiPersonality::Coward, true, false), AiMode::Chase);
    }

    #[test]
    fn coward_hurt_aware_flees() {
        assert_eq!(ai_mode(AiPersonality::Coward, true, true), AiMode::Flee);
    }

    #[test]
    fn coward_unaware_wanders_regardless_of_hp() {
        assert_eq!(ai_mode(AiPersonality::Coward, false, false), AiMode::Wander);
        assert_eq!(ai_mode(AiPersonality::Coward, false, true), AiMode::Wander);
    }

    // -- chase_step tests --

    #[test]
    fn adjacent_returns_attack() {
        assert_eq!(
            chase_step(1, 1, true, [true, true, true]),
            ChaseResult::Attack
        );
        assert_eq!(
            chase_step(0, 0, true, [true, true, true]),
            ChaseResult::Attack
        );
        assert_eq!(
            chase_step(-1, 0, true, [true, true, true]),
            ChaseResult::Attack
        );
    }

    #[test]
    fn diagonal_preferred() {
        let result = chase_step(1, 1, false, [true, true, true]);
        assert_eq!(result, ChaseResult::Move(Direction::SouthEast));
    }

    #[test]
    fn horizontal_fallback() {
        let result = chase_step(1, 1, false, [false, true, true]);
        assert_eq!(result, ChaseResult::Move(Direction::East));
    }

    #[test]
    fn vertical_fallback() {
        let result = chase_step(1, 1, false, [false, false, true]);
        assert_eq!(result, ChaseResult::Move(Direction::South));
    }

    #[test]
    fn all_blocked() {
        let result = chase_step(1, 1, false, [false, false, false]);
        assert_eq!(result, ChaseResult::Blocked);
    }

    #[test]
    fn negative_deltas() {
        let result = chase_step(-1, -1, false, [true, true, true]);
        assert_eq!(result, ChaseResult::Move(Direction::NorthWest));
    }

    #[test]
    fn horizontal_only_chase() {
        // sy == 0: candidates are (1,0), (1,0), (0,0).
        let result = chase_step(1, 0, false, [true, true, true]);
        assert_eq!(result, ChaseResult::Move(Direction::East));
    }

    #[test]
    fn vertical_only_chase() {
        // sx == 0: candidates are (0,1), (0,0), (0,1).
        let result = chase_step(0, 1, false, [true, true, true]);
        assert_eq!(result, ChaseResult::Move(Direction::South));
    }

    // -- wander_step tests --

    #[test]
    fn empty_mask_returns_none() {
        assert_eq!(wander_step(0, 0), None);
    }

    #[test]
    fn single_bit_returns_that_direction() {
        for i in 0..DIRECTION_COUNT as u8 {
            let mask = 1 << i;
            let result = wander_step(mask, 0);
            assert_eq!(result, Direction::from_index(i));
        }
    }

    #[test]
    fn roll_selects_nth_set_bit() {
        // Mask with bits 0, 2, 5 set (North, East, NorthWest).
        let mask: u8 = (1 << 0) | (1 << 2) | (1 << 5);
        assert_eq!(wander_step(mask, 0), Some(ALL_DIRECTIONS[0])); // North
        assert_eq!(wander_step(mask, 1), Some(ALL_DIRECTIONS[2])); // East
        assert_eq!(wander_step(mask, 2), Some(ALL_DIRECTIONS[5])); // NorthWest
    }

    #[test]
    fn full_mask_all_rolls() {
        let mask: u8 = 0xFF;
        for i in 0..DIRECTION_COUNT as u8 {
            assert_eq!(wander_step(mask, i), Some(ALL_DIRECTIONS[i as usize]));
        }
    }

    #[test]
    fn type_sizes_bounded() {
        assert!(core::mem::size_of::<AiMode>() <= 1);
        assert!(core::mem::size_of::<ChaseResult>() <= 2);
        assert!(core::mem::size_of::<FleeResult>() <= 2);
    }

    // -- flee_step tests --

    #[test]
    fn flee_diagonal_preferred() {
        // Player is SE (sx=1, sy=1). Flee should go NW via (-1,-1).
        let result = flee_step(1, 1, [true, true, true]);
        assert_eq!(result, FleeResult::Move(Direction::NorthWest));
    }

    #[test]
    fn flee_horizontal_fallback() {
        let result = flee_step(1, 1, [false, true, true]);
        assert_eq!(result, FleeResult::Move(Direction::West));
    }

    #[test]
    fn flee_vertical_fallback() {
        let result = flee_step(1, 1, [false, false, true]);
        assert_eq!(result, FleeResult::Move(Direction::North));
    }

    #[test]
    fn flee_all_blocked() {
        // A cornered coward stands still rather than attack.
        assert_eq!(flee_step(1, 1, [false, false, false]), FleeResult::Blocked);
    }

    #[test]
    fn flee_negative_deltas() {
        // Player is NW (sx=-1, sy=-1). Flee should go SE via (+1,+1).
        let result = flee_step(-1, -1, [true, true, true]);
        assert_eq!(result, FleeResult::Move(Direction::SouthEast));
    }

    #[test]
    fn flee_horizontal_only() {
        // sy == 0: candidates (-1,0), (-1,0), (0,0). Second filtered by cx==0 check.
        let result = flee_step(1, 0, [true, true, true]);
        assert_eq!(result, FleeResult::Move(Direction::West));
    }

    #[test]
    fn flee_vertical_only() {
        // sx == 0: candidates (0,-1), (0,0), (0,-1). Middle filtered.
        let result = flee_step(0, 1, [true, true, true]);
        assert_eq!(result, FleeResult::Move(Direction::North));
    }
}
