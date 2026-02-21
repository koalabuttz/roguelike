// Entity system — parallel arrays for player + monsters.
//
// The Rust version uses Vec<Entity> with String names and dynamic allocation.
// On C64 we use fixed-size parallel arrays indexed by entity slot (0-15).
// Slot 0 is always the player. Names are ROM string table lookups.
//
// Parallel arrays produce tighter 6502 code than array-of-structs:
// `LDA ent_x,X` is a single indexed load instruction.

use crate::map;
use crate::prng;

pub const MAX_ENTITIES: u8 = 16;
pub const PLAYER_IDX: u8 = 0;

// Entity type constants — index into stat/name tables
pub const ENT_NONE: u8 = 0;
pub const ENT_PLAYER: u8 = 1;
pub const ENT_GOBLIN: u8 = 2;
pub const ENT_ORC: u8 = 3;
pub const ENT_TROLL: u8 = 4;

// AI behavior (stored in ent_ai[])
pub const AI_NONE: u8 = 0;
pub const AI_CHASE: u8 = 1;
pub const AI_WANDER: u8 = 2;

// Screen codes for entity glyphs
const GLYPH_PLAYER: u8 = 0x00; // @ in screen codes
const GLYPH_GOBLIN: u8 = 0x07; // G
const GLYPH_ORC: u8 = 0x0F;    // O
const GLYPH_TROLL: u8 = 0x14;  // T
const GLYPH_CORPSE: u8 = 0x25; // %

// --- Entity parallel arrays ---

static mut ENT_X: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_Y: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_HP: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_MAX_HP: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_ATK: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_DEF: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_KIND: [u8; MAX_ENTITIES as usize] = [ENT_NONE; MAX_ENTITIES as usize];
static mut ENT_AI: [u8; MAX_ENTITIES as usize] = [AI_NONE; MAX_ENTITIES as usize];
static mut ENT_ALIVE: [bool; MAX_ENTITIES as usize] = [false; MAX_ENTITIES as usize];
static mut ENT_SIGHT: [u8; MAX_ENTITIES as usize] = [0; MAX_ENTITIES as usize];
static mut ENT_COUNT: u8 = 0;

// --- Stat tables (ROM data, indexed by ENT_* constants) ---

const STAT_HP: [u8; 5] = [0, 30, 6, 12, 20];    // max HP by type
const STAT_ATK: [u8; 5] = [0, 5, 3, 4, 6];       // attack by type
const STAT_DEF: [u8; 5] = [0, 2, 0, 1, 3];       // defense by type
const STAT_SIGHT: [u8; 5] = [0, 8, 6, 7, 5];     // sight radius by type
const STAT_GLYPH: [u8; 5] = [0, GLYPH_PLAYER, GLYPH_GOBLIN, GLYPH_ORC, GLYPH_TROLL];

// Monster colors (C64 color RAM values)
use crate::c64;
const STAT_COLOR: [u8; 5] = [
    c64::COLOR_BLACK,   // none
    c64::COLOR_YELLOW,  // player
    c64::COLOR_GREEN,   // goblin
    c64::COLOR_BROWN,   // orc (dark green → brown on C64)
    c64::COLOR_RED,     // troll
];

// Spawn weights (out of 100 total) — Goblin 60%, Orc 30%, Troll 10%
const SPAWN_WEIGHT: [u8; 3] = [60, 30, 10];
const SPAWN_KIND: [u8; 3] = [ENT_GOBLIN, ENT_ORC, ENT_TROLL];
const SPAWN_TOTAL: u8 = 100;

// Monster names as ASCII byte strings
const NAME_NONE: &[u8] = b"???";
const NAME_PLAYER: &[u8] = b"You";
const NAME_GOBLIN: &[u8] = b"Goblin";
const NAME_ORC: &[u8] = b"Orc";
const NAME_TROLL: &[u8] = b"Troll";

static NAMES: [&[u8]; 5] = [NAME_NONE, NAME_PLAYER, NAME_GOBLIN, NAME_ORC, NAME_TROLL];

// --- Accessors ---

#[inline(always)]
pub fn x(i: u8) -> u8 { unsafe { ENT_X[i as usize] } }
#[inline(always)]
pub fn y(i: u8) -> u8 { unsafe { ENT_Y[i as usize] } }
#[inline(always)]
pub fn hp(i: u8) -> u8 { unsafe { ENT_HP[i as usize] } }
#[inline(always)]
pub fn max_hp(i: u8) -> u8 { unsafe { ENT_MAX_HP[i as usize] } }
#[inline(always)]
pub fn atk(i: u8) -> u8 { unsafe { ENT_ATK[i as usize] } }
#[inline(always)]
pub fn def(i: u8) -> u8 { unsafe { ENT_DEF[i as usize] } }
#[inline(always)]
pub fn kind(i: u8) -> u8 { unsafe { ENT_KIND[i as usize] } }
#[inline(always)]
pub fn ai(i: u8) -> u8 { unsafe { ENT_AI[i as usize] } }
#[inline(always)]
pub fn is_alive(i: u8) -> bool { unsafe { ENT_ALIVE[i as usize] } }
#[inline(always)]
pub fn sight(i: u8) -> u8 { unsafe { ENT_SIGHT[i as usize] } }
#[inline(always)]
pub fn count() -> u8 { unsafe { ENT_COUNT } }
pub fn name(i: u8) -> &'static [u8] { NAMES[kind(i) as usize] }

pub fn glyph(i: u8) -> u8 { STAT_GLYPH[kind(i) as usize] }
pub fn color(i: u8) -> u8 { STAT_COLOR[kind(i) as usize] }

#[inline(always)]
pub fn set_pos(i: u8, new_x: u8, new_y: u8) {
    unsafe {
        ENT_X[i as usize] = new_x;
        ENT_Y[i as usize] = new_y;
    }
}

#[inline(always)]
pub fn set_hp(i: u8, val: u8) {
    unsafe { ENT_HP[i as usize] = val; }
}

#[inline(always)]
pub fn set_ai(i: u8, behavior: u8) {
    unsafe { ENT_AI[i as usize] = behavior; }
}

pub fn kill(i: u8) {
    unsafe { ENT_ALIVE[i as usize] = false; }
}

// --- Spawning ---

/// Reset all entities (call before map generation).
pub fn reset() {
    unsafe {
        ENT_COUNT = 0;
        for i in 0..MAX_ENTITIES as usize {
            ENT_ALIVE[i] = false;
            ENT_KIND[i] = ENT_NONE;
        }
    }
}

/// Spawn the player at position. Always slot 0.
pub fn spawn_player(px: u8, py: u8) {
    let i = PLAYER_IDX as usize;
    unsafe {
        ENT_X[i] = px;
        ENT_Y[i] = py;
        ENT_KIND[i] = ENT_PLAYER;
        ENT_HP[i] = STAT_HP[ENT_PLAYER as usize];
        ENT_MAX_HP[i] = STAT_HP[ENT_PLAYER as usize];
        ENT_ATK[i] = STAT_ATK[ENT_PLAYER as usize];
        ENT_DEF[i] = STAT_DEF[ENT_PLAYER as usize];
        ENT_AI[i] = AI_NONE;
        ENT_ALIVE[i] = true;
        ENT_SIGHT[i] = STAT_SIGHT[ENT_PLAYER as usize];
        if ENT_COUNT == 0 { ENT_COUNT = 1; }
    }
}

fn spawn_entity(kind: u8, ex: u8, ey: u8, behavior: u8) -> bool {
    let slot = unsafe { ENT_COUNT };
    if slot >= MAX_ENTITIES { return false; }
    let i = slot as usize;
    unsafe {
        ENT_X[i] = ex;
        ENT_Y[i] = ey;
        ENT_KIND[i] = kind;
        ENT_HP[i] = STAT_HP[kind as usize];
        ENT_MAX_HP[i] = STAT_HP[kind as usize];
        ENT_ATK[i] = STAT_ATK[kind as usize];
        ENT_DEF[i] = STAT_DEF[kind as usize];
        ENT_AI[i] = behavior;
        ENT_ALIVE[i] = true;
        ENT_SIGHT[i] = STAT_SIGHT[kind as usize];
        ENT_COUNT = slot + 1;
    }
    true
}

/// Pick a random monster type using weighted spawn table.
fn pick_monster_kind() -> u8 {
    let mut roll = prng::range(0, SPAWN_TOTAL - 1);
    for i in 0..SPAWN_WEIGHT.len() {
        if roll < SPAWN_WEIGHT[i] {
            return SPAWN_KIND[i];
        }
        roll -= SPAWN_WEIGHT[i];
    }
    ENT_GOBLIN // fallback
}

/// Spawn monsters in rooms (skip room 0 = player start). Max 2 per room.
pub fn spawn_monsters() {
    let rc = map::room_count();
    for ri in 1..rc {
        let room = map::room(ri);
        let count = prng::range(0, 2); // 0, 1, or 2 monsters per room
        for _ in 0..count {
            let mx = prng::range(room.x + 1, room.x + room.w - 1);
            let my = prng::range(room.y + 1, room.y + room.h - 1);
            // Don't spawn on top of another entity
            if entity_at(mx, my) != NO_ENTITY { continue; }
            let kind = pick_monster_kind();
            spawn_entity(kind, mx, my, AI_CHASE);
        }
    }
}

// --- Queries ---

pub const NO_ENTITY: u8 = 0xFF;

/// Find any alive entity at position. Returns slot index or NO_ENTITY.
pub fn entity_at(ex: u8, ey: u8) -> u8 {
    let n = unsafe { ENT_COUNT };
    for i in 0..n {
        if is_alive(i) && x(i) == ex && y(i) == ey {
            return i;
        }
    }
    NO_ENTITY
}

/// Find alive monster (non-player) at position.
pub fn monster_at(ex: u8, ey: u8) -> u8 {
    let n = unsafe { ENT_COUNT };
    for i in 1..n { // skip player
        if is_alive(i) && x(i) == ex && y(i) == ey {
            return i;
        }
    }
    NO_ENTITY
}

/// Check if position is occupied by any alive monster (excluding skip_idx).
pub fn is_occupied(ex: u8, ey: u8, skip_idx: u8) -> bool {
    let n = unsafe { ENT_COUNT };
    for i in 1..n { // skip player
        if i == skip_idx { continue; }
        if is_alive(i) && x(i) == ex && y(i) == ey {
            return true;
        }
    }
    false
}
