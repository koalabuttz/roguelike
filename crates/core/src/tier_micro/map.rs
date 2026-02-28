//! Dungeon map generation for the micro tier.
//!
//! Random room placement with collision checks and L-shaped corridor carving.
//! Ported from the C64 POC with struct-based storage instead of static muts.
//!
//! Arrays are sized for the maximum supported dimensions (80×60). Actual
//! map dimensions are stored as runtime fields `width` and `height`.

use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::balance;

pub const TILE_WALL: u8 = 0;
pub const TILE_FLOOR: u8 = 1;
pub const TILE_STAIRS_DOWN: u8 = 2;

#[derive(Copy, Clone)]
pub struct Room {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
}

impl Room {
    pub const fn empty() -> Self {
        Room {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        }
    }

    pub fn cx(&self) -> u8 {
        self.x + self.w / 2
    }

    pub fn cy(&self) -> u8 {
        self.y + self.h / 2
    }

    /// Check if two rooms overlap (with 1-tile padding for walls).
    pub fn intersects(&self, other: &Room) -> bool {
        self.x <= other.x + other.w
            && self.x + self.w >= other.x
            && self.y <= other.y + other.h
            && self.y + self.h >= other.y
    }
}

pub struct MicroMap {
    pub tiles: [u8; MAX_MAP_SIZE],
    structural: [u8; MAX_BITFIELD_SIZE],
    pub rooms: [Room; MAX_ROOMS],
    pub room_count: u8,
    pub width: u8,
    pub height: u8,
}

impl MicroMap {
    pub fn new(width: u8, height: u8) -> Self {
        debug_assert!(width <= MAX_MAP_WIDTH);
        debug_assert!(height <= MAX_MAP_HEIGHT);
        Self {
            tiles: [TILE_WALL; MAX_MAP_SIZE],
            structural: [0; MAX_BITFIELD_SIZE],
            rooms: [Room::empty(); MAX_ROOMS],
            room_count: 0,
            width,
            height,
        }
    }

    /// Create a map with C64 default dimensions (64×48).
    pub fn new_default() -> Self {
        Self::new(DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT)
    }

    pub(crate) fn idx(&self, x: u8, y: u8) -> usize {
        (y as usize) * (self.width as usize) + (x as usize)
    }

    pub fn in_bounds(&self, x: u8, y: u8) -> bool {
        x < self.width && y < self.height
    }

    pub fn tile_at(&self, x: u8, y: u8) -> u8 {
        if !self.in_bounds(x, y) {
            return TILE_WALL;
        }
        self.tiles[self.idx(x, y)]
    }

    pub fn is_walkable(&self, x: u8, y: u8) -> bool {
        self.tile_at(x, y) != TILE_WALL
    }

    pub fn is_structural(&self, x: u8, y: u8) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        let i = self.idx(x, y);
        self.structural[i / 8] & (1 << (i % 8)) != 0
    }

    /// Count walkable 8-neighbors excluding the direction (exclude_dx, exclude_dy).
    ///
    /// Used for corridor branch detection during autorun.
    pub fn open_neighbors_excluding(&self, x: u8, y: u8, exclude_dx: i8, exclude_dy: i8) -> u8 {
        let mut count: u8 = 0;
        for ny in -1i8..=1 {
            for nx in -1i8..=1 {
                if nx == 0 && ny == 0 {
                    continue;
                }
                if nx == exclude_dx && ny == exclude_dy {
                    continue;
                }
                let tx = (x as i8 + nx) as u8;
                let ty = (y as i8 + ny) as u8;
                if self.is_walkable(tx, ty) {
                    count += 1;
                }
            }
        }
        count
    }

    pub fn floor_count(&self) -> u16 {
        let mut count: u16 = 0;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.tiles[self.idx(x, y)] != TILE_WALL {
                    count += 1;
                }
            }
        }
        count
    }

    /// Place stairs down at the center of the last room.
    pub fn place_stairs_down(&mut self) {
        if self.room_count > 0 {
            let last = self.rooms[(self.room_count - 1) as usize];
            let i = self.idx(last.cx(), last.cy());
            self.tiles[i] = TILE_STAIRS_DOWN;
        }
    }

    fn set_tile(&mut self, x: u8, y: u8, tile: u8) {
        if self.in_bounds(x, y) {
            let i = self.idx(x, y);
            self.tiles[i] = tile;
        }
    }

    fn carve_room(&mut self, room: &Room) {
        for y in (room.y + 1)..(room.y + room.h) {
            for x in (room.x + 1)..(room.x + room.w) {
                self.set_tile(x, y, TILE_FLOOR);
            }
        }
    }

    fn carve_h_tunnel(&mut self, x1: u8, x2: u8, y: u8) {
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        for x in min_x..=max_x {
            self.set_tile(x, y, TILE_FLOOR);
        }
    }

    fn carve_v_tunnel(&mut self, y1: u8, y2: u8, x: u8) {
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        for y in min_y..=max_y {
            self.set_tile(x, y, TILE_FLOOR);
        }
    }

    fn compute_structural_walls(&mut self) {
        for b in self.structural.iter_mut() {
            *b = 0;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                if self.tile_at(x, y) != TILE_WALL {
                    continue;
                }
                let mut found = false;
                for dy in -1i8..=1 {
                    for dx in -1i8..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = (x as i8 + dx) as u8;
                        let ny = (y as i8 + dy) as u8;
                        if self.in_bounds(nx, ny) && self.tile_at(nx, ny) != TILE_WALL {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if found {
                    let i = self.idx(x, y);
                    self.structural[i / 8] |= 1 << (i % 8);
                }
            }
        }
    }

    /// Generate a dungeon. Returns the player start position.
    pub fn generate(&mut self, rng: &mut LfsrRng16) -> Pos {
        // Reset to all walls
        let map_size = (self.width as usize) * (self.height as usize);
        for t in self.tiles[..map_size].iter_mut() {
            *t = TILE_WALL;
        }
        self.room_count = 0;

        let room_min = balance::MICRO_ROOM_SIZE_MIN;
        let room_max = balance::MICRO_ROOM_SIZE_MAX;
        // Scale room attempts with map area, capped at MAX_ROOMS.
        let area = (self.width as u16) * (self.height as u16);
        let max_rooms = ((area / 256) as u8).max(6).min(MAX_ROOMS as u8);

        let mut start_x = self.width / 2;
        let mut start_y = self.height / 2;

        for _ in 0..max_rooms {
            let w = rng.range_u8(room_min, room_max);
            let h = rng.range_u8(room_min, room_max);
            // Ensure room fits within map with 1-tile border
            if w + 2 >= self.width || h + 2 >= self.height {
                continue;
            }
            let x = rng.range_u8(1, self.width - w - 2);
            let y = rng.range_u8(1, self.height - h - 2);

            let new_room = Room { x, y, w, h };

            // Check for overlap with existing rooms
            let mut overlaps = false;
            for i in 0..self.room_count {
                if new_room.intersects(&self.rooms[i as usize]) {
                    overlaps = true;
                    break;
                }
            }
            if overlaps {
                continue;
            }

            self.carve_room(&new_room);

            if self.room_count == 0 {
                start_x = new_room.cx();
                start_y = new_room.cy();
            } else {
                let prev = self.rooms[(self.room_count - 1) as usize];
                let (px, py) = (prev.cx(), prev.cy());
                let (nx, ny) = (new_room.cx(), new_room.cy());
                if rng.coin() {
                    self.carve_h_tunnel(px, nx, py);
                    self.carve_v_tunnel(py, ny, nx);
                } else {
                    self.carve_v_tunnel(py, ny, px);
                    self.carve_h_tunnel(px, nx, ny);
                }
            }

            self.rooms[self.room_count as usize] = new_room;
            self.room_count += 1;
        }

        self.place_stairs_down();
        self.compute_structural_walls();
        (start_x, start_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_map(seed: u16) -> (MicroMap, Pos) {
        let mut rng = LfsrRng16::new(seed);
        let mut map = MicroMap::new_default();
        let start = map.generate(&mut rng);
        (map, start)
    }

    #[test]
    fn generate_produces_rooms() {
        let (map, _) = make_map(42);
        assert!(map.room_count > 0);
    }

    #[test]
    fn start_pos_is_walkable() {
        let (map, (sx, sy)) = make_map(42);
        assert!(map.is_walkable(sx, sy));
    }

    #[test]
    fn rooms_dont_overlap() {
        let (map, _) = make_map(42);
        for i in 0..map.room_count as usize {
            for j in (i + 1)..map.room_count as usize {
                assert!(
                    !map.rooms[i].intersects(&map.rooms[j]),
                    "rooms {i} and {j} overlap"
                );
            }
        }
    }

    #[test]
    fn structural_walls_adjacent_to_floor() {
        let (map, _) = make_map(42);
        for y in 0..map.height {
            for x in 0..map.width {
                if map.is_structural(x, y) {
                    assert_eq!(map.tile_at(x, y), TILE_WALL);
                    let mut has_floor_neighbor = false;
                    for dy in -1i8..=1 {
                        for dx in -1i8..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = (x as i8 + dx) as u8;
                            let ny = (y as i8 + dy) as u8;
                            if map.in_bounds(nx, ny) && map.tile_at(nx, ny) == TILE_FLOOR {
                                has_floor_neighbor = true;
                            }
                        }
                    }
                    assert!(
                        has_floor_neighbor,
                        "structural wall ({x},{y}) has no floor neighbor"
                    );
                }
            }
        }
    }

    #[test]
    fn floor_count_matches_tiles() {
        let (map, _) = make_map(42);
        let mut manual: u16 = 0;
        for y in 0..map.height {
            for x in 0..map.width {
                if map.tiles[map.idx(x, y)] != TILE_WALL {
                    manual += 1;
                }
            }
        }
        assert_eq!(map.floor_count(), manual);
    }

    #[test]
    fn out_of_bounds_is_wall() {
        let (map, _) = make_map(42);
        assert_eq!(map.tile_at(255, 255), TILE_WALL);
        assert!(!map.is_walkable(map.width, 0));
    }

    #[test]
    fn deterministic_with_same_seed() {
        let (a, sa) = make_map(1234);
        let (b, sb) = make_map(1234);
        assert_eq!(sa, sb);
        assert_eq!(a.room_count, b.room_count);
        let a_size = (a.width as usize) * (a.height as usize);
        assert_eq!(a.tiles[..a_size], b.tiles[..a_size]);
    }

    #[test]
    fn open_neighbors_excluding_in_corridor() {
        let mut map = MicroMap::new_default();
        // Horizontal corridor at y=10: floor at x=5,6,7
        map.tiles[map.idx(5, 10)] = TILE_FLOOR;
        map.tiles[map.idx(6, 10)] = TILE_FLOOR;
        map.tiles[map.idx(7, 10)] = TILE_FLOOR;
        // At (6,10) heading east, excluding behind (-1,0): only (7,10) ahead.
        assert_eq!(map.open_neighbors_excluding(6, 10, -1, 0), 1);
    }

    #[test]
    fn open_neighbors_excluding_at_junction() {
        let mut map = MicroMap::new_default();
        // T-junction: corridor east-west at y=10, plus branch south.
        for x in 5..=8 {
            map.tiles[map.idx(x, 10)] = TILE_FLOOR;
        }
        map.tiles[map.idx(6, 11)] = TILE_FLOOR;
        // At (6,10) heading east, excluding behind (-1,0):
        // forward (7,10) + branch (6,11) = 2+
        assert!(map.open_neighbors_excluding(6, 10, -1, 0) >= 2);
    }

    #[test]
    fn custom_dimensions() {
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new(80, 40);
        let (sx, sy) = map.generate(&mut rng);
        assert!(map.is_walkable(sx, sy));
        assert_eq!(map.width, 80);
        assert_eq!(map.height, 40);
        assert!(map.room_count > 0);
    }

    #[test]
    fn stairs_placed_in_last_room() {
        let (map, _) = make_map(42);
        assert!(map.room_count >= 2, "need at least 2 rooms for stairs");
        let last = map.rooms[(map.room_count - 1) as usize];
        assert_eq!(
            map.tile_at(last.cx(), last.cy()),
            TILE_STAIRS_DOWN,
            "stairs should be at last room center"
        );
    }

    #[test]
    fn stairs_placed_with_single_room() {
        // Tiny map that can only fit 1 room — stairs should still be placed
        let mut rng = LfsrRng16::new(42);
        let mut map = MicroMap::new(12, 12);
        map.generate(&mut rng);
        assert!(map.room_count > 0);
        let last = map.rooms[(map.room_count - 1) as usize];
        assert_eq!(
            map.tile_at(last.cx(), last.cy()),
            TILE_STAIRS_DOWN,
            "stairs should be placed even with a single room"
        );
    }

    #[test]
    fn stairs_are_walkable() {
        let (map, _) = make_map(42);
        let last = map.rooms[(map.room_count - 1) as usize];
        assert!(
            map.is_walkable(last.cx(), last.cy()),
            "stairs tile should be walkable"
        );
    }

    #[test]
    fn floor_count_includes_stairs() {
        let (map, _) = make_map(42);
        // Manual count of all non-wall tiles
        let mut manual: u16 = 0;
        for y in 0..map.height {
            for x in 0..map.width {
                if map.tiles[map.idx(x, y)] != TILE_WALL {
                    manual += 1;
                }
            }
        }
        assert_eq!(map.floor_count(), manual);
    }
}
