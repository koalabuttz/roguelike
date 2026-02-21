// Monster AI — awareness checks and movement.
//
// Ports the Rust ai.rs: each monster checks line-of-sight to the player,
// then either chases (greedy movement toward player) or wanders randomly.
//
// Key difference from Rust version: monsters use single-ray Bresenham LOS
// instead of full shadowcasting for awareness checks. Much cheaper on 6502
// (~200 cycles per monster vs thousands for shadowcasting).

use crate::entity;
use crate::fov;
use crate::map;
use crate::prng;
use crate::combat;
use crate::msglog;

/// Signum function for i8.
#[inline(always)]
fn signum(v: i8) -> i8 {
    if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
}

/// Run all monster turns. Returns true if the player died.
pub fn run_monster_turns() -> bool {
    let px = entity::x(entity::PLAYER_IDX);
    let py = entity::y(entity::PLAYER_IDX);

    let count = entity::count();
    for i in 1..count {
        if !entity::is_alive(i) { continue; }

        let mx = entity::x(i);
        let my = entity::y(i);
        let behavior = entity::ai(i);

        // Check awareness via single-ray LOS
        let aware = match behavior {
            entity::AI_CHASE | entity::AI_WANDER => {
                fov::can_see(mx, my, px, py, entity::sight(i))
            }
            _ => false,
        };

        match behavior {
            entity::AI_CHASE => {
                if aware {
                    chase(i, px, py);
                }
            }
            entity::AI_WANDER => {
                if aware {
                    // Wander → Chase transition
                    entity::set_ai(i, entity::AI_CHASE);
                    msglog::add_notice_msg(i);
                    chase(i, px, py);
                } else {
                    wander(i);
                }
            }
            _ => {}
        }

        // Check if player died from this monster's attack
        if !entity::is_alive(entity::PLAYER_IDX) {
            return true;
        }
    }
    false
}

/// Greedy chase: try diagonal, then horizontal, then vertical toward player.
fn chase(idx: u8, px: u8, py: u8) {
    let mx = entity::x(idx);
    let my = entity::y(idx);

    let dx = (px as i8) - (mx as i8);
    let dy = (py as i8) - (my as i8);
    let dist_x = if dx > 0 { dx } else { -dx };
    let dist_y = if dy > 0 { dy } else { -dy };

    // If adjacent, attack instead of moving
    if dist_x <= 1 && dist_y <= 1 {
        combat::melee_attack(idx, entity::PLAYER_IDX);
        return;
    }

    let sx = signum(dx);
    let sy = signum(dy);

    // Try three movement candidates in order of preference
    let candidates: [(i8, i8); 3] = [
        (sx, sy),  // diagonal (preferred)
        (sx, 0),   // horizontal
        (0, sy),   // vertical
    ];

    for &(cx, cy) in candidates.iter() {
        if cx == 0 && cy == 0 { continue; }
        let nx = (mx as i8 + cx) as u8;
        let ny = (my as i8 + cy) as u8;
        if map::is_walkable(nx, ny) && !entity::is_occupied(nx, ny, idx) {
            entity::set_pos(idx, nx, ny);
            return;
        }
    }
}

/// Random walk: pick a random walkable, unoccupied neighbor.
fn wander(idx: u8) {
    let mx = entity::x(idx);
    let my = entity::y(idx);
    let px = entity::x(entity::PLAYER_IDX);
    let py = entity::y(entity::PLAYER_IDX);

    // Collect walkable neighbors (up to 8)
    let mut candidates: [(u8, u8); 8] = [(0, 0); 8];
    let mut count: u8 = 0;

    for dy in -1i8..=1 {
        for dx in -1i8..=1 {
            if dx == 0 && dy == 0 { continue; }
            let nx = (mx as i8 + dx) as u8;
            let ny = (my as i8 + dy) as u8;
            // Don't walk into the player
            if nx == px && ny == py { continue; }
            if map::is_walkable(nx, ny) && !entity::is_occupied(nx, ny, idx) {
                candidates[count as usize] = (nx, ny);
                count += 1;
            }
        }
    }

    if count > 0 {
        let pick = prng::range(0, count - 1);
        let (nx, ny) = candidates[pick as usize];
        entity::set_pos(idx, nx, ny);
    }
}
