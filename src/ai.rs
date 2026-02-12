use std::collections::HashSet;

use crate::combat;
use crate::entity::{AiBehavior, Entity};
use crate::map::Map;
use crate::message_log::MessageLog;

/// Run all monster turns. Returns true if the player was killed.
pub fn run_monster_turns(
    entities: &mut [Entity],
    map: &Map,
    visible: &HashSet<(i32, i32)>,
    log: &mut MessageLog,
) -> bool {
    let px = entities[0].x;
    let py = entities[0].y;

    for i in 1..entities.len() {
        if !entities[i].alive {
            continue;
        }

        // Only act if monster is visible (wake on sight)
        if !visible.contains(&(entities[i].x, entities[i].y)) {
            continue;
        }

        match entities[i].ai {
            AiBehavior::Chase => chase_ai(entities, i, px, py, map, log),
            AiBehavior::None => {}
        }
    }

    !entities[0].alive
}

fn chase_ai(
    entities: &mut [Entity],
    idx: usize,
    px: i32,
    py: i32,
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

fn is_occupied_by_monster(entities: &[Entity], x: i32, y: i32, skip: usize) -> bool {
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

    /// Make all positions in the map visible.
    fn full_visibility(w: i32, h: i32) -> HashSet<(i32, i32)> {
        let mut vis = HashSet::new();
        for y in 0..h {
            for x in 0..w {
                vis.insert((x, y));
            }
        }
        vis
    }

    fn make_player(x: i32, y: i32) -> Entity {
        Entity::player(x, y)
    }

    fn make_monster(x: i32, y: i32) -> Entity {
        Entity::from_template(&data::GOBLIN, x, y)
    }

    #[test]
    fn dead_monsters_are_skipped() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();
        let mut dead_monster = make_monster(3, 3);
        dead_monster.alive = false;
        let orig_x = dead_monster.x;
        let orig_y = dead_monster.y;

        let mut entities = vec![make_player(5, 5), dead_monster];
        run_monster_turns(&mut entities, &map, &vis, &mut log);
        // Dead monster should not have moved
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn non_visible_monsters_are_skipped() {
        let map = open_map(10, 10);
        let vis = HashSet::new(); // nothing visible
        let mut log = MessageLog::new();
        let monster = make_monster(3, 3);
        let orig_x = monster.x;
        let orig_y = monster.y;

        let mut entities = vec![make_player(5, 5), monster];
        run_monster_turns(&mut entities, &map, &vis, &mut log);
        assert_eq!(entities[1].x, orig_x);
        assert_eq!(entities[1].y, orig_y);
    }

    #[test]
    fn adjacent_monster_attacks_player() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(5, 4)]; // adjacent
        let player_hp_before = entities[0].hp;
        run_monster_turns(&mut entities, &map, &vis, &mut log);
        // Monster should have attacked, reducing player HP (goblin atk=3, player def=2, dmg=1)
        assert!(entities[0].hp < player_hp_before);
    }

    #[test]
    fn non_adjacent_monster_moves_toward_player() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(2, 2)]; // far away
        run_monster_turns(&mut entities, &map, &vis, &mut log);
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

        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 3), make_monster(3, 3)];
        run_monster_turns(&mut entities, &map, &vis, &mut log);
        // Monster should not be at (4, 3) — that's a wall
        assert!(!(entities[1].x == 4 && entities[1].y == 3));
    }

    #[test]
    fn monster_does_not_walk_through_other_monsters() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        // Monster at (3,5) wants to go toward player at (5,5).
        // Another monster at (4,5) blocks the direct path.
        let mut entities = vec![make_player(5, 5), make_monster(3, 5), make_monster(4, 5)];
        run_monster_turns(&mut entities, &map, &vis, &mut log);
        // First monster (index 1) should not end up at (4,5) — that's where monster 2 is
        assert!(!(entities[1].x == 4 && entities[1].y == 5));
    }

    #[test]
    fn returns_true_when_player_dies() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        let mut player = make_player(5, 5);
        player.hp = 1; // Nearly dead
        player.defense = 0;
        let mut entities = vec![player, make_monster(5, 4)]; // adjacent goblin, atk=3
        let player_died = run_monster_turns(&mut entities, &map, &vis, &mut log);
        assert!(player_died);
        assert!(!entities[0].alive);
    }

    #[test]
    fn returns_false_when_player_alive() {
        let map = open_map(10, 10);
        let vis = full_visibility(10, 10);
        let mut log = MessageLog::new();

        let mut entities = vec![make_player(5, 5), make_monster(5, 4)];
        let player_died = run_monster_turns(&mut entities, &map, &vis, &mut log);
        assert!(!player_died);
    }
}
