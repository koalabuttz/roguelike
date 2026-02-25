//! Dungeon map generation for the micro tier.
//!
//! Random room placement with collision checks and L-shaped corridor carving.
//! Ported from the C64 POC with struct-based storage instead of static muts.

use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::balance;

pub const TILE_WALL: u8 = 0;
pub const TILE_FLOOR: u8 = 1;

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
    pub tiles: [u8; MAP_SIZE],
    structural: [u8; BITFIELD_SIZE],
    pub rooms: [Room; MAX_ROOMS],
    pub room_count: u8,
}

fn idx(x: u8, y: u8) -> usize {
    (y as usize) * (MAP_WIDTH as usize) + (x as usize)
}

impl Default for MicroMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MicroMap {
    pub fn new() -> Self {
        Self {
            tiles: [TILE_WALL; MAP_SIZE],
            structural: [0; BITFIELD_SIZE],
            rooms: [Room::empty(); MAX_ROOMS],
            room_count: 0,
        }
    }

    pub fn in_bounds(x: u8, y: u8) -> bool {
        x < MAP_WIDTH && y < MAP_HEIGHT
    }

    pub fn tile_at(&self, x: u8, y: u8) -> u8 {
        if !Self::in_bounds(x, y) {
            return TILE_WALL;
        }
        self.tiles[idx(x, y)]
    }

    pub fn is_walkable(&self, x: u8, y: u8) -> bool {
        self.tile_at(x, y) == TILE_FLOOR
    }

    pub fn is_structural(&self, x: u8, y: u8) -> bool {
        if !Self::in_bounds(x, y) {
            return false;
        }
        let i = idx(x, y);
        self.structural[i / 8] & (1 << (i % 8)) != 0
    }

    pub fn floor_count(&self) -> u16 {
        let mut count: u16 = 0;
        for &t in &self.tiles {
            if t == TILE_FLOOR {
                count += 1;
            }
        }
        count
    }

    fn set_tile(&mut self, x: u8, y: u8, tile: u8) {
        if Self::in_bounds(x, y) {
            self.tiles[idx(x, y)] = tile;
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
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
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
                        if Self::in_bounds(nx, ny) && self.tile_at(nx, ny) == TILE_FLOOR {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if found {
                    let i = idx(x, y);
                    self.structural[i / 8] |= 1 << (i % 8);
                }
            }
        }
    }

    /// Generate a dungeon. Returns the player start position.
    pub fn generate(&mut self, rng: &mut LfsrRng16) -> Pos {
        // Reset to all walls
        for t in self.tiles.iter_mut() {
            *t = TILE_WALL;
        }
        self.room_count = 0;

        let room_min = balance::MICRO_ROOM_SIZE_MIN;
        let room_max = balance::MICRO_ROOM_SIZE_MAX;
        let max_rooms = MAX_ROOMS as u8;

        let mut start_x = MAP_WIDTH / 2;
        let mut start_y = MAP_HEIGHT / 2;

        for _ in 0..max_rooms {
            let w = rng.range_u8(room_min, room_max);
            let h = rng.range_u8(room_min, room_max);
            // Ensure room fits within map with 1-tile border
            if w + 2 >= MAP_WIDTH || h + 2 >= MAP_HEIGHT {
                continue;
            }
            let x = rng.range_u8(1, MAP_WIDTH - w - 2);
            let y = rng.range_u8(1, MAP_HEIGHT - h - 2);

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

        self.compute_structural_walls();
        (start_x, start_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_map(seed: u16) -> (MicroMap, Pos) {
        let mut rng = LfsrRng16::new(seed);
        let mut map = MicroMap::new();
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
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
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
                            if MicroMap::in_bounds(nx, ny) && map.tile_at(nx, ny) == TILE_FLOOR {
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
        let manual = map.tiles.iter().filter(|&&t| t == TILE_FLOOR).count() as u16;
        assert_eq!(map.floor_count(), manual);
    }

    #[test]
    fn out_of_bounds_is_wall() {
        let (map, _) = make_map(42);
        assert_eq!(map.tile_at(255, 255), TILE_WALL);
        assert!(!map.is_walkable(MAP_WIDTH, 0));
    }

    #[test]
    fn deterministic_with_same_seed() {
        let (a, sa) = make_map(1234);
        let (b, sb) = make_map(1234);
        assert_eq!(sa, sb);
        assert_eq!(a.room_count, b.room_count);
        assert_eq!(a.tiles, b.tiles);
    }
}
