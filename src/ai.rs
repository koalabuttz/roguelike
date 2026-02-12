use std::collections::HashSet;

use crate::combat;
use crate::entity::{AiBehavior, Entity};
use crate::map::Map;
use crate::message_log::MessageLog;

/// Run all monster turns. Returns true if the player was killed.
pub fn run_monster_turns(
    entities: &mut Vec<Entity>,
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
    entities: &mut Vec<Entity>,
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
    entities.iter().enumerate().any(|(idx, e)| {
        idx != skip && idx != 0 && e.alive && e.x == x && e.y == y
    })
}
