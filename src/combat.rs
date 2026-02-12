use crate::entity::Entity;
use crate::message_log::MessageLog;

/// Resolve a melee attack between two entities by index.
/// Returns true if the defender was killed.
pub fn melee_attack(
    entities: &mut Vec<Entity>,
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
