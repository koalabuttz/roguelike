//! Melee attack resolution for the micro tier.
//!
//! Uses `rules::damage::damage()` for the formula and emits `GameEvent`s.

use super::entity::EntityStore;
use super::msglog::MicroMessageLog;
use super::types::*;
use crate::rules::damage;
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
pub fn melee_attack(
    attacker: u8,
    defender: u8,
    entities: &mut EntityStore,
    log: &mut MicroMessageLog,
) -> bool {
    let atk = entities.atk[attacker as usize];
    let def = entities.def[defender as usize];
    let dmg = damage::damage(atk, def);

    let a = combatant(entities, attacker);
    let d = combatant(entities, defender);

    if dmg > 0 {
        let new_hp = entities.hp[defender as usize].saturating_sub(dmg);
        entities.hp[defender as usize] = new_hp;
        log.add(GameEvent::Attack {
            attacker: a,
            defender: d,
            damage: dmg,
        });
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
        melee_attack(0, 1, &mut e, &mut log);
        // Player ATK=5, Goblin DEF=0 → 5 damage
        assert!(e.hp[1] < hp_before);
    }

    #[test]
    fn zero_damage_when_defense_exceeds() {
        let (mut e, mut log) = setup();
        // Give defender high defense
        e.def[1] = 100;
        let hp_before = e.hp[1];
        melee_attack(0, 1, &mut e, &mut log);
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
        let killed = melee_attack(0, 1, &mut e, &mut log);
        assert!(killed);
        assert!(!e.alive[1]);
    }

    #[test]
    fn game_events_emitted() {
        let (mut e, mut log) = setup();
        melee_attack(0, 1, &mut e, &mut log);
        match log.recent(0) {
            Some(GameEvent::Attack {
                attacker,
                defender,
                damage: _,
            }) => {
                assert_eq!(attacker, Combatant::Player);
                assert_eq!(defender, Combatant::Monster(MonsterKind::Goblin));
            }
            other => panic!("expected Attack event, got {other:?}"),
        }
    }
}
