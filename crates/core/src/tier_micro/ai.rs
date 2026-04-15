//! Monster AI for the micro tier — delegates decisions to `rules::ai`.

use super::combat;
use super::entity::EntityStore;
use super::fov;
use super::map::MicroMap;
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::ai::{self, AiMode, ChaseResult};
use crate::rules::direction::ALL_DIRECTIONS;
use crate::rules::message::{Combatant, GameEvent};
use crate::rules::monster_table::AiBehavior;

/// Run all monster turns. Returns true if the player died.
///
/// `player_def` is the player's effective defense (base + armor bonus).
pub fn run_monster_turns(
    entities: &mut EntityStore,
    map: &MicroMap,
    rng: &mut LfsrRng16,
    log: &mut MicroMessageLog,
    player_def: u8,
) -> bool {
    let px = entities.x[PLAYER_IDX as usize];
    let py = entities.y[PLAYER_IDX as usize];

    let count = entities.count;
    for i in 1..count {
        let idx = i as usize;
        if !entities.alive[idx] {
            continue;
        }

        let mx = entities.x[idx];
        let my = entities.y[idx];
        let behavior = entities.ai[idx];
        let sight = entities.sight[idx];

        let aware = match behavior {
            AiBehavior::Chase | AiBehavior::Wander => fov::can_see(mx, my, px, py, sight, map),
            AiBehavior::None => false,
        };

        match ai::ai_mode(behavior, aware) {
            AiMode::Chase => {
                chase(i, px, py, entities, map, log, player_def);
            }
            AiMode::WakeUp => {
                entities.ai[idx] = AiBehavior::Chase;
                let who = match entities.kind[idx] {
                    Some(mk) => Combatant::Monster(mk),
                    None => Combatant::UnknownMonster,
                };
                log.add(GameEvent::EntityNotice { who });
                chase(i, px, py, entities, map, log, player_def);
            }
            AiMode::Wander => {
                wander(i, px, py, entities, map, rng);
            }
            AiMode::Idle => {}
        }

        if !entities.alive[PLAYER_IDX as usize] {
            return true;
        }
    }
    false
}

/// Chase: compute signum in i8, delegate decision to `rules::ai`.
fn chase(
    idx: u8,
    px: u8,
    py: u8,
    entities: &mut EntityStore,
    map: &MicroMap,
    log: &mut MicroMessageLog,
    player_def: u8,
) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];

    // i8 arithmetic avoids i32 signum (G_SCMP) which 6502 can't legalize.
    let dx = (px as i8) - (mx as i8);
    let dy = (py as i8) - (my as i8);
    let sx: i8 = if dx > 0 {
        1
    } else if dx < 0 {
        -1
    } else {
        0
    };
    let sy: i8 = if dy > 0 {
        1
    } else if dy < 0 {
        -1
    } else {
        0
    };
    let adjacent = dx.unsigned_abs() <= 1 && dy.unsigned_abs() <= 1;

    let passable = [
        passable_at(
            (mx as i8 + sx) as u8,
            (my as i8 + sy) as u8,
            idx,
            entities,
            map,
        ),
        passable_at((mx as i8 + sx) as u8, my, idx, entities, map),
        passable_at(mx, (my as i8 + sy) as u8, idx, entities, map),
    ];

    match ai::chase_step(sx as i32, sy as i32, adjacent, passable) {
        ChaseResult::Attack => {
            let atk = entities.atk[idx as usize];
            combat::melee_attack(idx, PLAYER_IDX, atk, player_def, entities, log);
        }
        ChaseResult::Move(dir) => {
            let (ddx, ddy) = dir.to_offset();
            entities.x[idx as usize] = (mx as i8 + ddx as i8) as u8;
            entities.y[idx as usize] = (my as i8 + ddy as i8) as u8;
        }
        ChaseResult::Blocked => {}
    }
}

/// Wander: build passable neighbor mask, delegate to `rules::ai`.
fn wander(
    idx: u8,
    px: u8,
    py: u8,
    entities: &mut EntityStore,
    map: &MicroMap,
    rng: &mut LfsrRng16,
) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let mut mask: u8 = 0;
    let mut count: u8 = 0;

    for (i, &dir) in ALL_DIRECTIONS.iter().enumerate() {
        let (ddx, ddy) = dir.to_offset();
        let nx = (mx as i8 + ddx as i8) as u8;
        let ny = (my as i8 + ddy as i8) as u8;
        if !(nx == px && ny == py) && map.is_walkable(nx, ny) && !entities.is_occupied(nx, ny, idx)
        {
            mask |= 1 << i;
            count += 1;
        }
    }

    if count > 0 {
        let roll = rng.range_u8(0, count - 1);
        if let Some(dir) = ai::wander_step(mask, roll) {
            let (ddx, ddy) = dir.to_offset();
            entities.x[idx as usize] = (mx as i8 + ddx as i8) as u8;
            entities.y[idx as usize] = (my as i8 + ddy as i8) as u8;
        }
    }
}

fn passable_at(x: u8, y: u8, idx: u8, entities: &EntityStore, map: &MicroMap) -> bool {
    map.is_walkable(x, y) && !entities.is_occupied(x, y, idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::monster_table::MonsterKind;

    fn arena_map() -> MicroMap {
        let mut map = MicroMap::new_default();
        for y in 5..25 {
            for x in 5..25 {
                map.set_tile(x, y, super::super::map::TILE_FLOOR);
            }
        }
        map
    }

    #[test]
    fn monster_chases_when_aware() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 15, 10, AiBehavior::Chase);
        let orig_x = entities.x[1];
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert!(entities.x[1] < orig_x, "monster should chase toward player");
    }

    #[test]
    fn monster_attacks_adjacent_player() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        let hp_before = entities.hp[0];
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert!(entities.hp[0] < hp_before, "player should take damage");
    }

    #[test]
    fn awareness_transition() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 13, 10, AiBehavior::Wander);
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert_eq!(entities.ai[1], AiBehavior::Chase);
    }

    #[test]
    fn dead_monsters_skip_turn() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        entities.kill(1);
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        let hp_before = entities.hp[0];
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert_eq!(entities.hp[0], hp_before, "dead monster should not attack");
    }
}
