//! Monster AI for the compact tier (GBA) — chase/wander with Bresenham LOS awareness.

use super::combat;
use super::entity::EntityStore;
use super::fov;
use super::map::CompactMap;
use super::msglog::CompactMessageLog;
use super::prng::LfsrRng32;
use super::types::*;
use crate::rules::message::{Combatant, GameEvent};
use crate::rules::monster_table::AiBehavior;

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
        let behavior = entities.ai[idx];
        let sight = entities.sight[idx];

        let aware = match behavior {
            AiBehavior::Chase | AiBehavior::Wander => fov::can_see(mx, my, px, py, sight, map),
            AiBehavior::None => false,
        };

        match behavior {
            AiBehavior::Chase => {
                if aware {
                    chase(i, px, py, entities, map, log, player_def);
                }
            }
            AiBehavior::Wander => {
                if aware {
                    entities.ai[idx] = AiBehavior::Chase;
                    let who = match entities.kind[idx] {
                        Some(mk) => Combatant::Monster(mk),
                        None => Combatant::UnknownMonster,
                    };
                    log.add(GameEvent::EntityNotice { who });
                    chase(i, px, py, entities, map, log, player_def);
                } else {
                    wander(i, entities, map, rng);
                }
            }
            AiBehavior::None => {}
        }

        if !entities.alive[PLAYER_IDX as usize] {
            return true;
        }
    }
    false
}

/// Greedy chase: try diagonal, then horizontal, then vertical toward player.
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
    let dist_x = dx.abs();
    let dist_y = dy.abs();

    // If adjacent, attack instead of moving.
    if dist_x <= 1 && dist_y <= 1 {
        let atk = entities.atk[idx as usize];
        combat::melee_attack(idx, PLAYER_IDX, atk, player_def, entities, log);
        return;
    }

    let sx = dx.signum();
    let sy = dy.signum();

    let candidates: [(Coord, Coord); 3] = [
        (sx, sy), // diagonal (preferred)
        (sx, 0),  // horizontal
        (0, sy),  // vertical
    ];

    for &(cx, cy) in candidates.iter() {
        if cx == 0 && cy == 0 {
            continue;
        }
        let nx = mx + cx;
        let ny = my + cy;
        if map.is_walkable(nx, ny) && !entities.is_occupied(nx, ny, idx) {
            entities.x[idx as usize] = nx;
            entities.y[idx as usize] = ny;
            return;
        }
    }
}

/// Random walk: pick a random walkable, unoccupied neighbor.
fn wander(idx: u8, entities: &mut EntityStore, map: &CompactMap, rng: &mut LfsrRng32) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let px = entities.x[PLAYER_IDX as usize];
    let py = entities.y[PLAYER_IDX as usize];

    let mut candidates: [(Coord, Coord); 8] = [(0, 0); 8];
    let mut count: u8 = 0;

    for dy in -1..=1_i32 {
        for dx in -1..=1_i32 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = mx + dx;
            let ny = my + dy;
            if !(nx == px && ny == py)
                && map.is_walkable(nx, ny)
                && !entities.is_occupied(nx, ny, idx)
            {
                candidates[count as usize] = (nx, ny);
                count += 1;
            }
        }
    }

    if count > 0 {
        let pick = rng.range_u8(0, count - 1);
        let (nx, ny) = candidates[pick as usize];
        entities.x[idx as usize] = nx;
        entities.y[idx as usize] = ny;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::monster_table::MonsterKind;
    use crate::tier_compact::map::TILE_FLOOR;

    /// Create a small open arena for AI testing.
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
        entities.spawn_monster(MonsterKind::Goblin, 15, 10, AiBehavior::Chase);
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
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
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
        entities.spawn_monster(MonsterKind::Goblin, 13, 10, AiBehavior::Wander);
        let player_def = entities.def[PLAYER_IDX as usize];

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
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

        let mut rng = LfsrRng32::new(42);
        let mut log = CompactMessageLog::new();
        let hp_before = entities.hp[0];
        run_monster_turns(&mut entities, &map, &mut rng, &mut log, player_def);

        assert_eq!(entities.hp[0], hp_before, "dead monster should not attack");
    }
}
