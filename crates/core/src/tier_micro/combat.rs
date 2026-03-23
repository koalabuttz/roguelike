//! Melee attack resolution for the micro tier.
//!
//! Uses `rules::damage::damage()` for the formula and emits `GameEvent`s.

use super::entity::EntityStore;
use super::msglog::MicroMessageLog;
use super::types::*;
use crate::rules::damage;
use crate::rules::health;
use crate::rules::message::{Combatant, GameEvent};

fn combatant(entities: &EntityStore, idx: u8) -> Combatant {
    if idx == PLAYER_IDX {
        Combatant::Player
    } else {
        match entities.kind[idx as usize] {
            Some(mk) => Combatant::Monster(mk),
            None => Combatant::UnknownMonster,
        }
    }
}

/// Execute a melee attack. Returns true if the defender was killed.
///
/// Callers provide effective ATK/DEF (base + equipment bonuses) so that
/// equipment logic stays out of the combat module.
pub fn melee_attack(
    attacker: u8,
    defender: u8,
    atk: u8,
    def: u8,
    entities: &mut EntityStore,
    log: &mut MicroMessageLog,
) -> bool {
    let dmg = damage::damage(atk, def);

    let a = combatant(entities, attacker);
    let d = combatant(entities, defender);

    if dmg > 0 {
        let old_tier = health::health_tier(
            entities.hp[defender as usize],
            entities.max_hp[defender as usize],
        );
        let new_hp = entities.hp[defender as usize].saturating_sub(dmg);
        entities.hp[defender as usize] = new_hp;
        log.add(GameEvent::Attack {
            attacker: a,
            defender: d,
            damage: dmg,
        });
        if new_hp > 0 {
            let new_tier = health::health_tier(new_hp, entities.max_hp[defender as usize]);
            if new_tier != old_tier {
                log.add(GameEvent::HealthStatus {
                    who: d,
                    tier: new_tier,
                });
            }
        }
        if new_hp == 0 {
            entities.kill(defender);
            log.add(GameEvent::Kill {
                attacker: a,
                victim: d,
            });
            return true;
        }
    } else {
        log.add(GameEvent::NoDamage {
            attacker: a,
            defender: d,
        });
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::monster_table::{AiBehavior, MonsterKind};

    fn setup() -> (EntityStore, MicroMessageLog) {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Goblin, 6, 5, AiBehavior::Chase);
        (e, MicroMessageLog::new())
    }

    #[test]
    fn positive_damage_reduces_hp() {
        let (mut e, mut log) = setup();
        let hp_before = e.hp[1];
        let atk = e.atk[0];
        let def = e.def[1];
        melee_attack(0, 1, atk, def, &mut e, &mut log);
        // Player ATK=5, Goblin DEF=0 → 5 damage
        assert!(e.hp[1] < hp_before);
    }

    #[test]
    fn zero_damage_when_defense_exceeds() {
        let (mut e, mut log) = setup();
        let atk = e.atk[0];
        let hp_before = e.hp[1];
        melee_attack(0, 1, atk, 100, &mut e, &mut log);
        assert_eq!(e.hp[1], hp_before);
        assert_eq!(
            log.recent(0),
            Some(GameEvent::NoDamage {
                attacker: Combatant::Player,
                defender: Combatant::Monster(MonsterKind::Goblin),
            })
        );
    }

    #[test]
    fn kill_on_zero_hp() {
        let (mut e, mut log) = setup();
        e.hp[1] = 1;
        let atk = e.atk[0];
        let def = e.def[1];
        let killed = melee_attack(0, 1, atk, def, &mut e, &mut log);
        assert!(killed);
        assert!(!e.alive[1]);
    }

    #[test]
    fn game_events_emitted() {
        let (mut e, mut log) = setup();
        let atk = e.atk[0];
        let def = e.def[1];
        melee_attack(0, 1, atk, def, &mut e, &mut log);
        // Attack may be followed by HealthStatus, so check both recent slots.
        let found = (0..4).any(|i| {
            matches!(
                log.recent(i),
                Some(GameEvent::Attack {
                    attacker: Combatant::Player,
                    defender: Combatant::Monster(MonsterKind::Goblin),
                    ..
                })
            )
        });
        assert!(found, "expected Attack event in log");
    }
}
