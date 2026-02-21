// Dungeon map generation — rooms and corridors.
//
// Ports the Rust `map.rs` algorithm: random room placement with collision
// checks, L-shaped corridor carving between consecutive room centers.
//
// Key differences from the Rust version:
// - Fixed 40x22 map (no scrolling in v1)
// - Max 12 rooms (down from 30)
// - Room sizes 3-7 (down from 4-10)
// - All coordinates u8 instead of i32
// - Static arrays instead of Vec
// - Structural wall bitfield instead of Vec<bool>

use crate::prng;

pub const MAP_W: u8 = 40;
pub const MAP_H: u8 = 22;
pub const MAP_SIZE: usize = (MAP_W as usize) * (MAP_H as usize); // 880
pub const MAX_ROOMS: u8 = 12;
pub const ROOM_MIN: u8 = 3;
pub const ROOM_MAX: u8 = 7;

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
        Room { x: 0, y: 0, w: 0, h: 0 }
    }

    #[inline(always)]
    pub fn cx(&self) -> u8 { self.x + self.w / 2 }

    #[inline(always)]
    pub fn cy(&self) -> u8 { self.y + self.h / 2 }

    /// Check if two rooms overlap (with 1-tile padding for walls).
    pub fn intersects(&self, other: &Room) -> bool {
        self.x <= other.x + other.w
            && self.x + self.w >= other.x
            && self.y <= other.y + other.h
            && self.y + self.h >= other.y
    }
}

// --- Static map storage ---

static mut TILES: [u8; MAP_SIZE] = [TILE_WALL; MAP_SIZE];
static mut STRUCTURAL: [u8; (MAP_SIZE + 7) / 8] = [0; (MAP_SIZE + 7) / 8]; // bitfield
static mut ROOMS: [Room; MAX_ROOMS as usize] = [Room::empty(); MAX_ROOMS as usize];
static mut ROOM_COUNT: u8 = 0;

// --- Tile access ---

#[inline(always)]
fn idx(x: u8, y: u8) -> usize {
    (y as usize) * (MAP_W as usize) + (x as usize)
}

#[inline(always)]
pub fn in_bounds(x: u8, y: u8) -> bool {
    x < MAP_W && y < MAP_H
}

#[inline(always)]
pub fn tile_at(x: u8, y: u8) -> u8 {
    if !in_bounds(x, y) { return TILE_WALL; }
    unsafe { TILES[idx(x, y)] }
}

#[inline(always)]
pub fn is_walkable(x: u8, y: u8) -> bool {
    tile_at(x, y) == TILE_FLOOR
}

#[inline(always)]
pub fn is_structural(x: u8, y: u8) -> bool {
    if !in_bounds(x, y) { return false; }
    let i = idx(x, y);
    unsafe { STRUCTURAL[i / 8] & (1 << (i % 8)) != 0 }
}

pub fn room_count() -> u8 {
    unsafe { ROOM_COUNT }
}

pub fn room(i: u8) -> Room {
    unsafe { ROOMS[i as usize] }
}

/// Count total floor tiles (for exploration percentage).
pub fn floor_count() -> u16 {
    let mut count: u16 = 0;
    for i in 0..MAP_SIZE {
        if unsafe { TILES[i] } == TILE_FLOOR {
            count += 1;
        }
    }
    count
}

// --- Map generation ---

fn set_tile(x: u8, y: u8, tile: u8) {
    if in_bounds(x, y) {
        unsafe { TILES[idx(x, y)] = tile; }
    }
}

fn carve_room(room: &Room) {
    for y in (room.y + 1)..(room.y + room.h) {
        for x in (room.x + 1)..(room.x + room.w) {
            set_tile(x, y, TILE_FLOOR);
        }
    }
}

fn carve_h_tunnel(x1: u8, x2: u8, y: u8) {
    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    for x in min_x..=max_x {
        set_tile(x, y, TILE_FLOOR);
    }
}

fn carve_v_tunnel(y1: u8, y2: u8, x: u8) {
    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
    for y in min_y..=max_y {
        set_tile(x, y, TILE_FLOOR);
    }
}

fn compute_structural_walls() {
    // Reset bitfield
    unsafe {
        for b in STRUCTURAL.iter_mut() { *b = 0; }
    }

    // A wall is structural if any 8-neighbor is a floor tile
    for y in 0..MAP_H {
        for x in 0..MAP_W {
            if tile_at(x, y) != TILE_WALL { continue; }
            let mut found = false;
            for dy in -1i8..=1 {
                for dx in -1i8..=1 {
                    if dx == 0 && dy == 0 { continue; }
                    let nx = (x as i8 + dx) as u8;
                    let ny = (y as i8 + dy) as u8;
                    if in_bounds(nx, ny) && tile_at(nx, ny) == TILE_FLOOR {
                        found = true;
                        break;
                    }
                }
                if found { break; }
            }
            if found {
                let i = idx(x, y);
                unsafe { STRUCTURAL[i / 8] |= 1 << (i % 8); }
            }
        }
    }
}

/// Generate a dungeon. Returns (player_start_x, player_start_y).
pub fn generate() -> (u8, u8) {
    // Reset map to all walls
    unsafe {
        for t in TILES.iter_mut() { *t = TILE_WALL; }
        ROOM_COUNT = 0;
    }

    let mut start_x: u8 = MAP_W / 2;
    let mut start_y: u8 = MAP_H / 2;

    for _ in 0..MAX_ROOMS {
        let w = prng::range(ROOM_MIN, ROOM_MAX);
        let h = prng::range(ROOM_MIN, ROOM_MAX);
        // Ensure room fits within map with 1-tile border
        if w + 2 >= MAP_W || h + 2 >= MAP_H { continue; }
        let x = prng::range(1, MAP_W - w - 2);
        let y = prng::range(1, MAP_H - h - 2);

        let new_room = Room { x, y, w, h };

        // Check for overlap with existing rooms
        let mut overlaps = false;
        let rc = unsafe { ROOM_COUNT };
        for i in 0..rc {
            if new_room.intersects(&unsafe { ROOMS[i as usize] }) {
                overlaps = true;
                break;
            }
        }
        if overlaps { continue; }

        carve_room(&new_room);

        if rc == 0 {
            // First room: player starts here
            start_x = new_room.cx();
            start_y = new_room.cy();
        } else {
            // Connect to previous room with L-shaped corridor
            let prev = unsafe { ROOMS[(rc - 1) as usize] };
            let (px, py) = (prev.cx(), prev.cy());
            let (nx, ny) = (new_room.cx(), new_room.cy());
            if prng::coin() {
                carve_h_tunnel(px, nx, py);
                carve_v_tunnel(py, ny, nx);
            } else {
                carve_v_tunnel(py, ny, px);
                carve_h_tunnel(px, nx, ny);
            }
        }

        unsafe {
            ROOMS[rc as usize] = new_room;
            ROOM_COUNT = rc + 1;
        }
    }

    compute_structural_walls();
    (start_x, start_y)
}
