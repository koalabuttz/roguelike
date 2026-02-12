use crate::entity::Entity;
use crate::message_log::MessageLog;

/// Resolve a melee attack between two entities by index.
/// Returns true if the defender was killed.
pub fn melee_attack(
    entities: &mut [Entity],
    attacker: usize,
    defender: usize,
    log: &mut MessageLog,
) -> bool {
    let atk = entities[attacker].attack;
    let def = entities[defender].defense;
    let damage = (atk - def).max(0);

    if damage > 0 {
        entities[defender].hp -= damage;
        log.add(format!(
            "{} attacks {} for {} damage.",
            entities[attacker].name, entities[defender].name, damage
        ));

        if entities[defender].hp <= 0 {
            entities[defender].alive = false;
            log.add(format!("{} is dead!", entities[defender].name));
            return true;
        }
    } else {
        log.add(format!(
            "{} attacks {} but does no damage.",
            entities[attacker].name, entities[defender].name
        ));
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;

    fn make_attacker(atk: i32) -> Entity {
        let mut e = Entity::player(0, 0);
        e.name = "Attacker".into();
        e.attack = atk;
        e
    }

    fn make_defender(hp: i32, def: i32) -> Entity {
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
        melee_attack(&mut entities, 0, 1, &mut log);
        assert_eq!(entities[1].hp, 7); // 5-2 = 3 damage, 10-3 = 7
    }

    #[test]
    fn zero_damage_when_defense_equals_attack() {
        let mut entities = vec![make_attacker(3), make_defender(10, 3)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, &mut log);
        assert_eq!(entities[1].hp, 10);
        assert!(log.recent(1)[0].contains("no damage"));
    }

    #[test]
    fn zero_damage_when_defense_exceeds_attack() {
        let mut entities = vec![make_attacker(2), make_defender(10, 5)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, &mut log);
        assert_eq!(entities[1].hp, 10);
    }

    #[test]
    fn kill_returns_true_and_marks_dead() {
        let mut entities = vec![make_attacker(5), make_defender(3, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, &mut log);
        assert!(killed);
        assert!(!entities[1].alive);
        assert!(log.recent(1)[0].contains("dead"));
    }

    #[test]
    fn non_kill_returns_false() {
        let mut entities = vec![make_attacker(5), make_defender(10, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, &mut log);
        assert!(!killed);
        assert!(entities[1].alive);
    }

    #[test]
    fn exact_lethal_damage_kills() {
        // 5 atk - 2 def = 3 damage, exactly equal to 3 hp
        let mut entities = vec![make_attacker(5), make_defender(3, 2)];
        let mut log = MessageLog::new();
        let killed = melee_attack(&mut entities, 0, 1, &mut log);
        assert!(killed);
        assert_eq!(entities[1].hp, 0);
    }

    #[test]
    fn log_messages_include_names() {
        let mut entities = vec![make_attacker(5), make_defender(10, 2)];
        let mut log = MessageLog::new();
        melee_attack(&mut entities, 0, 1, &mut log);
        let msg = &log.recent(1)[0];
        assert!(msg.contains("Attacker"));
        assert!(msg.contains("Defender"));
    }
}
