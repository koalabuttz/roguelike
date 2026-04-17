//! Monster AI for the compact tier (GBA) — delegates decisions to `rules::ai`.

use super::combat;
use super::entity::EntityStore;
use super::fov;
use super::map::CompactMap;
use super::msglog::CompactMessageLog;
use super::prng::LfsrRng32;
use super::types::*;
use crate::rules::ai::{self, AiMode, ChaseResult, FleeResult};
use crate::rules::direction::ALL_DIRECTIONS;
use crate::rules::message::{Combatant, GameEvent};
use crate::rules::monster_table::AiPersonality;
use crate::rules::spawn as rules_spawn;

/// Run all monster turns. Returns true if the player died.
///
/// `player_def` is the player's effective defense (base + armor bonus).
pub fn run_monster_turns(
    entities: &mut EntityStore,
    map: &CompactMap,
    rng: &mut LfsrRng32,
    log: &mut CompactMessageLog,
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
        let personality = entities.ai[idx];
        let sight = entities.sight[idx];

        let aware = match personality {
            AiPersonality::Aggressive | AiPersonality::Patrol | AiPersonality::Coward => {
                fov::can_see(mx, my, px, py, sight, map)
            }
            AiPersonality::Player => false,
        };

        let was_aware = entities.aware_last_turn[idx];
        entities.aware_last_turn[idx] = aware;
        if aware && !was_aware {
            let who = match entities.kind[idx] {
                Some(mk) => Combatant::Monster(mk),
                None => Combatant::UnknownMonster,
            };
            log.add(GameEvent::EntityNotice { who });
        }

        let hp_low = rules_spawn::hp_below_flee_threshold(entities.hp[idx], entities.max_hp[idx]);

        match ai::ai_mode(personality, aware, hp_low) {
            AiMode::Chase => {
                chase(i, px, py, entities, map, log, player_def);
            }
            AiMode::WakeUp => {
                entities.ai[idx] = AiPersonality::Aggressive;
                chase(i, px, py, entities, map, log, player_def);
            }
            AiMode::Wander => {
                wander(i, px, py, entities, map, rng);
            }
            AiMode::Flee => {
                flee(i, px, py, entities, map);
            }
            AiMode::Idle => {}
        }

        if !entities.alive[PLAYER_IDX as usize] {
            return true;
        }
    }
    false
}

/// Chase: compute candidate passability and delegate to `rules::ai::chase_step`.
fn chase(
    idx: u8,
    px: Coord,
    py: Coord,
    entities: &mut EntityStore,
    map: &CompactMap,
    log: &mut CompactMessageLog,
    player_def: u8,
) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let dx = px - mx;
    let dy = py - my;
    let sx = dx.signum();
    let sy = dy.signum();
    let adjacent = dx.abs() <= 1 && dy.abs() <= 1;

    let passable = [
        passable_at(mx + sx, my + sy, idx, entities, map),
        passable_at(mx + sx, my, idx, entities, map),
        passable_at(mx, my + sy, idx, entities, map),
    ];

    match ai::chase_step(sx, sy, adjacent, passable) {
        ChaseResult::Attack => {
            let atk = entities.atk[idx as usize];
            combat::melee_attack(idx, PLAYER_IDX, atk, player_def, entities, log);
        }
        ChaseResult::Move(dir) => {
            let (ddx, ddy) = dir.to_offset();
            entities.x[idx as usize] = mx + ddx;
            entities.y[idx as usize] = my + ddy;
        }
        ChaseResult::Blocked => {}
    }
}

/// Flee: greedy step away from the player. Cornered cowards idle.
fn flee(idx: u8, px: Coord, py: Coord, entities: &mut EntityStore, map: &CompactMap) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let dx = px - mx;
    let dy = py - my;
    let sx = dx.signum();
    let sy = dy.signum();

    let passable = [
        passable_at(mx - sx, my - sy, idx, entities, map),
        passable_at(mx - sx, my, idx, entities, map),
        passable_at(mx, my - sy, idx, entities, map),
    ];

    if let FleeResult::Move(dir) = ai::flee_step(sx, sy, passable) {
        let (ddx, ddy) = dir.to_offset();
        entities.x[idx as usize] = mx + ddx;
        entities.y[idx as usize] = my + ddy;
    }
}

/// Wander: build passable neighbor mask and delegate to `rules::ai::wander_step`.
fn wander(
    idx: u8,
    px: Coord,
    py: Coord,
    entities: &mut EntityStore,
    map: &CompactMap,
    rng: &mut LfsrRng32,
) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let mut mask: u8 = 0;
    let mut count: u8 = 0;

    for (i, &dir) in ALL_DIRECTIONS.iter().enumerate() {
        let (ddx, ddy) = dir.to_offset();
        let nx = mx + ddx;
        let ny = my + ddy;
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
            entities.x[idx as usize] = mx + ddx;
            entities.y[idx as usize] = my + ddy;
        }
    }
}

fn passable_at(x: Coord, y: Coord, idx: u8, entities: &EntityStore, map: &CompactMap) -> bool {
    map.is_walkable(x, y) && !entities.is_occupied(x, y, idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::monster_table::MonsterKind;
    use crate::tier_compact::map::TILE_FLOOR;

    fn arena_map() -> CompactMap {
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        for y in 5..25 {
            for x in 5..25 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        map
    }

    #[test]
    fn monster_chases_when_aware() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 15, 10, AiPersonality::Aggressive);
        let orig_x = entities.x[1];
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert!(entities.x[1] < orig_x, "monster should chase toward player");
    }

    #[test]
    fn monster_attacks_adjacent_player() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiPersonality::Aggressive);
        let hp_before = entities.hp[0];
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert!(entities.hp[0] < hp_before, "player should take damage");
    }

    #[test]
    fn awareness_transition() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 13, 10, AiPersonality::Patrol);
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert_eq!(entities.ai[1], AiPersonality::Aggressive);
    }

    #[test]
    fn dead_monsters_skip_turn() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiPersonality::Aggressive);
        entities.kill(1);
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
        let hp_before = entities.hp[0];
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert_eq!(entities.hp[0], hp_before, "dead monster should not attack");
    }
}
