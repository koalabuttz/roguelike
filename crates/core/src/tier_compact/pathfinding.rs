//! No-std BFS pathfinding for the compact tier (GBA).
//!
//! Two-pass BFS using fixed-size buffers (no alloc):
//! - **Pass 1** (`find_nearest_frontier`): Forward BFS from player through
//!   explored walkable tiles. First tile with an unexplored walkable neighbor
//!   is the target.
//! - **Pass 2** (`find_first_step`): Backward BFS from target through explored
//!   walkable tiles. First tile adjacent to the player gives the direction.
//!
//! Both passes share the same `BfsBuffers` (400-byte visited bitfield +
//! 2048-byte ring queue), zeroed between passes.

use super::fov::CompactFov;
use super::map::CompactMap;
use super::types::*;
use crate::command::Direction;
use crate::rules::direction::ALL_DIRECTIONS;

/// Fixed-size buffers for BFS — reusable between passes.
///
/// 400 bytes (visited) + 2048 bytes (queue) = ~2,450 bytes total.
pub struct BfsBuffers {
    visited: [u8; BITFIELD_SIZE],
    queue: [Pos; 256],
}

impl Default for BfsBuffers {
    fn default() -> Self {
        Self::new()
    }
}

impl BfsBuffers {
    pub fn new() -> Self {
        Self {
            visited: [0; BITFIELD_SIZE],
            queue: [(0, 0); 256],
        }
    }

    fn clear(&mut self) {
        self.visited = [0; BITFIELD_SIZE];
    }

    fn mark_visited(&mut self, x: Coord, y: Coord, width: Coord) {
        let idx = (y * width + x) as usize;
        self.visited[idx >> 3] |= 1u8 << (idx & 7);
    }

    fn is_visited(&self, x: Coord, y: Coord, width: Coord) -> bool {
        let idx = (y * width + x) as usize;
        self.visited[idx >> 3] & (1u8 << (idx & 7)) != 0
    }
}

/// Check if (x, y) is a frontier tile: explored and walkable, with at least
/// one unexplored walkable neighbor.
fn is_frontier(x: Coord, y: Coord, map: &CompactMap, fov: &CompactFov) -> bool {
    if !fov.is_explored(x, y) || !map.is_walkable(x, y) {
        return false;
    }
    for &dir in &ALL_DIRECTIONS {
        let (dx, dy) = dir.to_offset();
        let nx = x + dx as Coord;
        let ny = y + dy as Coord;
        if !map.in_bounds(nx, ny) {
            continue;
        }
        if !fov.is_explored(nx, ny) && map.is_walkable(nx, ny) {
            return true;
        }
    }
    false
}

/// Pass 1: BFS from (px, py) through explored walkable tiles to find the
/// nearest frontier tile. Returns its position, or `None` if fully explored.
pub fn find_nearest_frontier(
    px: Coord,
    py: Coord,
    map: &CompactMap,
    fov: &CompactFov,
    buf: &mut BfsBuffers,
) -> Option<Pos> {
    buf.clear();
    buf.mark_visited(px, py, map.width);

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
            let nx = cx + dx as Coord;
            let ny = cy + dy as Coord;
            if !map.in_bounds(nx, ny) {
                continue;
            }
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

    None
}

/// Pass 2: Backward BFS from (tx, ty) through explored walkable tiles.
/// Returns the `Direction` of the first step from (px, py) toward the target,
/// or `None` if unreachable.
pub fn find_first_step(
    px: Coord,
    py: Coord,
    tx: Coord,
    ty: Coord,
    map: &CompactMap,
    fov: &CompactFov,
    buf: &mut BfsBuffers,
) -> Option<Direction> {
    if px == tx && py == ty {
        return None;
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

        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx + dx as Coord;
            let ny = cy + dy as Coord;
            if !map.in_bounds(nx, ny) {
                continue;
            }
            if nx == px && ny == py {
                let step_dx = cx - px;
                let step_dy = cy - py;
                return Direction::from_offset(step_dx, step_dy);
            }
        }

        for &dir in &ALL_DIRECTIONS {
            let (dx, dy) = dir.to_offset();
            let nx = cx + dx as Coord;
            let ny = cy + dy as Coord;
            if !map.in_bounds(nx, ny) {
                continue;
            }
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

    None
}

/// Count frontier tiles reachable from (px, py) through explored walkable area.
pub fn frontier_count(
    px: Coord,
    py: Coord,
    map: &CompactMap,
    fov: &CompactFov,
    buf: &mut BfsBuffers,
) -> u16 {
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
            let nx = cx + dx as Coord;
            let ny = cy + dy as Coord;
            if !map.in_bounds(nx, ny) {
                continue;
            }
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
    use crate::rules::balance;
    use crate::tier_compact::map::TILE_FLOOR;

    /// Build a small open map where everything is floor and fully explored.
    fn open_map_and_fov() -> (CompactMap, CompactFov) {
        let w: Coord = 20;
        let h: Coord = 15;
        let mut map = CompactMap::new(w, h);
        let mut fov = CompactFov::new(w, h);

        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        // Mark everything explored by computing FOV from many points.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, balance::FOV_RADIUS, &map);
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
        let w: Coord = 20;
        let h: Coord = 15;
        let mut map = CompactMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        let mut fov = CompactFov::new(w, h);
        fov.compute_fov(5, 5, balance::FOV_RADIUS, &map);

        let mut buf = BfsBuffers::new();
        let frontier = find_nearest_frontier(5, 5, &map, &fov, &mut buf);
        assert!(frontier.is_some(), "should find a frontier tile");
        let (fx, fy) = frontier.unwrap();
        assert!(fov.is_explored(fx, fy));
        assert!(map.is_walkable(fx, fy));
        assert!(is_frontier(fx, fy, &map, &fov));
    }

    #[test]
    fn find_first_step_adjacent_target() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        let dir = find_first_step(5, 5, 6, 5, &map, &fov, &mut buf);
        assert_eq!(dir, Some(Direction::East));
    }

    #[test]
    fn find_first_step_distant_target() {
        let (map, fov) = open_map_and_fov();
        let mut buf = BfsBuffers::new();
        let dir = find_first_step(3, 3, 10, 3, &map, &fov, &mut buf);
        assert!(dir.is_some(), "should find path to distant target");
        assert_eq!(dir, Some(Direction::East));
    }

    #[test]
    fn find_first_step_around_wall() {
        let w: Coord = 20;
        let h: Coord = 15;
        let mut map = CompactMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        use super::super::map::TILE_WALL;
        for y in 3..10 {
            map.set_tile(8, y, TILE_WALL);
        }
        let mut fov = CompactFov::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, balance::FOV_RADIUS, &map);
                }
            }
        }

        let mut buf = BfsBuffers::new();
        let dir = find_first_step(6, 6, 10, 6, &map, &fov, &mut buf);
        assert!(dir.is_some(), "should find path around wall");
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
        let w: Coord = 20;
        let h: Coord = 15;
        let mut map = CompactMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        use super::super::map::TILE_WALL;
        for y in 1..h - 1 {
            map.set_tile(10, y, TILE_WALL);
        }
        let mut fov = CompactFov::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if map.is_walkable(x, y) {
                    fov.compute_fov(x, y, balance::FOV_RADIUS, &map);
                }
            }
        }

        let mut buf = BfsBuffers::new();
        assert!(find_first_step(5, 5, 15, 5, &map, &fov, &mut buf).is_none());
    }

    #[test]
    fn frontier_count_matches_manual() {
        let w: Coord = 20;
        let h: Coord = 15;
        let mut map = CompactMap::new(w, h);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                map.set_tile(x, y, TILE_FLOOR);
            }
        }
        let mut fov = CompactFov::new(w, h);
        fov.compute_fov(5, 5, balance::FOV_RADIUS, &map);

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
}
