//! Pure damage calculation shared by all capability tiers.
//!
//! All types are `u8` — fits C64 through PC. The standard tier widens to
//! `Stat` (i32) at the call site; the micro tier uses these directly.

/// Narrow a standard-tier `i32` stat to `u8` for rules-layer functions.
///
/// Negative values clamp to 0; values above 255 clamp to 255.
/// Use this instead of bare `as u8` casts to avoid wrapping surprises.
#[inline]
pub fn narrow(stat: i32) -> u8 {
    stat.clamp(0, u8::MAX as i32) as u8
}

/// Core damage formula: attacker's effective ATK minus defender's effective DEF,
/// clamped to zero. On the 6502 this is literally SEC / SBC / BCS.
pub const fn damage(atk: u8, def: u8) -> u8 {
    atk.saturating_sub(def)
}

/// Effective attack: base stat + weapon bonus.
pub const fn effective_attack(base_atk: u8, weapon_bonus: u8) -> u8 {
    base_atk.saturating_add(weapon_bonus)
}

/// Effective defense: base stat + armor bonus.
pub const fn effective_defense(base_def: u8, armor_bonus: u8) -> u8 {
    base_def.saturating_add(armor_bonus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_damage() {
        assert_eq!(damage(5, 2), 3);
    }

    #[test]
    fn zero_damage_when_equal() {
        assert_eq!(damage(3, 3), 0);
    }

    #[test]
    fn zero_damage_when_defense_exceeds() {
        assert_eq!(damage(2, 5), 0);
    }

    #[test]
    fn max_damage() {
        assert_eq!(damage(255, 0), 255);
    }

    #[test]
    fn effective_attack_adds_bonus() {
        assert_eq!(effective_attack(5, 3), 8);
    }

    #[test]
    fn effective_attack_no_bonus() {
        assert_eq!(effective_attack(5, 0), 5);
    }

    #[test]
    fn effective_attack_saturates() {
        assert_eq!(effective_attack(250, 10), 255);
    }

    #[test]
    fn effective_defense_adds_bonus() {
        assert_eq!(effective_defense(2, 2), 4);
    }

    #[test]
    fn effective_defense_no_bonus() {
        assert_eq!(effective_defense(2, 0), 2);
    }

    #[test]
    fn effective_defense_saturates() {
        assert_eq!(effective_defense(250, 10), 255);
    }

    #[test]
    fn narrow_positive_in_range() {
        assert_eq!(narrow(42), 42);
    }

    #[test]
    fn narrow_zero() {
        assert_eq!(narrow(0), 0);
    }

    #[test]
    fn narrow_negative_clamps_to_zero() {
        assert_eq!(narrow(-5), 0);
    }

    #[test]
    fn narrow_overflow_clamps_to_max() {
        assert_eq!(narrow(300), 255);
    }
}
