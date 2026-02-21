use rand::Rng;

use crate::combat;
use crate::entity::{AiBehavior, Entity};
use crate::fov;
use crate::map::Map;
use crate::message_log::MessageLog;
use crate::types::Coord;

/// Check whether a monster is aware of the player.
///
/// Each AI behavior has its own awareness model. Chase monsters use
/// line-of-sight within their individual `sight_radius`.
fn is_aware(entities: &[Entity], idx: usize, px: Coord, py: Coord, map: &Map) -> bool {
    match entities[idx].ai {
        AiBehavior::Chase | AiBehavior::Wander => fov::can_see(
            map,
            entities[idx].x,
            entities[idx].y,
            px,
            py,
            entities[idx].sight_radius,
        ),
        AiBehavior::None => false,
    }
}

/// Run all monster turns. Returns true if the player was killed.
pub fn run_monster_turns(
    entities: &mut [Entity],
    map: &Map,
    log: &mut MessageLog,
    rng: &mut impl Rng,
) -> bool {
    let px = entities[0].x;
    let py = entities[0].y;

    for i in 1..entities.len() {
        if !entities[i].alive {
            continue;
        }

        let aware = is_aware(entities, i, px, py, map);

        match entities[i].ai {
            AiBehavior::Chase => {
                if aware {
                    chase_ai(entities, i, px, py, map, log);
                }
            }
            AiBehavior::Wander => {
                if aware {
                    entities[i].ai = AiBehavior::Chase;
                    log.add(format!("The {} notices you!", entities[i].name));
                    chase_ai(entities, i, px, py, map, log);
                } else {
                    wander_ai(entities, i, map, rng);
                }
            }
            AiBehavior::None => {}
        }
    }

    !entities[0].alive
}

fn chase_ai(
    entities: &mut [Entity],
    idx: usize,
    px: Coord,
    py: Coord,
    map: &Map,
    log: &mut MessageLog,
) {
    let mx = entities[idx].x;
    let my = entities[idx].y;
    let dist_x = (px - mx).abs();
    let dist_y = (py - my).abs();

    // If adjacent, attack
    if dist_x <= 1 && dist_y <= 1 {
        combat::melee_attack(entities, idx, 0, log);
        return;
    }

    // Greedy chase: step toward player
    let step_x = (px - mx).signum();
    let step_y = (py - my).signum();

    let candidates = [
        (mx + step_x, my + step_y),
        (mx + step_x, my),
        (mx, my + step_y),
    ];

    for (nx, ny) in candidates {
        if map.is_walkable(nx, ny) && !is_occupied_by_monster(entities, nx, ny, idx) {
            entities[idx].x = nx;
            entities[idx].y = ny;
            break;
        }
    }
}

/// Random walk AI — pick a random walkable, unoccupied neighbor.
///
/// No pathfinding needed: in a 1-wide corridor, only 2 directions are walkable,
/// so the monster naturally follows corridors. In open rooms, movement is Brownian.
fn wander_ai(entities: &mut [Entity], idx: usize, map: &Map, rng: &mut impl Rng) {
    let mx = entities[idx].x;
    let my = entities[idx].y;
    let px = entities[0].x;
    let py = entities[0].y;

    // Collect walkable, unoccupied neighbor tiles (excluding player's tile).
    // Stack-local array: max 8 neighbors, zero heap allocation.
    let mut candidates = [(0i32, 0i32); 8];
    let mut count = 0;

    for &(dx, dy) in &[
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ] {
        let nx = mx + dx;
        let ny = my + dy;
        if nx == px && ny == py {
            continue;
        }
        if map.is_walkable(nx, ny) && !is_occupied_by_monster(entities, nx, ny, idx) {
            candidates[count] = (nx, ny);
            count += 1;
        }
    }

    if count > 0 {
        let pick = rng.gen_range(0..count);
        entities[idx].x = candidates[pick].0;
        entities[idx].y = candidates[pick].1;
    }
}

fn is_occupied_by_monster(entities: &[Entity], x: Coord, y: Coord, skip: usize) -> bool {
    entities
        .iter()
        .enumerate()
        .any(|(idx, e)| idx != skip && idx != 0 && e.alive && e.x == x && e.y == y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::map::{Map, Tile};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    /// Create a small open map (all floor except border walls).
    fn open_map(w: i32, h: i32) -> Map {
        let mut m = Map::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }
        m
    }

    fn test_rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    fn make_player(x: i32, y: i32) -> Entity {
        Entity::player(x, y)
    }

    fn make_monster(x: i32, y: i32) -> Entity {
        Entity::from_template(data::goblin(), x, y)
    }

    #[test]
    fn dead_monsters_are_skipped() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();
        let mut dead_monster = make_monster(3, 3);
        dead_monster.alive = false;
        let orig_x = dead_monster.x;
        let orig_y = dead_monster.y;

        let mut entities = vec![make_player(5, 5), dead_monster];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Dead monster should not have moved
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn non_visible_monsters_are_skipped() {
        let map = open_map(20, 20);
        let mut log = MessageLog::new();
        // Place monster behind a wall so it can't see the player
        // Build a wall column at x=10
        let mut walled_map = map;
        for y in 0..20 {
            let idx = walled_map.idx(10, y);
            walled_map.tiles[idx] = Tile::Wall;
        }
        let monster = make_monster(15, 5);
        let orig_x = monster.x;
        let orig_y = monster.y;

        let mut entities = vec![make_player(5, 5), monster];
        run_monster_turns(&mut entities, &walled_map, &mut log, &mut test_rng());
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn adjacent_monster_attacks_player() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(5, 4)]; // adjacent
        let player_hp_before = entities[0].hp;
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster should have attacked, reducing player HP (goblin atk=3, player def=2, dmg=1)
        assert!(entities[0].hp < player_hp_before);
    }

    #[test]
    fn non_adjacent_monster_moves_toward_player() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(2, 2)]; // far away
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster should have moved closer to player
        let dx = (entities[1].x - 5).abs();
        let dy = (entities[1].y - 5).abs();
        assert!(dx < 3 || dy < 3, "monster should have moved closer");
    }

    #[test]
    fn monster_does_not_walk_through_walls() {
        // Map with a wall blocking the direct path
        let mut map = open_map(10, 10);
        // Place wall at (4, 3) blocking direct path from (3,3) to (5,3)
        let idx = map.idx(4, 3);
        map.tiles[idx] = Tile::Wall;
        // Also block diagonal
        let idx2 = map.idx(4, 4);
        map.tiles[idx2] = Tile::Wall;

        let mut log = MessageLog::new();

        // Player at (5,3), monster at (3,3) — within sight radius on open tiles
        let mut entities = vec![make_player(5, 3), make_monster(3, 3)];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster should not be at (4, 3) — that's a wall
        assert!(!(entities[1].x == 4 && entities[1].y == 3));
    }

    #[test]
    fn monster_does_not_walk_through_other_monsters() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();

        // Monster at (3,5) wants to go toward player at (5,5).
        // Another monster at (4,5) blocks the direct path.
        let mut entities = vec![make_player(5, 5), make_monster(3, 5), make_monster(4, 5)];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // First monster (index 1) should not end up at (4,5) — that's where monster 2 is
        assert!(!(entities[1].x == 4 && entities[1].y == 5));
    }

    #[test]
    fn returns_true_when_player_dies() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();

        let mut player = make_player(5, 5);
        player.hp = 1; // Nearly dead
        player.defense = 0;
        let mut entities = vec![player, make_monster(5, 4)]; // adjacent goblin, atk=3
        let player_died = run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        assert!(player_died);
        assert!(!entities[0].alive);
    }

    #[test]
    fn returns_false_when_player_alive() {
        let map = open_map(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(5, 4)];
        let player_died = run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        assert!(!player_died);
    }

    // --- Per-monster FOV awareness tests ---

    #[test]
    fn monster_wakes_when_player_in_sight() {
        let map = open_map(20, 20);
        let mut log = MessageLog::new();
        // Goblin has sight_radius=6. Player at (5,5), monster at (8,5) — distance 3.
        let mut entities = vec![make_player(5, 5), make_monster(8, 5)];
        let orig_x = entities[1].x;
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster should have moved toward the player
        assert!(entities[1].x < orig_x, "monster should chase toward player");
    }

    #[test]
    fn monster_dormant_beyond_sight() {
        let map = open_map(20, 20);
        let mut log = MessageLog::new();
        // Create a monster with sight_radius=4 at distance 6
        let mut monster = make_monster(11, 5);
        monster.sight_radius = 4;
        let orig_x = monster.x;
        let orig_y = monster.y;
        let mut entities = vec![make_player(5, 5), monster];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster should NOT have moved — player is beyond its sight radius
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn wall_blocks_monster_sight() {
        let mut map = open_map(20, 20);
        // Build a wall column at x=7
        for y in 0..20 {
            let idx = map.idx(7, y);
            map.tiles[idx] = Tile::Wall;
        }
        let mut log = MessageLog::new();
        // Player at (5,5), monster at (9,5) — wall blocks LOS
        let monster = make_monster(9, 5);
        let orig_x = monster.x;
        let orig_y = monster.y;
        let mut entities = vec![make_player(5, 5), monster];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn different_sight_radii() {
        let map = open_map(20, 20);
        let mut log = MessageLog::new();
        // Place two monsters at distance 5 from player
        // Monster A: sight_radius=6 (can see player)
        let mut monster_a = make_monster(10, 5);
        monster_a.sight_radius = 6;
        let orig_a_x = monster_a.x;
        // Monster B: sight_radius=3 (cannot see player)
        let mut monster_b = make_monster(10, 8);
        monster_b.sight_radius = 3;
        let orig_b_x = monster_b.x;
        let orig_b_y = monster_b.y;
        let mut entities = vec![make_player(5, 5), monster_a, monster_b];
        run_monster_turns(&mut entities, &map, &mut log, &mut test_rng());
        // Monster A should have moved (can see)
        assert!(entities[1].x < orig_a_x, "monster A should chase");
        // Monster B should NOT have moved (can't see)
        assert_eq!(entities[2].x, orig_b_x);
        assert_eq!(entities[2].y, orig_b_y);
    }
}
