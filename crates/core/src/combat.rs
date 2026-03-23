use crate::entity::Entity;
use crate::message_log::MessageLog;
use crate::rules::damage as rules_damage;
use crate::rules::health;
use crate::rules::message::GameEvent;
use crate::types::Stat;

/// Resolve a melee attack between two entities by index.
///
/// `atk` and `def` are the *effective* stats (base + equipment bonuses).
/// Callers must compute these — this function does not read entity fields
/// for attack/defense, keeping equipment logic out of the combat module.
///
/// Returns true if the defender was killed.
pub fn melee_attack(
    entities: &mut [Entity],
    attacker: usize,
    defender: usize,
    atk: Stat,
    def: Stat,
    log: &mut MessageLog,
) -> bool {
    let damage = rules_damage::damage(rules_damage::narrow(atk), rules_damage::narrow(def)) as Stat;
    let attacker_c = entities[attacker].combatant();
    let defender_c = entities[defender].combatant();

    if damage > 0 {
        let old_tier = health::health_tier(
            rules_damage::narrow(entities[defender].hp),
            rules_damage::narrow(entities[defender].max_hp),
        );
        entities[defender].hp -= damage;
        log.add_event(GameEvent::Attack {
            attacker: attacker_c,
            defender: defender_c,
            damage: damage as u8,
        });

        if entities[defender].hp > 0 {
            let new_tier = health::health_tier(
                rules_damage::narrow(entities[defender].hp),
                rules_damage::narrow(entities[defender].max_hp),
            );
            if new_tier != old_tier {
                log.add_event(GameEvent::HealthStatus {
                    who: defender_c,
                    tier: new_tier,
                });
            }
        }

        if entities[defender].hp <= 0 {
            entities[defender].alive = false;
            log.add_event(GameEvent::Kill {
                attacker: attacker_c,
                victim: defender_c,
            });
            return true;
        }
    } else {
        log.add_event(GameEvent::NoDamage {
            attacker: attacker_c,
            defender: defender_c,
        });
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;
    use crate::types::Stat;

    fn make_attacker(atk: Stat) -> Entity {
        let mut e = Entity::player(0, 0);
        e.name = "Attacker".into();
        e.attack = atk;
        e
    }

    fn make_defender(hp: Stat, def: Stat) -> Entity {
        let mut e = Entity::player(1, 0);
        e.name = "Defender".into();
        e.hp = hp;
        e.max_hp = hp;
        e.defense = def;
        e
    }

    #[test]
    fn positive_damage_reduces_hp() {
        let mut entities = vec![make_attacker(5), make_defender(10, 2)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, 5, 2, &mut log);
        assert_eq!(entities[1].hp, 7); // 5-2 = 3 damage, 10-3 = 7
    }

    #[test]
    fn zero_damage_when_defense_equals_attack() {
        let mut entities = vec![make_attacker(3), make_defender(10, 3)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, 3, 3, &mut log);
        assert_eq!(entities[1].hp, 10);
        assert!(log.recent(1)[0].contains("deals no damage"));
    }

    #[test]
    fn zero_damage_when_defense_exceeds_attack() {
        let mut entities = vec![make_attacker(2), make_defender(10, 5)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, 2, 5, &mut log);
        assert_eq!(entities[1].hp, 10);
    }

    #[test]
    fn kill_returns_true_and_marks_dead() {
        let mut entities = vec![make_attacker(5), make_defender(3, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, 5, 2, &mut log);
        assert!(killed);
        assert!(!entities[1].alive);
        assert!(log.recent(1)[0].contains("dead"));
    }

    #[test]
    fn non_kill_returns_false() {
        let mut entities = vec![make_attacker(5), make_defender(10, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, 5, 2, &mut log);
        assert!(!killed);
        assert!(entities[1].alive);
    }

    #[test]
    fn exact_lethal_damage_kills() {
        // 5 atk - 2 def = 3 damage, exactly equal to 3 hp
        let mut entities = vec![make_attacker(5), make_defender(3, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, 5, 2, &mut log);
        assert!(killed);
        assert_eq!(entities[1].hp, 0);
    }

    #[test]
    fn log_messages_include_combatant_names() {
        let mut entities = vec![make_attacker(5), make_defender(10, 2)];
        // Both test entities lack monster_kind, so both map to Combatant::Player.
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, 5, 2, &mut log);
        // Check any recent message mentions "Player" (attack or health status).
        assert!(log.recent(5).iter().any(|m| m.contains("Player")));
    }
}
