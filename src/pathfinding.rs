use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::map::Map;

#[derive(Eq, PartialEq)]
struct Node {
    pos: (i32, i32),
    f_score: i32,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; reverse for min-heap behavior.
        other.f_score.cmp(&self.f_score)
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Chebyshev distance — optimal heuristic for 8-directional movement
/// where all moves cost 1.
fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// A* pathfinding from (sx, sy) to (tx, ty).
///
/// Only walks on tiles that are both walkable and explored (can't pathfind
/// through fog of war). Ignores entities — the caller handles monster
/// detection during path execution.
///
/// Returns the path as a list of (x, y) positions from start to target,
/// exclusive of start. Returns `None` if no path exists.
pub fn find_path(
    map: &Map,
    sx: i32,
    sy: i32,
    tx: i32,
    ty: i32,
    explored: &HashSet<(i32, i32)>,
) -> Option<Vec<(i32, i32)>> {
    if !map.is_walkable(tx, ty) || !explored.contains(&(tx, ty)) {
        return None;
    }

    let start = (sx, sy);
    let target = (tx, ty);

    if start == target {
        return Some(Vec::new());
    }

    let mut open = BinaryHeap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), i32> = HashMap::new();

    g_score.insert(start, 0);
    open.push(Node {
        pos: start,
        f_score: chebyshev(start, target),
    });

    while let Some(Node { pos: current, .. }) = open.pop() {
        if current == target {
            let mut path = Vec::new();
            let mut node = current;
            while node != start {
                path.push(node);
                node = came_from[&node];
            }
            path.reverse();
            return Some(path);
        }

        let current_g = g_score[&current];

        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let next = (current.0 + dx, current.1 + dy);

                if !map.is_walkable(next.0, next.1) || !explored.contains(&next) {
                    continue;
                }

                let tentative_g = current_g + 1;

                if tentative_g < *g_score.get(&next).unwrap_or(&i32::MAX) {
                    came_from.insert(next, current);
                    g_score.insert(next, tentative_g);
                    open.push(Node {
                        pos: next,
                        f_score: tentative_g + chebyshev(next, target),
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Tile};

    /// Build a 10x10 map with floor from (1,1) to (8,8).
    fn open_map() -> (Map, HashSet<(i32, i32)>) {
        let mut m = Map::new(10, 10);
        let mut explored = HashSet::new();
        for y in 1..=8 {
            for x in 1..=8 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
                explored.insert((x, y));
            }
        }
        (m, explored)
    }

    #[test]
    fn path_to_self_is_empty() {
        let (m, explored) = open_map();
        let path = find_path(&m, 5, 5, 5, 5, &explored);
        assert_eq!(path, Some(Vec::new()));
    }

    #[test]
    fn path_to_adjacent() {
        let (m, explored) = open_map();
        let path = find_path(&m, 5, 5, 6, 5, &explored).unwrap();
        assert_eq!(path, vec![(6, 5)]);
    }

    #[test]
    fn path_to_diagonal() {
        let (m, explored) = open_map();
        let path = find_path(&m, 5, 5, 7, 7, &explored).unwrap();
        // Chebyshev optimal: 2 diagonal steps
        assert_eq!(path.len(), 2);
        assert_eq!(*path.last().unwrap(), (7, 7));
    }

    #[test]
    fn path_around_wall() {
        let mut m = Map::new(10, 10);
        let mut explored = HashSet::new();
        // Floor everywhere except a wall barrier at x=5, y=3..=7
        for y in 1..=8 {
            for x in 1..=8 {
                if x == 5 && (3..=7).contains(&y) {
                    continue; // wall barrier
                }
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
                explored.insert((x, y));
            }
        }

        let path = find_path(&m, 3, 5, 7, 5, &explored).unwrap();
        // Must go around the barrier — path length > 4 (direct Chebyshev)
        assert!(path.len() > 4);
        assert_eq!(*path.last().unwrap(), (7, 5));
        // Verify no step crosses the wall
        for &(x, y) in &path {
            assert!(m.is_walkable(x, y));
        }
    }

    #[test]
    fn no_path_to_wall() {
        let (m, explored) = open_map();
        // (0,0) is a wall
        let path = find_path(&m, 5, 5, 0, 0, &explored);
        assert_eq!(path, None);
    }

    #[test]
    fn no_path_through_unexplored() {
        let mut m = Map::new(20, 10);
        let mut explored = HashSet::new();
        // Two separate explored zones with walkable tiles between them,
        // but the middle tiles are unexplored.
        for x in 1..=5 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
            explored.insert((x, 5));
        }
        for x in 8..=12 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
            explored.insert((x, 5));
        }
        // x=6,7 are walkable but NOT explored
        for x in 6..=7 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }

        let path = find_path(&m, 3, 5, 10, 5, &explored);
        assert_eq!(path, None);
    }

    #[test]
    fn path_in_corridor() {
        let mut m = Map::new(20, 10);
        let mut explored = HashSet::new();
        for x in 1..=18 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
            explored.insert((x, 5));
        }

        let path = find_path(&m, 1, 5, 18, 5, &explored).unwrap();
        assert_eq!(path.len(), 17);
        assert_eq!(*path.last().unwrap(), (18, 5));
    }
}
