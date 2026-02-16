use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::types::{Coord, Pos};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Tile {
    Wall,
    Floor,
}

impl Tile {
    /// Movement cost for walking onto this tile. Extend this when adding
    /// terrain types (e.g., Water → 2, Swamp → 3, Lava → 5).
    /// Walls are not walkable — callers must check `is_walkable` first.
    pub fn move_cost(&self) -> i32 {
        match self {
            Tile::Floor => 1,
            Tile::Wall => unreachable!("walls are not walkable"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Rect {
    pub x1: Coord,
    pub y1: Coord,
    pub x2: Coord,
    pub y2: Coord,
    /// Hidden rooms are carved into the map but not counted in stats
    /// until discovered. Prevents exploration percentage from leaking
    /// the existence of secret areas.
    pub hidden: bool,
}

impl Rect {
    pub fn new(x: Coord, y: Coord, w: Coord, h: Coord) -> Self {
        Rect {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
            hidden: false,
        }
    }

    pub fn center(&self) -> Pos {
        ((self.x1 + self.x2) / 2, (self.y1 + self.y2) / 2)
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x1 <= other.x2 && self.x2 >= other.x1 && self.y1 <= other.y2 && self.y2 >= other.y1
    }

    /// Whether (x, y) is in the carved interior of this room.
    /// Matches `carve_room` bounds: x1 < x < x2, y1 < y < y2.
    pub fn contains_interior(&self, x: Coord, y: Coord) -> bool {
        x > self.x1 && x < self.x2 && y > self.y1 && y < self.y2
    }
}

#[derive(Serialize, Deserialize)]
pub struct Map {
    pub width: Coord,
    pub height: Coord,
    pub tiles: Vec<Tile>,
    pub rooms: Vec<Rect>,
    /// Precomputed: `true` for wall tiles adjacent to at least one floor tile.
    /// Computed once after generation via `compute_structural_walls()`.
    /// Skipped in serialization — rebuilt on load.
    #[serde(skip)]
    pub structural: Vec<bool>,
}

impl Map {
    pub fn new(width: Coord, height: Coord) -> Self {
        let n = (width * height) as usize;
        Map {
            width,
            height,
            tiles: vec![Tile::Wall; n],
            rooms: Vec::new(),
            structural: vec![false; n],
        }
    }

    /// Precompute which wall tiles are structural (adjacent to floor).
    /// Call once after all carving is complete. O(W*H) one-time cost,
    /// then `structural[idx]` is O(1) per tile during rendering.
    pub fn compute_structural_walls(&mut self) {
        let n = (self.width * self.height) as usize;
        self.structural = vec![false; n];
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = self.idx(x, y);
                if self.tiles[idx] != Tile::Wall {
                    continue;
                }
                'neighbors: for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        if self.in_bounds(nx, ny) && self.tiles[self.idx(nx, ny)] == Tile::Floor {
                            self.structural[idx] = true;
                            break 'neighbors;
                        }
                    }
                }
            }
        }
    }

    pub fn idx(&self, x: Coord, y: Coord) -> usize {
        (y * self.width + x) as usize
    }

    pub fn in_bounds(&self, x: Coord, y: Coord) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    pub fn is_walkable(&self, x: Coord, y: Coord) -> bool {
        self.in_bounds(x, y) && self.tiles[self.idx(x, y)] == Tile::Floor
    }

    /// Count walkable tiles adjacent to (x, y), excluding one direction.
    ///
    /// Useful for corridor topology detection: in a straight 1-wide corridor,
    /// the count excluding "behind" is exactly 1. At junctions or room
    /// entrances, it's 2+. At dead ends, it's 0.
    pub fn open_neighbors_excluding(
        &self,
        x: Coord,
        y: Coord,
        exclude_dx: Coord,
        exclude_dy: Coord,
    ) -> i32 {
        let mut count = 0;
        for ny in -1..=1 {
            for nx in -1..=1 {
                if nx == 0 && ny == 0 {
                    continue;
                }
                if nx == exclude_dx && ny == exclude_dy {
                    continue;
                }
                if self.is_walkable(x + nx, y + ny) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Count the total number of floor tiles on the map.
    pub fn floor_count(&self) -> i32 {
        self.tiles.iter().filter(|t| **t == Tile::Floor).count() as i32
    }

    /// Whether (x, y) is inside any room's interior.
    pub fn is_in_room(&self, x: Coord, y: Coord) -> bool {
        self.rooms.iter().any(|r| r.contains_interior(x, y))
    }

    /// Count floor tiles excluding those inside hidden rooms.
    /// Use this for exploration stats so hidden rooms don't leak info.
    pub fn known_floor_count(&self) -> i32 {
        let mut count = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.tiles[self.idx(x, y)] == Tile::Floor
                    && !self
                        .rooms
                        .iter()
                        .any(|r| r.hidden && r.contains_interior(x, y))
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Count non-hidden rooms.
    pub fn known_room_count(&self) -> i32 {
        self.rooms.iter().filter(|r| !r.hidden).count() as i32
    }

    pub(crate) fn carve_room(&mut self, room: &Rect) {
        for y in (room.y1 + 1)..room.y2 {
            for x in (room.x1 + 1)..room.x2 {
                if self.in_bounds(x, y) {
                    let idx = self.idx(x, y);
                    self.tiles[idx] = Tile::Floor;
                }
            }
        }
    }

    fn carve_h_tunnel(&mut self, x1: Coord, x2: Coord, y: Coord) {
        for x in x1.min(x2)..=x1.max(x2) {
            if self.in_bounds(x, y) {
                let idx = self.idx(x, y);
                self.tiles[idx] = Tile::Floor;
            }
        }
    }

    fn carve_v_tunnel(&mut self, y1: Coord, y2: Coord, x: Coord) {
        for y in y1.min(y2)..=y1.max(y2) {
            if self.in_bounds(x, y) {
                let idx = self.idx(x, y);
                self.tiles[idx] = Tile::Floor;
            }
        }
    }

    /// Generate a dungeon with random rooms connected by corridors.
    /// Returns the player's starting position (center of the first room).
    pub fn generate(
        &mut self,
        max_rooms: i32,
        min_size: Coord,
        max_size: Coord,
        rng: &mut impl Rng,
    ) -> Pos {
        let mut player_start = (0, 0);

        for _ in 0..max_rooms {
            let w = rng.gen_range(min_size..=max_size);
            let h = rng.gen_range(min_size..=max_size);
            let x = rng.gen_range(1..self.width - w - 1);
            let y = rng.gen_range(1..self.height - h - 1);

            let new_room = Rect::new(x, y, w, h);

            if self.rooms.iter().any(|r| r.intersects(&new_room)) {
                continue;
            }

            self.carve_room(&new_room);

            let (new_cx, new_cy) = new_room.center();

            if self.rooms.is_empty() {
                player_start = (new_cx, new_cy);
            } else {
                let (prev_cx, prev_cy) = self.rooms.last().unwrap().center();
                if rng.gen_bool(0.5) {
                    self.carve_h_tunnel(prev_cx, new_cx, prev_cy);
                    self.carve_v_tunnel(prev_cy, new_cy, new_cx);
                } else {
                    self.carve_v_tunnel(prev_cy, new_cy, prev_cx);
                    self.carve_h_tunnel(prev_cx, new_cx, new_cy);
                }
            }

            self.rooms.push(new_room);
        }

        player_start
    }

    /// Generate a map from a named preset. Returns the player start position.
    ///
    /// Presets are deterministic given a seed and produce specific topologies
    /// useful for targeted testing and development.
    pub fn from_preset(&mut self, preset: MapPreset, rng: &mut impl Rng) -> Pos {
        match preset {
            MapPreset::Arena => self.gen_arena(rng),
            MapPreset::Corridor => self.gen_corridor(),
            MapPreset::Labyrinth => self.gen_labyrinth(rng),
            MapPreset::SingleRoom => self.gen_single_room(),
            MapPreset::OpenField => self.gen_open_field(),
        }
    }

    /// One large room filling most of the map with monsters spawning inside.
    fn gen_arena(&mut self, rng: &mut impl Rng) -> Pos {
        let room = Rect::new(1, 1, self.width - 3, self.height - 3);
        self.carve_room(&room);
        self.rooms.push(room);

        // Add a second room in the same space so monsters spawn
        // (spawn_monsters skips room 0).
        let spawn_room = Rect::new(1, 1, self.width - 3, self.height - 3);
        self.rooms.push(spawn_room);

        let _ = rng; // suppress unused parameter warning — rng is in the signature for trait consistency
        let (cx, cy) = self.rooms[0].center();
        (cx, cy)
    }

    /// Long horizontal corridor spanning the map.
    fn gen_corridor(&mut self) -> Pos {
        let mid_y = self.height / 2;
        for x in 1..self.width - 1 {
            let idx = self.idx(x, mid_y);
            self.tiles[idx] = Tile::Floor;
        }
        // Register as rooms so spawn works.
        let left = Rect::new(0, mid_y - 1, self.width / 3, 2);
        let right = Rect::new(self.width * 2 / 3, mid_y - 1, self.width / 3 - 1, 2);
        // Carve room floors too (so interior check works).
        self.carve_room(&left);
        self.carve_room(&right);
        self.rooms.push(left);
        self.rooms.push(right);
        (2, mid_y)
    }

    /// Dense maze with many corridors and intersections.
    fn gen_labyrinth(&mut self, rng: &mut impl Rng) -> Pos {
        // Use many small rooms connected by corridors.
        self.generate(
            50, // many rooms
            2,  // tiny rooms
            4,  // small max
            rng,
        )
    }

    /// Minimal map: one small room.
    fn gen_single_room(&mut self) -> Pos {
        let room = Rect::new(2, 2, 6, 6);
        self.carve_room(&room);
        self.rooms.push(room);
        self.rooms[0].center()
    }

    /// Large open area with no walls (except border).
    fn gen_open_field(&mut self) -> Pos {
        for y in 1..self.height - 1 {
            for x in 1..self.width - 1 {
                let idx = self.idx(x, y);
                self.tiles[idx] = Tile::Floor;
            }
        }
        let room = Rect::new(0, 0, self.width - 1, self.height - 1);
        self.rooms.push(room);
        (self.width / 2, self.height / 2)
    }
}

/// Named map generation presets for development and testing.
///
/// Each preset produces a specific map topology useful for targeted scenarios:
/// combat testing, pathfinding validation, FOV edge cases, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapPreset {
    /// One large room filling the map — good for combat testing.
    Arena,
    /// Long horizontal corridor — good for pathfinding / FOV testing.
    Corridor,
    /// Dense maze of tiny rooms — good for auto-explore testing.
    Labyrinth,
    /// One small room — minimal scenario for unit tests.
    SingleRoom,
    /// Huge open area — good for FOV performance testing.
    OpenField,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn rect_center() {
        let r = Rect::new(0, 0, 10, 6);
        assert_eq!(r.center(), (5, 3));
    }

    #[test]
    fn rect_center_odd_dimensions() {
        let r = Rect::new(1, 1, 5, 5);
        // x1=1, x2=6, y1=1, y2=6 → center (3,3)
        assert_eq!(r.center(), (3, 3));
    }

    #[test]
    fn rect_intersects_overlapping() {
        let a = Rect::new(0, 0, 5, 5);
        let b = Rect::new(3, 3, 5, 5);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn rect_intersects_adjacent() {
        // Rects sharing an edge — x2 of a == x1 of b
        let a = Rect::new(0, 0, 5, 5);
        let b = Rect::new(5, 0, 5, 5);
        assert!(a.intersects(&b)); // touching counts as intersecting
    }

    #[test]
    fn rect_no_intersect_disjoint() {
        let a = Rect::new(0, 0, 3, 3);
        let b = Rect::new(10, 10, 3, 3);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn map_new_all_walls() {
        let m = Map::new(10, 10);
        assert!(m.tiles.iter().all(|t| *t == Tile::Wall));
        assert_eq!(m.tiles.len(), 100);
        assert!(m.rooms.is_empty());
    }

    #[test]
    fn map_idx_correct() {
        let m = Map::new(80, 50);
        assert_eq!(m.idx(0, 0), 0);
        assert_eq!(m.idx(1, 0), 1);
        assert_eq!(m.idx(0, 1), 80);
        assert_eq!(m.idx(5, 3), 3 * 80 + 5);
    }

    #[test]
    fn in_bounds_interior() {
        let m = Map::new(80, 50);
        assert!(m.in_bounds(0, 0));
        assert!(m.in_bounds(79, 49));
        assert!(m.in_bounds(40, 25));
    }

    #[test]
    fn in_bounds_out_of_range() {
        let m = Map::new(80, 50);
        assert!(!m.in_bounds(-1, 0));
        assert!(!m.in_bounds(0, -1));
        assert!(!m.in_bounds(80, 0));
        assert!(!m.in_bounds(0, 50));
    }

    #[test]
    fn is_walkable_wall() {
        let m = Map::new(10, 10);
        assert!(!m.is_walkable(5, 5)); // all walls
    }

    #[test]
    fn is_walkable_floor() {
        let mut m = Map::new(10, 10);
        let idx = m.idx(5, 5);
        m.tiles[idx] = Tile::Floor;
        assert!(m.is_walkable(5, 5));
    }

    #[test]
    fn is_walkable_out_of_bounds() {
        let m = Map::new(10, 10);
        assert!(!m.is_walkable(-1, -1));
        assert!(!m.is_walkable(100, 100));
    }

    #[test]
    fn generate_creates_at_least_one_room() {
        let mut m = Map::new(80, 50);
        let mut rng = StdRng::seed_from_u64(42);
        m.generate(30, 4, 10, &mut rng);
        assert!(!m.rooms.is_empty());
    }

    #[test]
    fn generate_player_start_is_walkable() {
        let mut m = Map::new(80, 50);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.generate(30, 4, 10, &mut rng);
        assert!(m.is_walkable(px, py));
    }

    #[test]
    fn open_neighbors_in_corridor() {
        // Horizontal corridor at y=5
        let mut m = Map::new(10, 10);
        for x in 1..=8 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Middle of corridor: excluding west (behind), only east is open → 1
        assert_eq!(m.open_neighbors_excluding(5, 5, -1, 0), 1);
        // Dead end at x=8: excluding west, east is wall → 0
        assert_eq!(m.open_neighbors_excluding(8, 5, -1, 0), 0);
    }

    #[test]
    fn open_neighbors_at_junction() {
        let mut m = Map::new(10, 10);
        // Horizontal corridor at y=5
        for x in 1..=8 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Branch going north at x=5
        for y in 1..=4 {
            let idx = m.idx(5, y);
            m.tiles[idx] = Tile::Floor;
        }
        // At the junction: excluding west (behind), east and north are open → 2
        assert_eq!(m.open_neighbors_excluding(5, 5, -1, 0), 2);
    }

    #[test]
    fn floor_count_empty_map() {
        let m = Map::new(10, 10);
        assert_eq!(m.floor_count(), 0);
    }

    #[test]
    fn floor_count_with_floors() {
        let mut m = Map::new(10, 10);
        for x in 1..=5 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        assert_eq!(m.floor_count(), 5);
    }

    #[test]
    fn contains_interior_inside() {
        let r = Rect::new(2, 2, 4, 4); // x1=2, y1=2, x2=6, y2=6
        // Interior is 3..=5 x 3..=5
        assert!(r.contains_interior(3, 3));
        assert!(r.contains_interior(5, 5));
    }

    #[test]
    fn contains_interior_on_wall() {
        let r = Rect::new(2, 2, 4, 4);
        // Walls are at x=2, x=6, y=2, y=6
        assert!(!r.contains_interior(2, 3));
        assert!(!r.contains_interior(6, 3));
        assert!(!r.contains_interior(3, 2));
        assert!(!r.contains_interior(3, 6));
    }

    #[test]
    fn contains_interior_outside() {
        let r = Rect::new(2, 2, 4, 4);
        assert!(!r.contains_interior(0, 0));
        assert!(!r.contains_interior(10, 10));
    }

    #[test]
    fn known_floor_count_equals_floor_count_without_hidden() {
        let mut m = Map::new(80, 50);
        let mut rng = StdRng::seed_from_u64(42);
        m.generate(30, 4, 10, &mut rng);
        // No hidden rooms → known == total
        assert_eq!(m.known_floor_count(), m.floor_count());
    }

    #[test]
    fn known_floor_count_excludes_hidden_rooms() {
        let mut m = Map::new(20, 20);
        // Carve two rooms manually
        let room1 = Rect::new(1, 1, 4, 4); // interior: 2..=4 x 2..=4 = 9 tiles
        m.carve_room(&room1);
        m.rooms.push(room1);

        let mut room2 = Rect::new(10, 10, 4, 4); // interior: 11..=13 x 11..=13 = 9 tiles
        room2.hidden = true;
        m.carve_room(&room2);
        m.rooms.push(room2);

        assert_eq!(m.floor_count(), 18); // both rooms
        assert_eq!(m.known_floor_count(), 9); // only room1
    }

    #[test]
    fn known_room_count_excludes_hidden() {
        let mut m = Map::new(20, 20);
        m.rooms.push(Rect::new(1, 1, 4, 4));
        let mut hidden = Rect::new(10, 10, 4, 4);
        hidden.hidden = true;
        m.rooms.push(hidden);
        assert_eq!(m.known_room_count(), 1);
    }

    #[test]
    fn is_in_room_interior() {
        let mut m = Map::new(20, 20);
        let room = Rect::new(2, 2, 6, 6); // interior: 3..=7 x 3..=7
        m.rooms.push(room);
        assert!(m.is_in_room(5, 5));
        assert!(!m.is_in_room(2, 5)); // wall edge
        assert!(!m.is_in_room(0, 0)); // outside
    }

    #[test]
    fn is_in_room_corridor() {
        let m = Map::new(20, 20);
        // No rooms registered — corridor tiles are never "in a room"
        assert!(!m.is_in_room(5, 5));
    }

    #[test]
    fn generate_rooms_dont_overlap() {
        let mut m = Map::new(80, 50);
        let mut rng = StdRng::seed_from_u64(42);
        m.generate(30, 4, 10, &mut rng);
        for i in 0..m.rooms.len() {
            for j in (i + 1)..m.rooms.len() {
                assert!(
                    !m.rooms[i].intersects(&m.rooms[j]),
                    "rooms {} and {} overlap",
                    i,
                    j
                );
            }
        }
    }

    // --- Map preset tests ---

    #[test]
    fn preset_arena_has_walkable_center() {
        let mut m = Map::new(40, 30);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.from_preset(MapPreset::Arena, &mut rng);
        assert!(m.is_walkable(px, py));
        assert!(m.floor_count() > 100);
    }

    #[test]
    fn preset_corridor_is_horizontal() {
        let mut m = Map::new(40, 20);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.from_preset(MapPreset::Corridor, &mut rng);
        assert!(m.is_walkable(px, py));
        let mid_y = 20 / 2;
        // The corridor should span the middle row.
        for x in 1..39 {
            assert!(
                m.is_walkable(x, mid_y),
                "tile ({}, {}) should be walkable in corridor",
                x,
                mid_y
            );
        }
    }

    #[test]
    fn preset_labyrinth_has_rooms() {
        let mut m = Map::new(80, 50);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.from_preset(MapPreset::Labyrinth, &mut rng);
        assert!(m.is_walkable(px, py));
        assert!(m.rooms.len() > 5, "labyrinth should have many rooms");
    }

    #[test]
    fn preset_single_room_is_small() {
        let mut m = Map::new(40, 30);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.from_preset(MapPreset::SingleRoom, &mut rng);
        assert!(m.is_walkable(px, py));
        assert_eq!(m.rooms.len(), 1);
    }

    #[test]
    fn preset_open_field_mostly_walkable() {
        let mut m = Map::new(40, 30);
        let mut rng = StdRng::seed_from_u64(42);
        let (px, py) = m.from_preset(MapPreset::OpenField, &mut rng);
        assert!(m.is_walkable(px, py));
        // Nearly all interior tiles should be floor.
        let expected_floor = (38) * (28);
        assert!(
            m.floor_count() >= expected_floor,
            "expected at least {} floor tiles, got {}",
            expected_floor,
            m.floor_count()
        );
    }

    #[test]
    fn preset_deterministic_with_same_seed() {
        let mut m1 = Map::new(40, 30);
        let mut rng1 = StdRng::seed_from_u64(99);
        let (p1x, p1y) = m1.from_preset(MapPreset::Arena, &mut rng1);

        let mut m2 = Map::new(40, 30);
        let mut rng2 = StdRng::seed_from_u64(99);
        let (p2x, p2y) = m2.from_preset(MapPreset::Arena, &mut rng2);

        assert_eq!(p1x, p2x);
        assert_eq!(p1y, p2y);
        assert_eq!(m1.tiles, m2.tiles);
    }

    #[test]
    fn preset_serialization_roundtrip() {
        let preset = MapPreset::Corridor;
        let json = serde_json::to_string(&preset).unwrap();
        let loaded: MapPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(preset, loaded);
    }
}
