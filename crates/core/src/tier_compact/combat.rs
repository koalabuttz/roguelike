//! Melee attack resolution for the compact tier (GBA).
//!
//! Delegates to [`rules::combat::resolve_melee`](crate::rules::combat::resolve_melee)
//! for the algorithm. This module adapts between the shared rule and the
//! compact tier's parallel-array entity storage and circular message log.

use super::entity::EntityStore;
use super::msglog::CompactMessageLog;
use super::types::*;
use crate::rules::combat as rules_combat;
use crate::rules::message::Combatant;

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
    log: &mut CompactMessageLog,
) -> bool {
    let a = combatant(entities, attacker);
    let d = combatant(entities, defender);

    let outcome = rules_combat::resolve_melee(
        a,
        d,
        atk,
        def,
        entities.hp[defender as usize],
        entities.max_hp[defender as usize],
    );

    entities.hp[defender as usize] = outcome.new_hp;
    if outcome.killed {
        entities.kill(defender);
    }

    for i in 0..outcome.event_count {
        log.add(outcome.events[i as usize]);
    }

    outcome.killed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::message::GameEvent;
    use crate::rules::monster_table::{AiPersonality, MonsterKind};

    fn setup() -> (EntityStore, CompactMessageLog) {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Goblin, 6, 5, AiPersonality::Aggressive);
        (e, CompactMessageLog::new())
    }

    #[test]
    fn positive_damage_reduces_hp() {
        let (mut e, mut log) = setup();
        let hp_before = e.hp[1];
        let atk = e.atk[0];
        let def = e.def[1];
        melee_attack(0, 1, atk, def, &mut e, &mut log);
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
