use crate::entity::Entity;
use crate::message_log::MessageLog;
use crate::rules::combat as rules_combat;
use crate::rules::damage::narrow;
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
    let attacker_c = entities[attacker].combatant();
    let defender_c = entities[defender].combatant();

    let outcome = rules_combat::resolve_melee(
        attacker_c,
        defender_c,
        narrow(atk),
        narrow(def),
        narrow(entities[defender].hp),
        narrow(entities[defender].max_hp),
    );

    entities[defender].hp = outcome.new_hp as Stat;
    if outcome.killed {
        entities[defender].alive = false;
    }

    for i in 0..outcome.event_count {
        log.add_event(outcome.events[i as usize]);
    }

    outcome.killed
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
