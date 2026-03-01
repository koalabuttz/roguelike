// Screen rendering — VIC-II character mode with viewport scrolling.
//
// Reads game state from &MicroGameState (roguelike-core::tier_micro).
// The 64x48 tile map is viewed through a 40x22 player-centered viewport.
//
// Layout:
//   Rows 0-21:  Map viewport (40x22 tiles from the 64x48 map)
//   Row 22:     Status bar (HP bar, kills, turns)
//   Rows 23-24: Message log (2 most recent GameEvents, formatted to PETSCII)

use core::ptr::write_volatile;

use crate::c64;
use roguelike_core::rules::balance;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::items;
use roguelike_core::rules::message::{GameEvent, SoundDistance};
use roguelike_core::rules::monster_table;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::item_store::MAX_ITEMS;
use roguelike_core::tier_micro::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL, TILE_WALL};
use roguelike_core::tier_micro::types::{MAX_BITFIELD_SIZE, MAX_ENTITIES, NO_ENTITY, PLAYER_IDX};

// Screen codes for map tiles
const SC_SPACE: u8 = 0x20;
const SC_FLOOR: u8 = 0x2E;  // .
const SC_WALL: u8 = 0xA0;   // reverse space = solid block
const SC_STAIRS: u8 = 0x3E; // >

const STATUS_ROW: u8 = 22;
const MSG_ROW: u8 = 23;
const VIEW_W: u8 = 40;
const VIEW_H: u8 = 22;

/// Compute viewport origin so the player is centered on screen.
/// Clamps to map edges so we never show out-of-bounds tiles.
pub fn viewport_pos(state: &MicroGameState) -> (u8, u8) {
    let px = state.entities.x[PLAYER_IDX as usize];
    let py = state.entities.y[PLAYER_IDX as usize];
    let vx = px.saturating_sub(VIEW_W / 2).min(state.map.width.saturating_sub(VIEW_W));
    let vy = py.saturating_sub(VIEW_H / 2).min(state.map.height.saturating_sub(VIEW_H));
    (vx, vy)
}

/// Map a platform-independent GameColor to a C64 4-bit color value.
fn game_color_to_c64(gc: GameColor) -> u8 {
    match gc {
        GameColor::Black => c64::COLOR_BLACK,
        GameColor::White => c64::COLOR_WHITE,
        GameColor::Grey => c64::COLOR_GREY,
        GameColor::DarkGrey => c64::COLOR_DGREY,
        GameColor::Red => c64::COLOR_RED,
        GameColor::DarkRed => c64::COLOR_BROWN,
        GameColor::Green => c64::COLOR_GREEN,
        GameColor::DarkGreen => c64::COLOR_BROWN,
        GameColor::Yellow => c64::COLOR_YELLOW,
        GameColor::DarkBlue => c64::COLOR_BLUE,
        GameColor::Cyan => c64::COLOR_CYAN,
    }
}

/// Compute the screen code and color for a map tile based on visibility.
fn tile_appearance(state: &MicroGameState, wx: u8, wy: u8) -> (u8, u8) {
    let visible = state.fov.is_visible(wx, wy);
    let explored = state.fov.is_explored(wx, wy);

    if visible {
        let tile = state.map.tile_at(wx, wy);
        match tile {
            TILE_FLOOR => (SC_FLOOR, c64::COLOR_DGREY),
            TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_CYAN),
            TILE_STRUCTURAL => (SC_WALL, c64::COLOR_LGREY),
            TILE_WALL => (SC_SPACE, c64::COLOR_BLACK),
            _ => (SC_SPACE, c64::COLOR_BLACK),
        }
    } else if explored {
        let tile = state.map.tile_at(wx, wy);
        match tile {
            TILE_FLOOR => (SC_FLOOR, c64::COLOR_BLUE),
            TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_BLUE),
            TILE_STRUCTURAL => (SC_WALL, c64::COLOR_BLUE),
            TILE_WALL => (SC_SPACE, c64::COLOR_BLACK),
            _ => (SC_SPACE, c64::COLOR_BLACK),
        }
    } else {
        (SC_SPACE, c64::COLOR_BLACK)
    }
}

/// Full screen render: map + items + entities + status + messages.
pub fn render_all(state: &MicroGameState) {
    let (vx, vy) = viewport_pos(state);
    render_map(state, vx, vy);
    render_items(state, vx, vy);
    render_entities(state, vx, vy);
    render_status_bar(state);
    render_messages(state);
}

/// Extract a 4-bit packed tile from the tile array using a linear index.
#[inline(always)]
fn tile_at_index(tiles: &[u8], fi: usize) -> u8 {
    let byte = tiles[fi >> 1];
    if fi & 1 == 0 {
        byte & 0x0F
    } else {
        byte >> 4
    }
}

/// Render the dungeon map tiles within the current viewport.
///
/// Uses row-major index pre-computation to avoid per-cell multiplies.
/// Two counters advance through the loop: `fi` (FOV/map linear index,
/// stride = map_width) and `si` (screen memory offset, stride = 40).
/// Both share the same per-column increment of 1. This eliminates ~4
/// multiplies per cell vs the naive `tile_appearance` + `draw_char` path.
fn render_map(state: &MicroGameState, vx: u8, vy: u8) {
    let vis = state.fov.visible_bytes();
    let exp = state.fov.explored_bytes();
    let tiles = &state.map.tiles;
    let map_w = state.fov.width as usize;

    let mut fov_row = (vy as usize) * map_w + (vx as usize);
    let mut scr_row: usize = 0;

    for _sy in 0..VIEW_H as usize {
        let mut fi = fov_row;
        let mut si = scr_row;

        for _sx in 0..VIEW_W as usize {
            let byte_idx = fi >> 3;
            let bit = 1u8 << (fi & 7);

            let (sc, color) = if vis[byte_idx] & bit != 0 {
                match tile_at_index(tiles, fi) {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_DGREY),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_CYAN),
                    TILE_STRUCTURAL => (SC_WALL, c64::COLOR_LGREY),
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else if exp[byte_idx] & bit != 0 {
                match tile_at_index(tiles, fi) {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_BLUE),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_BLUE),
                    TILE_STRUCTURAL => (SC_WALL, c64::COLOR_BLUE),
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else {
                (SC_SPACE, c64::COLOR_BLACK)
            };

            unsafe {
                write_volatile(c64::SCREEN.add(si), sc);
                write_volatile(c64::COLOR_RAM.add(si), color);
            }

            fi += 1;
            si += 1;
        }

        fov_row += map_w;
        scr_row += 40;
    }
}

/// Sparse re-render for viewport scrolls: skip cells where both the old and
/// new world tiles are unexplored (both render as black, so the write is
/// redundant). Uses the same row-major dual-counter pattern as `render_map()`
/// with an additional `old_fi` index tracking the previous viewport position.
///
/// The explored bitfield is monotonically increasing (tiles never become
/// unexplored), so checking the current explored state is sufficient to know
/// that both old and new tiles are unexplored.
fn render_map_sparse(
    state: &MicroGameState,
    vx: u8,
    vy: u8,
    old_vx: u8,
    old_vy: u8,
) {
    let vis = state.fov.visible_bytes();
    let exp = state.fov.explored_bytes();
    let tiles = &state.map.tiles;
    let map_w = state.fov.width as usize;

    let mut fov_row = (vy as usize) * map_w + (vx as usize);
    let mut old_row = (old_vy as usize) * map_w + (old_vx as usize);
    let mut scr_row: usize = 0;
    let row_skip = map_w - (VIEW_W as usize);

    for _sy in 0..VIEW_H as usize {
        let mut fi = fov_row;
        let mut old_fi = old_row;
        let mut si = scr_row;

        for _sx in 0..VIEW_W as usize {
            // If both old and new world tiles are unexplored, the screen cell
            // was black and stays black — skip the write.
            let new_explored = exp[fi >> 3] & (1u8 << (fi & 7)) != 0;
            let old_explored = exp[old_fi >> 3] & (1u8 << (old_fi & 7)) != 0;

            if !new_explored && !old_explored {
                fi += 1;
                old_fi += 1;
                si += 1;
                continue;
            }

            let byte_idx = fi >> 3;
            let bit = 1u8 << (fi & 7);

            let (sc, color) = if vis[byte_idx] & bit != 0 {
                match tile_at_index(tiles, fi) {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_DGREY),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_CYAN),
                    TILE_STRUCTURAL => (SC_WALL, c64::COLOR_LGREY),
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else if new_explored {
                match tile_at_index(tiles, fi) {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_BLUE),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_BLUE),
                    TILE_STRUCTURAL => (SC_WALL, c64::COLOR_BLUE),
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else {
                (SC_SPACE, c64::COLOR_BLACK)
            };

            unsafe {
                write_volatile(c64::SCREEN.add(si), sc);
                write_volatile(c64::COLOR_RAM.add(si), color);
            }

            fi += 1;
            old_fi += 1;
            si += 1;
        }

        fov_row += map_w;
        old_row += map_w;
        scr_row += 40;
    }

    // Suppress unused-variable warning — row_skip is zero when VIEW_W == 40
    // but kept for clarity; the compiler optimizes it away.
    let _ = row_skip;
}

/// Render ground items that are visible and within the viewport.
fn render_items(state: &MicroGameState, vx: u8, vy: u8) {
    for i in 0..state.items.count as usize {
        if !state.items.alive[i] {
            continue;
        }
        let ix = state.items.x[i];
        let iy = state.items.y[i];
        if !state.fov.is_visible(ix, iy) {
            continue;
        }
        if ix < vx || ix >= vx + VIEW_W || iy < vy || iy >= vy + VIEW_H {
            continue;
        }
        let sx = ix - vx;
        let sy = iy - vy;
        let kind = state.items.kind[i];
        let glyph = items::glyph(kind) as u8;
        let color = game_color_to_c64(items::color(kind));
        c64::draw_char(sx, sy, c64::to_screen_code(glyph), color);
    }
}

/// Render all alive entities that are visible and within the viewport.
fn render_entities(state: &MicroGameState, vx: u8, vy: u8) {
    for i in 0..state.entities.count {
        let idx = i as usize;
        if !state.entities.alive[idx] {
            continue;
        }
        let ex = state.entities.x[idx];
        let ey = state.entities.y[idx];
        if !state.fov.is_visible(ex, ey) {
            continue;
        }
        // Check if entity is within viewport
        if ex < vx || ex >= vx + VIEW_W || ey < vy || ey >= vy + VIEW_H {
            continue;
        }

        let sx = ex - vx;
        let sy = ey - vy;

        let (glyph, color) = if i == PLAYER_IDX {
            (balance::PLAYER_GLYPH as u8, c64::COLOR_YELLOW)
        } else {
            match state.entities.kind[idx] {
                Some(kind) => {
                    let g = monster_table::glyph(kind) as u8;
                    let c = game_color_to_c64(monster_table::color(kind));
                    (g, c)
                }
                None => (b'?', c64::COLOR_WHITE),
            }
        };

        c64::draw_char(sx, sy, c64::to_screen_code(glyph), color);
    }
}

/// Render the status bar on row 22.
fn render_status_bar(state: &MicroGameState) {
    c64::fill_row(STATUS_ROW, SC_SPACE, c64::COLOR_BLACK);

    let hp = state.entities.hp[PLAYER_IDX as usize];
    let max_hp = state.entities.max_hp[PLAYER_IDX as usize];

    // "HP " label
    c64::draw_text(0, STATUS_ROW, b"HP ", c64::COLOR_WHITE);

    // HP bar: 12 chars wide, filled proportionally
    let bar_width: u8 = 12;
    let filled = if max_hp > 0 {
        ((hp as u16) * (bar_width as u16) / (max_hp as u16)) as u8
    } else {
        0
    };

    let bar_color = if max_hp == 0 || hp * 100 / max_hp > 60 {
        c64::COLOR_GREEN
    } else if hp * 100 / max_hp > 30 {
        c64::COLOR_YELLOW
    } else {
        c64::COLOR_RED
    };

    for i in 0..bar_width {
        if i < filled {
            c64::draw_char(3 + i, STATUS_ROW, 0xA0, bar_color); // filled block
        } else {
            c64::draw_char(3 + i, STATUS_ROW, 0x65, c64::COLOR_DGREY); // light shade
        }
    }

    // "HP/MaxHP" after bar
    let mut col = 3 + bar_width + 1;
    col += c64::draw_number(col, STATUS_ROW, hp, c64::COLOR_WHITE);
    c64::draw_char(col, STATUS_ROW, c64::to_screen_code(b'/'), c64::COLOR_GREY);
    col += 1;
    col += c64::draw_number(col, STATUS_ROW, max_hp, c64::COLOR_WHITE);

    // Depth indicator
    col += 1;
    c64::draw_text(col, STATUS_ROW, b"D:", c64::COLOR_GREY);
    col += 2;
    col += c64::draw_number(col, STATUS_ROW, state.depth, c64::COLOR_WHITE);
    c64::draw_char(col, STATUS_ROW, c64::to_screen_code(b'/'), c64::COLOR_GREY);
    col += 1;
    col += c64::draw_number(col, STATUS_ROW, balance::TARGET_DEPTH, c64::COLOR_WHITE);

    // Kills counter
    col += 1;
    c64::draw_text(col, STATUS_ROW, b"K:", c64::COLOR_GREY);
    col += 2;
    c64::draw_number(col, STATUS_ROW, state.kills, c64::COLOR_WHITE);

    // Turn counter (right-aligned)
    c64::draw_text(33, STATUS_ROW, b"T:", c64::COLOR_GREY);
    c64::draw_number_u16(35, STATUS_ROW, state.turn_count, c64::COLOR_WHITE);
}

// ---------------------------------------------------------------------------
// Message formatting — GameEvent to fixed-width PETSCII buffer
// ---------------------------------------------------------------------------

fn copy_bytes(buf: &mut [u8; 40], pos: usize, src: &[u8]) -> usize {
    let mut p = pos;
    for &b in src {
        if p >= 40 {
            break;
        }
        buf[p] = b;
        p += 1;
    }
    p
}

fn copy_num(buf: &mut [u8; 40], pos: usize, val: u8) -> usize {
    let mut p = pos;
    if val >= 100 {
        if p < 40 {
            buf[p] = b'0' + val / 100;
            p += 1;
        }
    }
    if val >= 10 {
        if p < 40 {
            buf[p] = b'0' + (val / 10) % 10;
            p += 1;
        }
    }
    if p < 40 {
        buf[p] = b'0' + val % 10;
        p += 1;
    }
    p
}

/// Format a GameEvent into a 40-byte PETSCII buffer (space-padded).
fn format_event(event: GameEvent, buf: &mut [u8; 40]) {
    for b in buf.iter_mut() {
        *b = b' ';
    }
    let _ = match event {
        GameEvent::Attack {
            attacker,
            defender,
            damage,
        } => {
            let p = copy_bytes(buf, 0, attacker.name().as_bytes());
            let p = copy_bytes(buf, p, b" hit ");
            let p = copy_bytes(buf, p, defender.name().as_bytes());
            let p = copy_bytes(buf, p, b" for ");
            copy_num(buf, p, damage)
        }
        GameEvent::NoDamage {
            attacker,
            defender,
        } => {
            let p = copy_bytes(buf, 0, attacker.name().as_bytes());
            let p = copy_bytes(buf, p, b" hit ");
            let p = copy_bytes(buf, p, defender.name().as_bytes());
            copy_bytes(buf, p, b": no dmg")
        }
        GameEvent::Kill { attacker: _, victim } => {
            let p = copy_bytes(buf, 0, victim.name().as_bytes());
            copy_bytes(buf, p, b" is dead!")
        }
        GameEvent::EntityNotice { who } => {
            let p = copy_bytes(buf, 0, who.name().as_bytes());
            copy_bytes(buf, p, b" notices you!")
        }
        GameEvent::DrinkPotion { kind: _, healed } => {
            let p = copy_bytes(buf, 0, b"+");
            let p = copy_num(buf, p, healed);
            copy_bytes(buf, p, b" HP")
        }
        GameEvent::EquipWeapon { kind, bonus: _ } => {
            let p = copy_bytes(buf, 0, b"Equip ");
            copy_bytes(buf, p, items::name(kind).as_bytes())
        }
        GameEvent::EquipArmor { kind, bonus: _ } => {
            let p = copy_bytes(buf, 0, b"Equip ");
            copy_bytes(buf, p, items::name(kind).as_bytes())
        }
        GameEvent::NoStairs => copy_bytes(buf, 0, b"No stairs"),
        GameEvent::Descend { depth, target: _ } => {
            let p = copy_bytes(buf, 0, b"Depth ");
            copy_num(buf, p, depth)
        }
        GameEvent::Victory { depth: _ } => copy_bytes(buf, 0, b"Victory!"),
        GameEvent::Welcome => copy_bytes(buf, 0, b"Welcome!"),
        GameEvent::SoundCue { distance } => {
            let msg = match distance {
                SoundDistance::Near => b"You hear something nearby!" as &[u8],
                SoundDistance::Medium => b"You hear a distant sound..." as &[u8],
                SoundDistance::Far => b"You hear something far away..." as &[u8],
            };
            copy_bytes(buf, 0, msg)
        }
        GameEvent::PlayerDeath => copy_bytes(buf, 0, b"You have died!"),
    };
}

/// Render the 2 most recent messages on rows 23-24.
fn render_messages(state: &MicroGameState) {
    let mut buf = [b' '; 40];

    // Row 23: second most recent (dim)
    match state.log.recent(1) {
        Some(event) => {
            format_event(event, &mut buf);
            for i in 0..40u8 {
                c64::draw_char(
                    i,
                    MSG_ROW,
                    c64::to_screen_code(buf[i as usize]),
                    c64::COLOR_GREY,
                );
            }
        }
        None => {
            c64::fill_row(MSG_ROW, SC_SPACE, c64::COLOR_BLACK);
        }
    }

    // Row 24: most recent (bright)
    match state.log.recent(0) {
        Some(event) => {
            format_event(event, &mut buf);
            for i in 0..40u8 {
                c64::draw_char(
                    i,
                    MSG_ROW + 1,
                    c64::to_screen_code(buf[i as usize]),
                    c64::COLOR_WHITE,
                );
            }
        }
        None => {
            c64::fill_row(MSG_ROW + 1, SC_SPACE, c64::COLOR_BLACK);
        }
    }
}

// ---------------------------------------------------------------------------
// Differential rendering — snapshot + dirty-cell diffing
// ---------------------------------------------------------------------------

/// Viewport dirty bitfield size: ceil(40 * 22 / 8) = 110 bytes.
const DIRTY_SIZE: usize = ((VIEW_W as usize) * (VIEW_H as usize) + 7) / 8;

/// Packed alive-flag bitfield size for entities.
const ENTITY_ALIVE_BYTES: usize = (MAX_ENTITIES + 7) / 8;

/// Packed alive-flag bitfield size for items.
const ITEM_ALIVE_BYTES: usize = (MAX_ITEMS + 7) / 8;

/// Previous-frame snapshot for differential rendering.
///
/// Stored as `static mut` in main.rs (~810 bytes in BSS). Captures the
/// rendering-relevant state after each frame so the next frame can diff
/// against it and only redraw changed cells.
pub struct DiffState {
    pub viewport: (u8, u8),
    pub depth: u8,
    fov_visible: [u8; MAX_BITFIELD_SIZE],
    entity_x: [u8; MAX_ENTITIES],
    entity_y: [u8; MAX_ENTITIES],
    entity_alive: [u8; ENTITY_ALIVE_BYTES],
    entity_count: u8,
    item_x: [u8; MAX_ITEMS],
    item_y: [u8; MAX_ITEMS],
    item_alive: [u8; ITEM_ALIVE_BYTES],
    item_count: u8,
}

impl DiffState {
    /// Create a zeroed DiffState. Call `snapshot()` before first use.
    pub const fn new() -> Self {
        Self {
            viewport: (0, 0),
            depth: 0,
            fov_visible: [0; MAX_BITFIELD_SIZE],
            entity_x: [0; MAX_ENTITIES],
            entity_y: [0; MAX_ENTITIES],
            entity_alive: [0; ENTITY_ALIVE_BYTES],
            entity_count: 0,
            item_x: [0; MAX_ITEMS],
            item_y: [0; MAX_ITEMS],
            item_alive: [0; ITEM_ALIVE_BYTES],
            item_count: 0,
        }
    }

    /// Save the current rendering-relevant state for next-frame comparison.
    pub fn snapshot(&mut self, state: &MicroGameState) {
        self.viewport = viewport_pos(state);
        self.depth = state.depth;

        // Copy FOV visible bitfield
        let vis = state.fov.visible_bytes();
        self.fov_visible.copy_from_slice(vis);

        // Copy entity positions and pack alive flags
        let ec = state.entities.count as usize;
        self.entity_count = state.entities.count;
        self.entity_x[..ec].copy_from_slice(&state.entities.x[..ec]);
        self.entity_y[..ec].copy_from_slice(&state.entities.y[..ec]);
        for b in self.entity_alive.iter_mut() {
            *b = 0;
        }
        for i in 0..ec {
            if state.entities.alive[i] {
                self.entity_alive[i >> 3] |= 1u8 << (i & 7);
            }
        }

        // Copy item positions and pack alive flags
        let ic = state.items.count as usize;
        self.item_count = state.items.count;
        self.item_x[..ic].copy_from_slice(&state.items.x[..ic]);
        self.item_y[..ic].copy_from_slice(&state.items.y[..ic]);
        for b in self.item_alive.iter_mut() {
            *b = 0;
        }
        for i in 0..ic {
            if state.items.alive[i] {
                self.item_alive[i >> 3] |= 1u8 << (i & 7);
            }
        }
    }

    fn was_entity_alive(&self, i: usize) -> bool {
        self.entity_alive[i >> 3] & (1u8 << (i & 7)) != 0
    }

    fn was_item_alive(&self, i: usize) -> bool {
        self.item_alive[i >> 3] & (1u8 << (i & 7)) != 0
    }
}

/// Set a bit in the viewport dirty bitfield.
#[inline(always)]
fn mark_dirty(dirty: &mut [u8; DIRTY_SIZE], sx: u8, sy: u8) {
    let idx = (sy as usize) * (VIEW_W as usize) + (sx as usize);
    dirty[idx >> 3] |= 1u8 << (idx & 7);
}

/// Mark a world-coordinate position dirty if it falls within the viewport.
#[inline(always)]
fn mark_dirty_world(dirty: &mut [u8; DIRTY_SIZE], vx: u8, vy: u8, wx: u8, wy: u8) {
    if wx >= vx && wx < vx + VIEW_W && wy >= vy && wy < vy + VIEW_H {
        mark_dirty(dirty, wx - vx, wy - vy);
    }
}

/// Differential render: only redraw cells that changed since the last frame.
///
/// Falls back to `render_all()` on depth change (entire level replaced).
/// On viewport scroll, uses `render_map_sparse()` to skip cells where both
/// old and new world tiles are unexplored (~44% of viewport on average).
/// Otherwise, computes a dirty bitfield from FOV/entity/item changes and
/// redraws only those cells via `restore_tile()`.
pub fn render_diff(state: &MicroGameState, prev: &DiffState) {
    let (vx, vy) = viewport_pos(state);

    // Depth changed → full redraw (entire level changed)
    if state.depth != prev.depth {
        render_all(state);
        return;
    }

    // Viewport scrolled → sparse re-render (skip unexplored cells)
    if (vx, vy) != prev.viewport {
        let (old_vx, old_vy) = prev.viewport;
        render_map_sparse(state, vx, vy, old_vx, old_vy);
        render_items(state, vx, vy);
        render_entities(state, vx, vy);
        render_status_bar(state);
        render_messages(state);
        return;
    }

    let mut dirty = [0u8; DIRTY_SIZE];

    // --- 1. FOV visibility changes ---
    // XOR old and new visible bitfields; differing bits indicate tiles
    // that gained or lost visibility and need redrawing.
    let vis = state.fov.visible_bytes();
    let map_w = state.fov.width as usize;
    let fov_bytes_used = (map_w * (state.fov.height as usize) + 7) / 8;
    for byte_idx in 0..fov_bytes_used {
        let diff = prev.fov_visible[byte_idx] ^ vis[byte_idx];
        if diff == 0 {
            continue;
        }
        for bit in 0..8u8 {
            if diff & (1u8 << bit) == 0 {
                continue;
            }
            let tile_idx = byte_idx * 8 + (bit as usize);
            let wy = (tile_idx / map_w) as u8;
            let wx = (tile_idx % map_w) as u8;
            mark_dirty_world(&mut dirty, vx, vy, wx, wy);
        }
    }

    // --- 2. Entity position changes ---
    // Mark old positions of entities that moved, died, or were removed.
    let prev_ec = prev.entity_count as usize;
    for i in 0..prev_ec {
        if !prev.was_entity_alive(i) {
            continue;
        }
        let ox = prev.entity_x[i];
        let oy = prev.entity_y[i];
        let changed = if i < state.entities.count as usize {
            !state.entities.alive[i]
                || state.entities.x[i] != ox
                || state.entities.y[i] != oy
        } else {
            true
        };
        if changed {
            mark_dirty_world(&mut dirty, vx, vy, ox, oy);
        }
    }
    // Mark new positions of entities that moved or were spawned.
    for i in 0..state.entities.count as usize {
        if !state.entities.alive[i] {
            continue;
        }
        let ex = state.entities.x[i];
        let ey = state.entities.y[i];
        let changed = if i < prev_ec {
            !prev.was_entity_alive(i)
                || prev.entity_x[i] != ex
                || prev.entity_y[i] != ey
        } else {
            true
        };
        if changed {
            mark_dirty_world(&mut dirty, vx, vy, ex, ey);
        }
    }

    // --- 3. Item changes ---
    let prev_ic = prev.item_count as usize;
    for i in 0..prev_ic {
        if !prev.was_item_alive(i) {
            continue;
        }
        let ox = prev.item_x[i];
        let oy = prev.item_y[i];
        let changed = if i < state.items.count as usize {
            !state.items.alive[i]
                || state.items.x[i] != ox
                || state.items.y[i] != oy
        } else {
            true
        };
        if changed {
            mark_dirty_world(&mut dirty, vx, vy, ox, oy);
        }
    }
    for i in 0..state.items.count as usize {
        if !state.items.alive[i] {
            continue;
        }
        let ix = state.items.x[i];
        let iy = state.items.y[i];
        let changed = if i < prev_ic {
            !prev.was_item_alive(i)
                || prev.item_x[i] != ix
                || prev.item_y[i] != iy
        } else {
            true
        };
        if changed {
            mark_dirty_world(&mut dirty, vx, vy, ix, iy);
        }
    }

    // --- 4. Render dirty cells ---
    for byte_idx in 0..DIRTY_SIZE {
        if dirty[byte_idx] == 0 {
            continue;
        }
        for bit in 0..8u8 {
            if dirty[byte_idx] & (1u8 << bit) == 0 {
                continue;
            }
            let cell_idx = byte_idx * 8 + (bit as usize);
            if cell_idx >= (VIEW_W as usize) * (VIEW_H as usize) {
                break;
            }
            let sx = (cell_idx % (VIEW_W as usize)) as u8;
            let sy = (cell_idx / (VIEW_W as usize)) as u8;
            restore_tile(state, vx, vy, sx + vx, sy + vy);
        }
    }

    // --- 5. Status bar and messages (always, cheap) ---
    render_status_bar(state);
    render_messages(state);
}

// ---------------------------------------------------------------------------
// Look mode rendering
// ---------------------------------------------------------------------------

/// Compute viewport origin centered on the look cursor instead of the player.
/// Clamps to map edges identically to `viewport_pos`.
pub fn look_viewport(state: &MicroGameState, cx: u8, cy: u8) -> (u8, u8) {
    let vx = cx.saturating_sub(VIEW_W / 2).min(state.map.width.saturating_sub(VIEW_W));
    let vy = cy.saturating_sub(VIEW_H / 2).min(state.map.height.saturating_sub(VIEW_H));
    (vx, vy)
}

/// Full look-mode render: map + entities + cursor + look status bar + messages.
/// Uses the provided viewport (cursor-centered) rather than player-centered.
pub fn render_look(state: &MicroGameState, vx: u8, vy: u8, cx: u8, cy: u8) {
    render_map(state, vx, vy);
    render_items(state, vx, vy);
    render_entities(state, vx, vy);

    // Cursor overlay
    let sx = cx - vx;
    let sy = cy - vy;
    c64::draw_char(sx, sy, c64::to_screen_code(b'X'), c64::COLOR_YELLOW);

    // Look status bar (replaces normal status bar)
    render_look_status(state, cx, cy);

    render_messages(state);
}

/// Restore a single world tile at (wx, wy) to its normal appearance.
/// Redraws the terrain, then overlays any item, then any entity — the
/// same layering as render_map + render_items + render_entities but for
/// a single cell. Used by look mode to erase the cursor from the old
/// position without a full-screen redraw.
pub fn restore_tile(state: &MicroGameState, vx: u8, vy: u8, wx: u8, wy: u8) {
    let sx = wx - vx;
    let sy = wy - vy;

    // 1. Terrain layer
    let (sc, color) = tile_appearance(state, wx, wy);
    c64::draw_char(sx, sy, sc, color);
    let visible = state.fov.is_visible(wx, wy);

    // 2. Item layer (only if visible)
    if visible {
        for i in 0..state.items.count as usize {
            if state.items.alive[i] && state.items.x[i] == wx && state.items.y[i] == wy {
                let kind = state.items.kind[i];
                let glyph = items::glyph(kind) as u8;
                let c = game_color_to_c64(items::color(kind));
                c64::draw_char(sx, sy, c64::to_screen_code(glyph), c);
                break;
            }
        }

        // 3. Entity layer (only if visible — entities occlude items)
        for i in 0..state.entities.count {
            let idx = i as usize;
            if !state.entities.alive[idx] {
                continue;
            }
            if state.entities.x[idx] == wx && state.entities.y[idx] == wy {
                let (glyph, c) = if i == PLAYER_IDX {
                    (balance::PLAYER_GLYPH as u8, c64::COLOR_YELLOW)
                } else {
                    match state.entities.kind[idx] {
                        Some(kind) => {
                            let g = monster_table::glyph(kind) as u8;
                            (g, game_color_to_c64(monster_table::color(kind)))
                        }
                        None => (b'?', c64::COLOR_WHITE),
                    }
                };
                c64::draw_char(sx, sy, c64::to_screen_code(glyph), c);
                break;
            }
        }
    }
}

/// Draw the look cursor (yellow 'X') at world position (cx, cy).
pub fn draw_cursor(vx: u8, vy: u8, cx: u8, cy: u8) {
    let sx = cx - vx;
    let sy = cy - vy;
    c64::draw_char(sx, sy, c64::to_screen_code(b'X'), c64::COLOR_YELLOW);
}

/// Render the look mode status bar on row 22, replacing the normal status bar.
/// Shows: [L] terrain + entity/item name based on visibility.
pub fn render_look_status(state: &MicroGameState, cx: u8, cy: u8) {
    let mut buf = [b' '; 40];

    let mut p = copy_bytes(&mut buf, 0, b"[L] ");

    if !state.fov.is_explored(cx, cy) {
        p = copy_bytes(&mut buf, p, b"Unexplored");
    } else {
        let visible = state.fov.is_visible(cx, cy);

        let tile = state.map.tile_at(cx, cy);
        match tile {
            TILE_FLOOR => p = copy_bytes(&mut buf, p, b"Floor"),
            TILE_STAIRS_DOWN => p = copy_bytes(&mut buf, p, b"Stairs"),
            TILE_STRUCTURAL => p = copy_bytes(&mut buf, p, b"Wall"),
            TILE_WALL => p = copy_bytes(&mut buf, p, b"Void"),
            _ => p = copy_bytes(&mut buf, p, b"???"),
        }

        if !visible {
            p = copy_bytes(&mut buf, p, b" (dim)");
        } else {
            let eidx = state.entities.entity_at(cx, cy);
            if eidx != NO_ENTITY {
                if p < 40 { buf[p] = b' '; p += 1; }
                if eidx == PLAYER_IDX {
                    p = copy_bytes(&mut buf, p, b"Player");
                } else if let Some(kind) = state.entities.kind[eidx as usize] {
                    p = copy_bytes(&mut buf, p, monster_table::name(kind).as_bytes());
                }
            }

            for i in 0..state.items.count as usize {
                if state.items.alive[i] && state.items.x[i] == cx && state.items.y[i] == cy {
                    if p < 40 { buf[p] = b' '; p += 1; }
                    if p < 40 { buf[p] = b'['; p += 1; }
                    p = copy_bytes(&mut buf, p, items::name(state.items.kind[i]).as_bytes());
                    if p < 40 { buf[p] = b']'; p += 1; }
                    break;
                }
            }
        }
    }
    let _ = p;

    for i in 0..40u8 {
        c64::draw_char(
            i,
            STATUS_ROW,
            c64::to_screen_code(buf[i as usize]),
            c64::COLOR_CYAN,
        );
    }
}

// ---------------------------------------------------------------------------
// Menu rendering helpers
// ---------------------------------------------------------------------------

/// Draw a list of menu items. Selected item gets `>` prefix in yellow,
/// others get a space prefix in light grey.
pub fn draw_menu(items: &[&[u8]], selected: u8, x: u8, y: u8) {
    for (i, item) in items.iter().enumerate() {
        let row = y + (i as u8) * 2; // 2-row spacing between items
        let is_selected = i as u8 == selected;

        // Clear the row area (item + prefix)
        for col in x..(x + 20) {
            if col < 40 {
                c64::draw_char(col, row, SC_SPACE, c64::COLOR_BLACK);
            }
        }

        if is_selected {
            c64::draw_char(x, row, c64::to_screen_code(b'>'), c64::COLOR_YELLOW);
            c64::draw_text(x + 2, row, item, c64::COLOR_YELLOW);
        } else {
            c64::draw_text(x + 2, row, item, c64::COLOR_LGREY);
        }
    }
}

/// Clear a rectangular region of the screen.
fn clear_rect(bx: u8, by: u8, bw: u8, bh: u8) {
    for y in by..(by + bh) {
        for x in bx..(bx + bw) {
            c64::draw_char(x, y, SC_SPACE, c64::COLOR_BLACK);
        }
    }
}

// ---------------------------------------------------------------------------
// Game over, title, pause, and seed input screens
// ---------------------------------------------------------------------------

/// Shared end-of-game screen (death or victory).
fn render_end_screen(state: &MicroGameState, selected: u8, title: &[u8], title_color: u8) {
    let bx: u8 = 8;
    let by: u8 = 7;
    let bw: u8 = 24;
    let bh: u8 = 9;

    clear_rect(bx, by, bw, bh);
    c64::fill_row(by, 0xC0, title_color);

    c64::draw_text(bx + 2, by + 1, title, title_color);

    c64::draw_text(bx + 2, by + 3, b"Kills: ", c64::COLOR_GREY);
    c64::draw_number(bx + 9, by + 3, state.kills, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 4, b"Turns: ", c64::COLOR_GREY);
    c64::draw_number_u16(bx + 9, by + 4, state.turn_count, c64::COLOR_WHITE);

    let menu_items: [&[u8]; 2] = [b"Play Again", b"Title Screen"];
    draw_menu(&menu_items, selected, bx + 4, by + 6);
}

/// Render the game over screen overlay with menu items.
pub fn render_game_over(state: &MicroGameState, selected: u8) {
    render_end_screen(state, selected, b"YOU HAVE DIED", c64::COLOR_RED);
}

/// Render the victory screen overlay with menu items.
pub fn render_victory(state: &MicroGameState, selected: u8) {
    render_end_screen(state, selected, b"VICTORY!", c64::COLOR_GREEN);
}

/// Render the title screen with menu.
pub fn render_title(selected: u8) {
    c64::clear_screen();

    c64::draw_text(8, 4, b"ROGUELIKE DUNGEON", c64::COLOR_WHITE);
    c64::draw_text(8, 5, b"CRAWLER", c64::COLOR_WHITE);
    c64::draw_text(8, 7, b"C64 + RUST-MOS", c64::COLOR_LGREY);

    let menu_items: [&[u8]; 2] = [b"NEW GAME", b"ENTER SEED"];
    draw_menu(&menu_items, selected, 10, 10);
}

/// Render the pause menu overlay on top of the game screen.
pub fn render_pause(state: &MicroGameState, selected: u8) {
    render_all(state);

    let bx: u8 = 8;
    let by: u8 = 8;
    let bw: u8 = 24;
    let bh: u8 = 7;

    clear_rect(bx, by, bw, bh);
    c64::fill_row(by, 0xC0, c64::COLOR_CYAN);

    c64::draw_text(bx + 2, by + 1, b"PAUSED", c64::COLOR_CYAN);

    let menu_items: [&[u8]; 2] = [b"Resume", b"New Game"];
    draw_menu(&menu_items, selected, bx + 4, by + 3);
}

/// Render the seed code text input dialog.
pub fn render_seed_input(buf: &[u8], len: u8) {
    let bx: u8 = 5;
    let by: u8 = 9;
    let bw: u8 = 30;
    let bh: u8 = 6;

    clear_rect(bx, by, bw, bh);
    c64::fill_row(by, 0xC0, c64::COLOR_CYAN);

    c64::draw_text(bx + 2, by + 1, b"ENTER SEED CODE", c64::COLOR_CYAN);

    let field_x = bx + 3;
    let field_y = by + 3;
    let field_w: u8 = 16;
    for i in 0..field_w {
        c64::draw_char(field_x + i, field_y, c64::to_screen_code(b'_'), c64::COLOR_DGREY);
    }

    for i in 0..len {
        let ch = buf[i as usize];
        let display = if ch >= b'a' && ch <= b'z' {
            ch - b'a' + b'A'
        } else {
            ch
        };
        c64::draw_char(field_x + i, field_y, c64::to_screen_code(display), c64::COLOR_WHITE);
    }

    if (len as u8) < field_w {
        c64::draw_char(field_x + len, field_y, 0xA0, c64::COLOR_YELLOW);
    }
}

/// Render a brief error message overlay for invalid seed codes.
pub fn render_seed_error() {
    let bx: u8 = 8;
    let by: u8 = 10;
    let bw: u8 = 24;
    let bh: u8 = 3;

    clear_rect(bx, by, bw, bh);
    c64::draw_text(bx + 2, by, b"INVALID SEED", c64::COLOR_RED);
    c64::draw_text(bx + 2, by + 2, b"PRESS ANY KEY", c64::COLOR_DGREY);
}
