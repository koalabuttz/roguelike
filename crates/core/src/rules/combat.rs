//! Pure combat resolution shared by all capability tiers.
//!
//! The damage *formula* lives in [`damage`](super::damage). This module owns the
//! resolution *algorithm*: apply damage, detect death, detect health-tier
//! transitions, and decide which [`GameEvent`]s to emit. All inputs and outputs
//! are scalars — no entity structs, no message logs.

use super::damage;
use super::health;
use super::message::{Combatant, GameEvent};

/// Result of a melee attack resolution.
///
/// Callers read these fields and apply them to their tier-specific data
/// structures (Entity slice, EntityStore parallel arrays, etc.).
///
/// `events[0..event_count]` contains the [`GameEvent`]s to emit, in order.
/// Slots beyond `event_count` are initialized but must not be read.
#[derive(Clone, Copy, Debug)]
pub struct CombatOutcome {
    /// Damage dealt (0 when defense >= attack).
    pub damage: u8,
    /// Defender's HP after damage (`saturating_sub`, so always >= 0).
    pub new_hp: u8,
    /// Whether the defender was killed.
    pub killed: bool,
    /// Number of valid events in `events` (1 or 2).
    pub event_count: u8,
    /// Events to emit. Only `events[0..event_count]` are meaningful.
    pub events: [GameEvent; 2],
}

/// Resolve a melee attack from pure scalar inputs.
///
/// `atk` and `def` are *effective* stats (base + equipment bonuses) —
/// callers compute these before calling. `defender_hp` and
/// `defender_max_hp` are the defender's current and maximum health.
///
/// Returns a [`CombatOutcome`] describing what happened. The caller is
/// responsible for writing `new_hp` back, marking the defender dead if
/// `killed`, and pushing `events[0..event_count]` into its message log.
pub fn resolve_melee(
    attacker: Combatant,
    defender: Combatant,
    atk: u8,
    def: u8,
    defender_hp: u8,
    defender_max_hp: u8,
) -> CombatOutcome {
    let dmg = damage::damage(atk, def);

    // Dummy value for unused array slots — never read by callers.
    let dummy = GameEvent::NoDamage { attacker, defender };

    if dmg == 0 {
        return CombatOutcome {
            damage: 0,
            new_hp: defender_hp,
            killed: false,
            event_count: 1,
            events: [dummy, dummy],
        };
    }

    let old_tier = health::health_tier(defender_hp, defender_max_hp);
    let new_hp = defender_hp.saturating_sub(dmg);
    let attack_event = GameEvent::Attack {
        attacker,
        defender,
        damage: dmg,
    };

    if new_hp == 0 {
        return CombatOutcome {
            damage: dmg,
            new_hp: 0,
            killed: true,
            event_count: 2,
            events: [
                attack_event,
                GameEvent::Kill {
                    attacker,
                    victim: defender,
                },
            ],
        };
    }

    let new_tier = health::health_tier(new_hp, defender_max_hp);
    if new_tier != old_tier {
        CombatOutcome {
            damage: dmg,
            new_hp,
            killed: false,
            event_count: 2,
            events: [
                attack_event,
                GameEvent::HealthStatus {
                    who: defender,
                    tier: new_tier,
                },
            ],
        }
    } else {
        CombatOutcome {
            damage: dmg,
            new_hp,
            killed: false,
            event_count: 1,
            events: [attack_event, dummy],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::health::HealthTier;
    use crate::rules::monster_table::MonsterKind;

    const PLAYER: Combatant = Combatant::Player;
    const GOBLIN: Combatant = Combatant::Monster(MonsterKind::Goblin);

    #[test]
    fn positive_damage_reduces_hp() {
        let o = resolve_melee(PLAYER, GOBLIN, 5, 2, 10, 10);
        assert_eq!(o.damage, 3);
        assert_eq!(o.new_hp, 7);
        assert!(!o.killed);
    }

    #[test]
    fn zero_damage_when_defense_equals_attack() {
        let o = resolve_melee(PLAYER, GOBLIN, 4, 4, 10, 10);
        assert_eq!(o.damage, 0);
        assert_eq!(o.new_hp, 10);
        assert!(!o.killed);
        assert_eq!(o.event_count, 1);
        assert!(matches!(o.events[0], GameEvent::NoDamage { .. }));
    }

    #[test]
    fn zero_damage_when_defense_exceeds_attack() {
        let o = resolve_melee(PLAYER, GOBLIN, 2, 5, 10, 10);
        assert_eq!(o.damage, 0);
        assert_eq!(o.new_hp, 10);
        assert!(!o.killed);
    }

    #[test]
    fn lethal_damage_emits_attack_and_kill() {
        let o = resolve_melee(PLAYER, GOBLIN, 5, 2, 3, 10);
        assert!(o.killed);
        assert_eq!(o.new_hp, 0);
        assert_eq!(o.event_count, 2);
        assert!(matches!(o.events[0], GameEvent::Attack { damage: 3, .. }));
        assert!(matches!(o.events[1], GameEvent::Kill { .. }));
    }

    #[test]
    fn exact_lethal_damage() {
        let o = resolve_melee(PLAYER, GOBLIN, 5, 2, 3, 3);
        assert!(o.killed);
        assert_eq!(o.new_hp, 0);
        assert_eq!(o.damage, 3);
    }

    #[test]
    fn overkill_clamps_to_zero() {
        let o = resolve_melee(PLAYER, GOBLIN, 10, 0, 3, 3);
        assert!(o.killed);
        assert_eq!(o.new_hp, 0);
    }

    #[test]
    fn health_tier_change_emits_status() {
        // 20/20 = 100% Healthy → 15/20 = 75% Moderate
        let o = resolve_melee(PLAYER, GOBLIN, 5, 0, 20, 20);
        assert_eq!(o.damage, 5);
        assert_eq!(o.new_hp, 15);
        assert!(!o.killed);
        assert_eq!(o.event_count, 2);
        assert!(matches!(o.events[0], GameEvent::Attack { .. }));
        assert!(matches!(
            o.events[1],
            GameEvent::HealthStatus {
                tier: HealthTier::Moderate,
                ..
            }
        ));
    }

    #[test]
    fn no_tier_change_no_status_event() {
        // 20/20 = 100% Healthy → 19/20 = 95% still Healthy
        let o = resolve_melee(PLAYER, GOBLIN, 1, 0, 20, 20);
        assert_eq!(o.event_count, 1);
        assert!(matches!(o.events[0], GameEvent::Attack { .. }));
    }

    #[test]
    fn combatant_identities_in_events() {
        let o = resolve_melee(PLAYER, GOBLIN, 5, 2, 10, 10);
        match o.events[0] {
            GameEvent::Attack {
                attacker, defender, ..
            } => {
                assert_eq!(attacker, PLAYER);
                assert_eq!(defender, GOBLIN);
            }
            _ => panic!("expected Attack event"),
        }
    }

    #[test]
    fn outcome_size_is_bounded() {
        assert!(
            core::mem::size_of::<CombatOutcome>() <= 24,
            "CombatOutcome grew beyond expected size"
        );
    }
}
