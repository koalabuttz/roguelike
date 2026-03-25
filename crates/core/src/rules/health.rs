//! Qualitative health tier descriptions (DCSS-style).
//!
//! Pure `no_std` functions mapping HP percentages to human-readable
//! health descriptions. Used by look mode, combat log threshold
//! messages, and the C64 status bar.

/// Qualitative health tier, derived from current HP as a percentage of max HP.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthTier {
    /// >75% HP.
    Healthy = 0,
    /// 40–75% HP.
    Moderate = 1,
    /// 15–40% HP.
    Severe = 2,
    /// >0% and ≤15% HP.
    AlmostDead = 3,
}

/// Compute the health tier from current and max HP.
///
/// Uses multiplication instead of division to avoid pulling in
/// `__udivhi3` (192 bytes) on 6502. `hp > N%` is equivalent to
/// `hp * D > max_hp * N` for appropriate D, N constants.
pub const fn health_tier(hp: u8, max_hp: u8) -> HealthTier {
    if max_hp == 0 || hp == 0 {
        return HealthTier::AlmostDead;
    }
    let h = hp as u16;
    let m = max_hp as u16;
    if h * 4 > m * 3 {
        // hp > 75%
        HealthTier::Healthy
    } else if h * 5 > m * 2 {
        // hp > 40%
        HealthTier::Moderate
    } else if h * 20 > m * 3 {
        // hp > 15%
        HealthTier::Severe
    } else {
        HealthTier::AlmostDead
    }
}

/// Human-readable description for a health tier (standard tier messages).
pub const fn health_description(tier: HealthTier) -> &'static str {
    match tier {
        HealthTier::Healthy => "healthy",
        HealthTier::Moderate => "moderately damaged",
        HealthTier::Severe => "severely wounded",
        HealthTier::AlmostDead => "almost dead",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hp_is_healthy() {
        assert_eq!(health_tier(20, 20), HealthTier::Healthy);
    }

    #[test]
    fn boundary_76_percent_healthy() {
        // 76% → Healthy (just above 75% threshold)
        assert_eq!(health_tier(76, 100), HealthTier::Healthy);
    }

    #[test]
    fn boundary_75_percent_moderate() {
        // 75% → Moderate (at threshold, not above)
        assert_eq!(health_tier(75, 100), HealthTier::Moderate);
    }

    #[test]
    fn boundary_41_percent_moderate() {
        assert_eq!(health_tier(41, 100), HealthTier::Moderate);
    }

    #[test]
    fn boundary_40_percent_severe() {
        assert_eq!(health_tier(40, 100), HealthTier::Severe);
    }

    #[test]
    fn boundary_16_percent_severe() {
        assert_eq!(health_tier(16, 100), HealthTier::Severe);
    }

    #[test]
    fn boundary_15_percent_almost_dead() {
        assert_eq!(health_tier(15, 100), HealthTier::AlmostDead);
    }

    #[test]
    fn one_hp_almost_dead() {
        assert_eq!(health_tier(1, 100), HealthTier::AlmostDead);
    }

    #[test]
    fn zero_hp_almost_dead() {
        assert_eq!(health_tier(0, 20), HealthTier::AlmostDead);
    }

    #[test]
    fn zero_max_hp_almost_dead() {
        assert_eq!(health_tier(0, 0), HealthTier::AlmostDead);
    }

    #[test]
    fn small_max_hp_tiers() {
        // max_hp=4: 4/4=100% Healthy, 3/4=75% Moderate, 2/4=50% Moderate,
        //           1/4=25% Severe
        assert_eq!(health_tier(4, 4), HealthTier::Healthy);
        assert_eq!(health_tier(3, 4), HealthTier::Moderate);
        assert_eq!(health_tier(2, 4), HealthTier::Moderate);
        assert_eq!(health_tier(1, 4), HealthTier::Severe);
    }

    #[test]
    fn descriptions() {
        assert_eq!(health_description(HealthTier::Healthy), "healthy");
        assert_eq!(
            health_description(HealthTier::Moderate),
            "moderately damaged"
        );
        assert_eq!(health_description(HealthTier::Severe), "severely wounded");
        assert_eq!(health_description(HealthTier::AlmostDead), "almost dead");
    }

    #[test]
    fn health_tier_is_one_byte() {
        assert_eq!(core::mem::size_of::<HealthTier>(), 1);
    }
}
