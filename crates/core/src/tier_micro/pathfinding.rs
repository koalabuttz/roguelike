//! No-std BFS pathfinding for the micro tier.
//!
//! Two-pass BFS using fixed-size buffers (no alloc):
//! - **Pass 1** (`find_nearest_frontier`): Forward BFS from player through
//!   explored walkable tiles. First tile with an unexplored walkable neighbor
//!   is the target.
//! - **Pass 2** (`find_first_step`): Backward BFS from target through explored
//!   walkable tiles. First tile adjacent to the player gives the direction.
//!
//! Both passes share the same `BfsBuffers` (600-byte visited bitfield +
//! 512-byte ring queue), zeroed between passes.

use super::fov::MicroFov;
use super::map::MicroMap;
use super::types::*;
use crate::command::Direction;
use crate::rules::direction::ALL_DIRECTIONS;

/// Fixed-size buffers for BFS — reusable between passes.
///
/// 600 bytes (visited) + 512 bytes (queue) = 1,112 bytes total.
/// Allocated on the caller's stack (std adapter) or as a local in C64 code.
pub struct BfsBuffers {
    visited: [u8; MAX_BITFIELD_SIZE],
    queue: [Pos; 256],
}

impl Default for BfsBuffers {
    fn default() -> Self {
        Self::new()
    }
}

impl BfsBuffers {
    /// Create zeroed buffers.
    pub fn new() -> Self {
        Self {
            visited: [0; MAX_BITFIELD_SIZE],
            queue: [(0, 0); 256],
        }
    }

    fn clear(&mut self) {
        self.visited = [0; MAX_BITFIELD_SIZE];
    }

    fn mark_visited(&mut self, x: u8, y: u8, width: u8) {
        let idx = row_col_idx(y, x, width);
        self.visited[idx / 8] |= 1u8 << (idx % 8);
    }

    fn is_visited(&self, x: u8, y: u8, width: u8) -> bool {
        let idx = row_col_idx(y, x, width);
        self.visited[idx / 8] & (1u8 << (idx % 8)) != 0
    }
}

/// Check if (x, y) is a frontier tile: explored and walkable, with at least
/// one unexplored walkable neighbor.
fn is_frontier(x: u8, y: u8, map: &MicroMap, fov: &MicroFov) -> bool {
    if !fov.is_explored(x, y) || !map.is_walkable(x, y) {
        return false;
    }
    for &dir in &ALL_DIRECTIONS {
        let (dx, dy) = dir.to_offset();
        let nx = x as i16 + dx as i16;
        let ny = y as i16 + dy as i16;
        if nx < 0 || ny < 0 || nx >= map.width as i16 || ny >= map.height as i16 {
            continue;
        }
        let nx = nx as u8;
        let ny = ny as u8;
        if !fov.is_explored(nx, ny) && map.is_walkable(nx, ny) {
            return true;
        }
    }
    false
}

/// Pass 1: BFS from (px, py) through explored walkable tiles to find the
/// nearest frontier tile. Returns its position, or `None` if fully explored.
pub fn find_nearest_frontier(
    px: u8,
    py: u8,
    map: &MicroMap,
    fov: &MicroFov,
    buf: &mut BfsBuffers,
) -> Option<Pos> {
    buf.clear();
    buf.mark_visited(px, py, map.width);

    // Ring queue: head reads, tail writes, both wrap at 256.
    let mut head: u8 = 0;
    let mut tail: u8 = 0;
    buf.queue[tail as usize] = (px, py);
    tail = tail.wrapping_add(1);

    while head != tail {
        let (cx, cy) = buf.queue[head as usize];
        head = head.wrapping_add(1);

        if is_frontier(cx, cy, map, fov) {
            return Some((cx, cy));
        }

        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx as i16 + dx as i16;
            let ny = cy as i16 + dy as i16;
            if nx < 0 || ny < 0 || nx >= map.width as i16 || ny >= map.height as i16 {
                continue;
            }
            let nx = nx as u8;
            let ny = ny as u8;
            if buf.is_visited(nx, ny, map.width) {
                continue;
            }
            if !fov.is_explored(nx, ny) || !map.is_walkable(nx, ny) {
                continue;
            }
            buf.mark_visited(nx, ny, map.width);
            // Queue overflow: silently skip (BFS incomplete but still finds
            // nearby targets in most cases).
            if tail.wrapping_add(1) != head {
                buf.queue[tail as usize] = (nx, ny);
                tail = tail.wrapping_add(1);
            }
        }
    }

    None
}

/// Pass 2: Backward BFS from (tx, ty) through explored walkable tiles.
/// Returns the `Direction` of the first step from (px, py) toward the target,
/// or `None` if unreachable.
pub fn find_first_step(
    px: u8,
    py: u8,
    tx: u8,
    ty: u8,
    map: &MicroMap,
    fov: &MicroFov,
    buf: &mut BfsBuffers,
) -> Option<Direction> {
    if px == tx && py == ty {
        return None; // Already there.
    }

    buf.clear();
    buf.mark_visited(tx, ty, map.width);

    let mut head: u8 = 0;
    let mut tail: u8 = 0;
    buf.queue[tail as usize] = (tx, ty);
    tail = tail.wrapping_add(1);

    while head != tail {
        let (cx, cy) = buf.queue[head as usize];
        head = head.wrapping_add(1);

        // Check if any neighbor of this tile is the player position.
        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx as i16 + dx as i16;
            let ny = cy as i16 + dy as i16;
            if nx < 0 || ny < 0 || nx >= map.width as i16 || ny >= map.height as i16 {
                continue;
            }
            let nx = nx as u8;
            let ny = ny as u8;
            if nx == px && ny == py {
                // The direction FROM player TO this tile (cx, cy).
                let step_dx = cx as i32 - px as i32;
                let step_dy = cy as i32 - py as i32;
                return Direction::from_offset(step_dx, step_dy);
            }
        }

        // Expand neighbors.
        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx as i16 + dx as i16;
            let ny = cy as i16 + dy as i16;
            if nx < 0 || ny < 0 || nx >= map.width as i16 || ny >= map.height as i16 {
                continue;
            }
            let nx = nx as u8;
            let ny = ny as u8;
            if buf.is_visited(nx, ny, map.width) {
                continue;
            }
            if !fov.is_explored(nx, ny) || !map.is_walkable(nx, ny) {
                continue;
            }
            buf.mark_visited(nx, ny, map.width);
            if tail.wrapping_add(1) != head {
                buf.queue[tail as usize] = (nx, ny);
                tail = tail.wrapping_add(1);
            }
        }
    }

    None // Unreachable.
}

/// Count frontier tiles reachable from (px, py) through explored walkable area.
/// Used by the MCP server to report `frontier_count` for micro-tier games.
pub fn frontier_count(px: u8, py: u8, map: &MicroMap, fov: &MicroFov, buf: &mut BfsBuffers) -> u16 {
    buf.clear();
    buf.mark_visited(px, py, map.width);

    let mut head: u8 = 0;
    let mut tail: u8 = 0;
    buf.queue[tail as usize] = (px, py);
    tail = tail.wrapping_add(1);

    let mut count: u16 = 0;

    while head != tail {
        let (cx, cy) = buf.queue[head as usize];
        head = head.wrapping_add(1);

        if is_frontier(cx, cy, map, fov) {
            count += 1;
        }

        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx as i16 + dx as i16;
            let ny = cy as i16 + dy as i16;
            if nx < 0 || ny < 0 || nx >= map.width as i16 || ny >= map.height as i16 {
                continue;
            }
            let nx = nx as u8;
            let ny = ny as u8;
            if buf.is_visited(nx, ny, map.width) {
                continue;
            }
            if !fov.is_explored(nx, ny) || !map.is_walkable(nx, ny) {
                continue;
            }
            buf.mark_visited(nx, ny, map.width);
            if tail.wrapping_add(1) != head {
                buf.queue[tail as usize] = (nx, ny);
                tail = tail.wrapping_add(1);
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier_micro::map::{TILE_FLOOR, TILE_WALL};

    /// Build a small open map where everything is floor and fully explored.
    fn open_map_and_fov() -> (MicroMap, MicroFov) {
        let w: u8 = 20;
        let h: u8 = 15;
        let mut map = MicroMap::new(w, h);
        let mut fov = MicroFov::new(w, h);

        // Fill interior with floor, leave 1-tile border as wall.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Mark everything explored.
        for y in 0..h {
            for x in 0..w {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, &map);
                }
            }
        }
        (map, fov)
    }

    #[test]
    fn no_frontier_when_fully_explored() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        assert!(find_nearest_frontier(5, 5, &map, &fov, &mut buf).is_none());
    }

    #[test]
    fn finds_frontier_at_exploration_edge() {
        let w: u8 = 20;
        let h: u8 = 15;
        let mut map = MicroMap::new(w, h);
        // All floor.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Only explore a small area around (5, 5).
        let mut fov = MicroFov::new(w, h);
        fov.compute_fov(5, 5, &map);

        let mut buf = BfsBuffers::new();
        let frontier = find_nearest_frontier(5, 5, &map, &fov, &mut buf);
        assert!(frontier.is_some(), "should find a frontier tile");
        let (fx, fy) = frontier.unwrap();
        // Must be explored and walkable.
        assert!(fov.is_explored(fx, fy));
        assert!(map.is_walkable(fx, fy));
        // Must have an unexplored walkable neighbor.
        assert!(is_frontier(fx, fy, &map, &fov));
    }

    #[test]
    fn find_first_step_adjacent_target() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        // Target is one tile east.
        let dir = find_first_step(5, 5, 6, 5, &map, &fov, &mut buf);
        assert_eq!(dir, Some(Direction::East));
    }

    #[test]
    fn find_first_step_distant_target() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        // Target is several tiles away.
        let dir = find_first_step(3, 3, 10, 3, &map, &fov, &mut buf);
        assert!(dir.is_some(), "should find path to distant target");
        // First step should move toward the target (east).
        assert_eq!(dir, Some(Direction::East));
    }

    #[test]
    fn find_first_step_around_wall() {
        let w: u8 = 20;
        let h: u8 = 15;
        let mut map = MicroMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Wall column at x=8, y=3..10 — must go around.
        for y in 3..10 {
            map.set_tile(8, y, TILE_WALL);
        }
        // Fully explore.
        let mut fov = MicroFov::new(w, h);
        for y in 0..h {
            for x in 0..w {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, &map);
                }
            }
        }

        let mut buf = BfsBuffers::new();
        // Player at (6, 6), target at (10, 6) — wall blocks direct path.
        let dir = find_first_step(6, 6, 10, 6, &map, &fov, &mut buf);
        assert!(dir.is_some(), "should find path around wall");
        // First step should NOT be east (wall at x=8).
        // It should go north or south to get around the wall.
        let d = dir.unwrap();
        assert_ne!(d, Direction::East, "should not walk into wall");
    }

    #[test]
    fn find_first_step_same_position() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        assert!(find_first_step(5, 5, 5, 5, &map, &fov, &mut buf).is_none());
    }

    #[test]
    fn find_first_step_unreachable() {
        let w: u8 = 20;
        let h: u8 = 15;
        let mut map = MicroMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Completely wall off the target area.
        for y in 1..h - 1 {
            map.set_tile(10, y, TILE_WALL);
        }
        let mut fov = MicroFov::new(w, h);
        for y in 0..h {
            for x in 0..w {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, &map);
                }
            }
        }

        let mut buf = BfsBuffers::new();
        // Player on left side, target on right side — unreachable.
        assert!(find_first_step(5, 5, 15, 5, &map, &fov, &mut buf).is_none());
    }

    #[test]
    fn frontier_count_matches_manual() {
        let w: u8 = 20;
        let h: u8 = 15;
        let mut map = MicroMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        let mut fov = MicroFov::new(w, h);
        fov.compute_fov(5, 5, &map);

        // Count manually.
        let mut expected: u16 = 0;
        for y in 0..h {
            for x in 0..w {
                if is_frontier(x, y, &map, &fov) {
                    expected += 1;
                }
            }
        }

        let mut buf = BfsBuffers::new();
        let count = frontier_count(5, 5, &map, &fov, &mut buf);
        assert_eq!(count, expected);
    }

    #[test]
    fn bfs_on_generated_map() {
        use crate::tier_micro::game::MicroGameState;

        let state = MicroGameState::new(42, 64, 48);
        let pi = PLAYER_IDX as usize;
        let px = state.entities.x[pi];
        let py = state.entities.y[pi];

        let mut buf = BfsBuffers::new();
        // After game creation, player has FOV computed — should have frontiers.
        let frontier = find_nearest_frontier(px, py, &state.map, &state.fov, &mut buf);
        assert!(frontier.is_some(), "new game should have frontier tiles");

        let (fx, fy) = frontier.unwrap();
        let dir = find_first_step(px, py, fx, fy, &state.map, &state.fov, &mut buf);
        assert!(dir.is_some(), "should find path to frontier");
    }
}
