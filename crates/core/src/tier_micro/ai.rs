//! Monster AI for the micro tier — chase/wander with Bresenham LOS awareness.

use super::combat;
use super::entity::EntityStore;
use super::fov;
use super::map::MicroMap;
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::message::{Combatant, GameEvent};
use crate::rules::monster_table::AiBehavior;

fn signum(v: i8) -> i8 {
    if v > 0 {
        1
    } else if v < 0 {
        -1
    } else {
        0
    }
}

/// Run all monster turns. Returns true if the player died.
pub fn run_monster_turns(
    entities: &mut EntityStore,
    map: &MicroMap,
    rng: &mut LfsrRng16,
    log: &mut MicroMessageLog,
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
                    chase(i, px, py, entities, map, log);
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
                    chase(i, px, py, entities, map, log);
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
    px: u8,
    py: u8,
    entities: &mut EntityStore,
    map: &MicroMap,
    log: &mut MicroMessageLog,
) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];

    let dx = (px as i8) - (mx as i8);
    let dy = (py as i8) - (my as i8);
    let dist_x = dx.unsigned_abs();
    let dist_y = dy.unsigned_abs();

    // If adjacent, attack instead of moving
    if dist_x <= 1 && dist_y <= 1 {
        combat::melee_attack(idx, PLAYER_IDX, entities, log);
        return;
    }

    let sx = signum(dx);
    let sy = signum(dy);

    let candidates: [(i8, i8); 3] = [
        (sx, sy), // diagonal (preferred)
        (sx, 0),  // horizontal
        (0, sy),  // vertical
    ];

    for &(cx, cy) in candidates.iter() {
        if cx == 0 && cy == 0 {
            continue;
        }
        let nx = (mx as i8 + cx) as u8;
        let ny = (my as i8 + cy) as u8;
        if map.is_walkable(nx, ny) && !entities.is_occupied(nx, ny, idx) {
            entities.x[idx as usize] = nx;
            entities.y[idx as usize] = ny;
            return;
        }
    }
}

/// Random walk: pick a random walkable, unoccupied neighbor.
fn wander(idx: u8, entities: &mut EntityStore, map: &MicroMap, rng: &mut LfsrRng16) {
    let mx = entities.x[idx as usize];
    let my = entities.y[idx as usize];
    let px = entities.x[PLAYER_IDX as usize];
    let py = entities.y[PLAYER_IDX as usize];

    let mut candidates: [(u8, u8); 8] = [(0, 0); 8];
    let mut count: u8 = 0;

    for dy in -1i8..=1 {
        for dx in -1i8..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (mx as i8 + dx) as u8;
            let ny = (my as i8 + dy) as u8;
            if nx == px && ny == py {
                continue;
            }
            if map.is_walkable(nx, ny) && !entities.is_occupied(nx, ny, idx) {
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

    /// Create a small open arena for AI testing.
    fn arena_map() -> MicroMap {
        let mut map = MicroMap::new_default();
        // Carve a 20x20 floor area
        for y in 5..25 {
            for x in 5..25 {
                map.tiles[map.idx(x, y)] = super::super::map::TILE_FLOOR;
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

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log);

        // Monster should have moved closer to player
        assert!(entities.x[1] < orig_x, "monster should chase toward player");
    }

    #[test]
    fn monster_attacks_adjacent_player() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        let hp_before = entities.hp[0];

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log);

        // Player should have taken damage (Goblin ATK=3, Player DEF=2 → 1 damage)
        assert!(entities.hp[0] < hp_before, "player should take damage");
    }

    #[test]
    fn awareness_transition() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        // Place wandering monster within sight range
        entities.spawn_monster(MonsterKind::Goblin, 13, 10, AiBehavior::Wander);

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        run_monster_turns(&mut entities, &map, &mut rng, &mut log);

        // Should have switched to Chase
        assert_eq!(entities.ai[1], AiBehavior::Chase);
    }

    #[test]
    fn dead_monsters_skip_turn() {
        let map = arena_map();
        let mut entities = EntityStore::new();
        entities.spawn_player(10, 10);
        entities.spawn_monster(MonsterKind::Goblin, 11, 10, AiBehavior::Chase);
        entities.kill(1);

        let mut rng = LfsrRng16::new(42);
        let mut log = MicroMessageLog::new();
        let hp_before = entities.hp[0];
        run_monster_turns(&mut entities, &map, &mut rng, &mut log);

        assert_eq!(entities.hp[0], hp_before, "dead monster should not attack");
    }
}
