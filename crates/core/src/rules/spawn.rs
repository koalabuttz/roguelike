//! Pure spawn logic shared by all capability tiers.
//!
//! Weighted selection and depth scaling as pure functions. Callers generate
//! the random roll from their tier-specific RNG and pass it in.

use super::balance;
use super::monster_table::AiPersonality;

/// Weighted selection from a parallel weights array.
///
/// `roll`: random value in `[0, total_weight - 1]` (caller generates).
/// `weights`: parallel array of weights (index maps to a kind enum).
///
/// Returns the index of the selected entry. Falls back to 0 if the roll
/// overshoots (shouldn't happen with valid inputs).
pub fn weighted_select(roll: u8, weights: &[u8]) -> usize {
    let mut remaining = roll;
    for (i, &w) in weights.iter().enumerate() {
        if w == 0 {
            continue;
        }
        if remaining < w {
            return i;
        }
        remaining -= w;
    }
    0
}

/// Sum all weights, saturating at 255.
pub fn total_weight(weights: &[u8]) -> u8 {
    let mut total: u8 = 0;
    for &w in weights {
        total = total.saturating_add(w);
    }
    total
}

/// Decide whether a freshly spawned monster rolls Coward.
///
/// `chance`: percent chance (0-100) from [`super::monster_table::coward_chance`].
/// `roll`: random value in `[0, 99]` produced by the caller's RNG.
/// `default`: personality to use when the roll fails (the kind's default).
///
/// Keeping this as a pure function means the RNG stays in the tier layer, so
/// `rules::spawn` remains no-std and tier-portable.
pub fn roll_coward(chance: u8, roll: u8, default: AiPersonality) -> AiPersonality {
    if chance == 0 {
        return default;
    }
    if roll < chance {
        AiPersonality::Coward
    } else {
        default
    }
}

/// Whether a monster's HP is below the flee threshold
/// (`hp * FLEE_THRESHOLD_RECIP < max_hp`). Zero `max_hp` defensively returns
/// false.
pub const fn hp_below_flee_threshold(hp: u8, max_hp: u8) -> bool {
    if max_hp == 0 {
        return false;
    }
    (hp as u16) * (balance::FLEE_THRESHOLD_RECIP as u16) < (max_hp as u16)
}

/// Compute depth-scaled stat bonuses using the default balance constants.
///
/// Returns `(hp_bonus, atk_bonus)` for monsters at the given depth.
/// Depth 1 always returns `(0, 0)`.
pub fn depth_bonus(depth: u8) -> (u8, u8) {
    depth_bonus_custom(
        depth,
        balance::DEPTH_SCALE_INTERVAL,
        balance::MONSTER_HP_PER_FLOOR,
        balance::MONSTER_ATK_PER_FLOOR,
    )
}

/// Compute depth-scaled stat bonuses with custom parameters.
///
/// Standard tier uses this with its configurable `DepthScaling` values.
pub fn depth_bonus_custom(
    depth: u8,
    interval: u8,
    hp_per_floor: u8,
    atk_per_floor: u8,
) -> (u8, u8) {
    if depth <= 1 || interval == 0 {
        return (0, 0);
    }
    let steps = (depth - 1) / interval;
    let hp = hp_per_floor.saturating_mul(steps);
    let atk = atk_per_floor.saturating_mul(steps);
    (hp, atk)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- weighted_select --

    #[test]
    fn selects_first_when_roll_zero() {
        assert_eq!(weighted_select(0, &[60, 30, 10]), 0);
    }

    #[test]
    fn selects_second_at_boundary() {
        assert_eq!(weighted_select(60, &[60, 30, 10]), 1);
    }

    #[test]
    fn selects_last() {
        assert_eq!(weighted_select(90, &[60, 30, 10]), 2);
    }

    #[test]
    fn skips_zero_weights() {
        // Weights: [0, 0, 50, 0, 50]
        assert_eq!(weighted_select(0, &[0, 0, 50, 0, 50]), 2);
        assert_eq!(weighted_select(49, &[0, 0, 50, 0, 50]), 2);
        assert_eq!(weighted_select(50, &[0, 0, 50, 0, 50]), 4);
    }

    #[test]
    fn single_nonzero_always_selected() {
        assert_eq!(weighted_select(0, &[0, 0, 10]), 2);
    }

    #[test]
    fn fallback_on_empty() {
        assert_eq!(weighted_select(0, &[0, 0, 0]), 0);
    }

    // -- total_weight --

    #[test]
    fn sums_weights() {
        assert_eq!(total_weight(&[60, 30, 10]), 100);
    }

    #[test]
    fn saturates_at_255() {
        assert_eq!(total_weight(&[200, 100]), 255);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(total_weight(&[]), 0);
    }

    // -- depth_bonus --

    #[test]
    fn depth_1_is_zero() {
        assert_eq!(depth_bonus(1), (0, 0));
    }

    #[test]
    fn within_first_interval_is_zero() {
        // Depths 2, 3: (d-1)/3 = 0
        assert_eq!(depth_bonus(2), (0, 0));
        assert_eq!(depth_bonus(3), (0, 0));
    }

    #[test]
    fn first_step_at_depth_4() {
        // (4-1)/3 = 1
        assert_eq!(depth_bonus(4), (1, 1));
    }

    #[test]
    fn depth_7_gives_two_steps() {
        // (7-1)/3 = 2
        assert_eq!(depth_bonus(7), (2, 2));
    }

    #[test]
    fn max_depth_22() {
        // (22-1)/3 = 7
        assert_eq!(depth_bonus(22), (7, 7));
    }

    // -- depth_bonus_custom --

    #[test]
    fn custom_interval_zero_returns_zero() {
        assert_eq!(depth_bonus_custom(10, 0, 1, 1), (0, 0));
    }

    #[test]
    fn custom_rates() {
        // interval=5, hp=2, atk=3, depth=11: (11-1)/5 = 2 steps → (4, 6)
        assert_eq!(depth_bonus_custom(11, 5, 2, 3), (4, 6));
    }

    // -- roll_coward --

    #[test]
    fn roll_coward_zero_chance_always_default() {
        for roll in 0..=99u8 {
            assert_eq!(
                roll_coward(0, roll, AiPersonality::Aggressive),
                AiPersonality::Aggressive
            );
        }
    }

    #[test]
    fn roll_coward_full_chance_always_coward() {
        for roll in 0..=99u8 {
            assert_eq!(
                roll_coward(100, roll, AiPersonality::Aggressive),
                AiPersonality::Coward
            );
        }
    }

    #[test]
    fn roll_coward_boundary() {
        // chance=25: rolls 0..24 → Coward, 25..99 → default.
        assert_eq!(
            roll_coward(25, 0, AiPersonality::Aggressive),
            AiPersonality::Coward
        );
        assert_eq!(
            roll_coward(25, 24, AiPersonality::Aggressive),
            AiPersonality::Coward
        );
        assert_eq!(
            roll_coward(25, 25, AiPersonality::Aggressive),
            AiPersonality::Aggressive
        );
        assert_eq!(
            roll_coward(25, 99, AiPersonality::Aggressive),
            AiPersonality::Aggressive
        );
    }

    // -- hp_below_flee_threshold --

    #[test]
    fn hp_threshold_full_hp_not_low() {
        assert!(!hp_below_flee_threshold(6, 6));
    }

    #[test]
    fn hp_threshold_at_third_not_low() {
        // With recip=3: hp*3 < max? 2*3=6 < 6 is false. Exactly at threshold is NOT low.
        assert!(!hp_below_flee_threshold(2, 6));
    }

    #[test]
    fn hp_threshold_below_third_is_low() {
        // 1*3=3 < 6 → true.
        assert!(hp_below_flee_threshold(1, 6));
    }

    #[test]
    fn hp_threshold_zero_max_not_low() {
        assert!(!hp_below_flee_threshold(0, 0));
    }
}
