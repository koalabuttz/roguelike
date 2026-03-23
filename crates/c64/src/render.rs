// Screen rendering — VIC-II character mode with viewport scrolling.
//
// Reads game state from &MicroGameState (roguelike-core::tier_micro).
// The 64x48 tile map is viewed through a 40x22 player-centered viewport.
//
// Layout:
//   Rows 0-21:  Map viewport (40x22 tiles from the 64x48 map)
//   Row 22:     Status bar (HP bar, ATK/DEF, depth, kills)
//   Rows 23-24: Message log (2 most recent GameEvents, formatted to PETSCII)

use core::ptr::write_volatile;

use crate::c64;
use roguelike_core::rules::balance;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::items;
use roguelike_core::rules::health::{self, HealthTier};
use roguelike_core::rules::message::{GameEvent, SoundDistance};
use roguelike_core::rules::monster_table;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::item_store::MAX_ITEMS;
use roguelike_core::tier_micro::msglog::MSG_COUNT;
use roguelike_core::tier_micro::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL, TILE_WALL};
use roguelike_core::tier_micro::types::{BIT, MAX_BITFIELD_SIZE, MAX_ENTITIES, NO_ENTITY, PLAYER_IDX};

// Screen codes for map tiles
const SC_SPACE: u8 = 0x20;
const SC_FLOOR: u8 = 0x2E;  // .
const SC_WALL: u8 = 0xA0;   // reverse space = solid block
const SC_STAIRS: u8 = 0x3E; // >
const SC_CORPSE: u8 = 0x25; // %

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

/// Dead-zone margin: the player can roam this many tiles from the
/// viewport edge before a scroll is triggered.  With VIEW_W=40 and
/// VIEW_H=22 this gives a 30×12 free-movement zone.
const DEADZONE: u8 = 5;

/// Dead-zone viewport positioning.  Returns the previous viewport
/// unchanged unless the player is within DEADZONE tiles of an edge,
/// in which case the viewport shifts by exactly 1 tile in the
/// breached direction(s).  Clamped to valid map bounds.
pub fn viewport_pos_lazy(
    state: &MicroGameState,
    prev_vx: u8,
    prev_vy: u8,
) -> (u8, u8) {
    let px = state.entities.x[PLAYER_IDX as usize];
    let py = state.entities.y[PLAYER_IDX as usize];

    let max_vx = state.map.width.saturating_sub(VIEW_W);
    let max_vy = state.map.height.saturating_sub(VIEW_H);

    let new_vx = if px < prev_vx + DEADZONE {
        prev_vx.saturating_sub(1).min(max_vx)
    } else if px >= prev_vx + VIEW_W - DEADZONE {
        (prev_vx + 1).min(max_vx)
    } else {
        prev_vx
    };

    let new_vy = if py < prev_vy + DEADZONE {
        prev_vy.saturating_sub(1).min(max_vy)
    } else if py >= prev_vy + VIEW_H - DEADZONE {
        (prev_vy + 1).min(max_vy)
    } else {
        prev_vy
    };

    (new_vx, new_vy)
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

/// Map a raw tile value to (screen_code, color) for the given visibility.
fn tile_sc_color(tile: u8, visible: bool) -> (u8, u8) {
    match tile {
        TILE_FLOOR => (SC_FLOOR, if visible { c64::COLOR_DGREY } else { c64::COLOR_BLUE }),
        TILE_STAIRS_DOWN => (SC_STAIRS, if visible { c64::COLOR_CYAN } else { c64::COLOR_BLUE }),
        TILE_STRUCTURAL => (SC_WALL, if visible { c64::COLOR_LGREY } else { c64::COLOR_BLUE }),
        _ => (SC_SPACE, c64::COLOR_BLACK),
    }
}

/// Compute the screen code and color for a map tile based on visibility.
fn tile_appearance(state: &MicroGameState, wx: u8, wy: u8) -> (u8, u8) {
    if state.fov.is_visible(wx, wy) {
        tile_sc_color(state.map.tile_at(wx, wy), true)
    } else if state.fov.is_explored(wx, wy) {
        tile_sc_color(state.map.tile_at(wx, wy), false)
    } else {
        (SC_SPACE, c64::COLOR_BLACK)
    }
}

/// Full screen render: map + items + entities + status + messages.
pub fn render_all(state: &MicroGameState) {
    c64::sync_frame();
    let (vx, vy) = viewport_pos(state);
    render_map(state, vx, vy);
    render_items(state, vx, vy);
    render_entities(state, vx, vy);
    render_status_bar(state);
    render_messages(state);
}

/// Extract a 4-bit packed tile from the tile array using a linear index.
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
            let bit = BIT[fi & 7];

            let (sc, color) = if vis[byte_idx] & bit != 0 {
                tile_sc_color(tile_at_index(tiles, fi), true)
            } else if exp[byte_idx] & bit != 0 {
                tile_sc_color(tile_at_index(tiles, fi), false)
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
            let new_explored = exp[fi >> 3] & (BIT[fi & 7]) != 0;
            let old_explored = exp[old_fi >> 3] & (BIT[old_fi & 7]) != 0;

            if !new_explored && !old_explored {
                fi += 1;
                old_fi += 1;
                si += 1;
                continue;
            }

            let byte_idx = fi >> 3;
            let bit = BIT[fi & 7];

            let (sc, color) = if vis[byte_idx] & bit != 0 {
                tile_sc_color(tile_at_index(tiles, fi), true)
            } else if new_explored {
                tile_sc_color(tile_at_index(tiles, fi), false)
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

// ---------------------------------------------------------------------------
// Memory-copy viewport scrolling (1-tile shifts)
// ---------------------------------------------------------------------------

/// Shift screen RAM and color RAM vertically by 1 row.
/// `dy > 0` (scroll down): copy rows 1..VIEW_H to 0..VIEW_H-1.
/// `dy < 0` (scroll up):   copy rows 0..VIEW_H-1 to 1..VIEW_H.
fn scroll_vertical(dy: i8) {
    let row_bytes = VIEW_W as usize; // 40
    let total = row_bytes * (VIEW_H as usize - 1); // 840

    unsafe {
        if dy > 0 {
            core::ptr::copy(c64::SCREEN.add(row_bytes), c64::SCREEN, total);
            core::ptr::copy(c64::COLOR_RAM.add(row_bytes), c64::COLOR_RAM, total);
        } else {
            core::ptr::copy(c64::SCREEN, c64::SCREEN.add(row_bytes), total);
            core::ptr::copy(c64::COLOR_RAM, c64::COLOR_RAM.add(row_bytes), total);
        }
    }
}

/// Shift screen RAM and color RAM horizontally by 1 column.
/// `dx > 0` (scroll right): for each row, copy cols 1..VIEW_W to 0..VIEW_W-1.
/// `dx < 0` (scroll left):  for each row, copy cols 0..VIEW_W-1 to 1..VIEW_W.
fn scroll_horizontal(dx: i8) {
    let copy_len = (VIEW_W - 1) as usize; // 39

    for row in 0..VIEW_H as usize {
        let base = row * (VIEW_W as usize);
        unsafe {
            if dx > 0 {
                core::ptr::copy(c64::SCREEN.add(base + 1), c64::SCREEN.add(base), copy_len);
                core::ptr::copy(c64::COLOR_RAM.add(base + 1), c64::COLOR_RAM.add(base), copy_len);
            } else {
                core::ptr::copy(c64::SCREEN.add(base), c64::SCREEN.add(base + 1), copy_len);
                core::ptr::copy(c64::COLOR_RAM.add(base), c64::COLOR_RAM.add(base + 1), copy_len);
            }
        }
    }
}

/// Row-wise diagonal scroll using `ptr::copy` per row.
///
/// Each row's 39-byte slice is copied in bulk rather than per-cell,
/// matching the approach used by `scroll_vertical`/`scroll_horizontal`.
/// Screen rows are copied first (time-critical — stays ahead of the
/// raster with the vblank head start), then color rows (may lag by
/// one frame but is fixed by the subsequent `refresh_fov_area` pass).
///
/// Within each row, source and destination don't overlap (they are in
/// different rows, copying 39 out of 40 columns), so `ptr::copy` is
/// safe.  Row iteration order (top-to-bottom for dy>0, bottom-to-top
/// for dy<0) ensures each source row is read before being overwritten.
fn scroll_diagonal(dx: i8, dy: i8) {
    // Screen first — glyph correctness is most visible
    copy_diagonal_rows(c64::SCREEN, dx, dy);
    // Color second — stale colors for one frame are barely noticeable
    copy_diagonal_rows(c64::COLOR_RAM, dx, dy);
}

/// Copy 21 rows of 39 bytes diagonally within a 40-column buffer.
fn copy_diagonal_rows(base: *mut u8, dx: i8, dy: i8) {
    let rows = (VIEW_H - 1) as usize;
    let cols = (VIEW_W - 1) as usize;
    let src_col_start = if dx > 0 { 1usize } else { 0 };
    let dst_col_start = if dx > 0 { 0usize } else { 1 };

    for row_step in 0..rows {
        let dst_row = if dy > 0 {
            row_step
        } else {
            VIEW_H as usize - 1 - row_step
        };
        let src_row = (dst_row as isize + dy as isize) as usize;

        unsafe {
            core::ptr::copy(
                base.add(src_row * 40 + src_col_start),
                base.add(dst_row * 40 + dst_col_start),
                cols,
            );
        }
    }
}

/// Render a single viewport row at screen row `sy`.
fn render_edge_row(state: &MicroGameState, vx: u8, vy: u8, sy: u8) {
    let wy = vy + sy;
    let vis = state.fov.visible_bytes();
    let exp = state.fov.explored_bytes();
    let tiles = &state.map.tiles;
    let map_w = state.fov.width as usize;

    let fi_base = (wy as usize) * map_w + (vx as usize);
    let si_base = (sy as usize) * (VIEW_W as usize);

    for sx in 0..VIEW_W as usize {
        let fi = fi_base + sx;
        let byte_idx = fi >> 3;
        let bit = BIT[fi & 7];

        let (sc, color) = if vis[byte_idx] & bit != 0 {
            tile_sc_color(tile_at_index(tiles, fi), true)
        } else if exp[byte_idx] & bit != 0 {
            tile_sc_color(tile_at_index(tiles, fi), false)
        } else {
            (SC_SPACE, c64::COLOR_BLACK)
        };

        unsafe {
            write_volatile(c64::SCREEN.add(si_base + sx), sc);
            write_volatile(c64::COLOR_RAM.add(si_base + sx), color);
        }
    }
}

/// Render a single viewport column at screen column `sx`.
fn render_edge_col(state: &MicroGameState, vx: u8, vy: u8, sx: u8) {
    let wx = vx + sx;
    let vis = state.fov.visible_bytes();
    let exp = state.fov.explored_bytes();
    let tiles = &state.map.tiles;
    let map_w = state.fov.width as usize;

    for sy in 0..VIEW_H as usize {
        let wy = vy as usize + sy;
        let fi = wy * map_w + (wx as usize);
        let si = sy * (VIEW_W as usize) + (sx as usize);
        let byte_idx = fi >> 3;
        let bit = BIT[fi & 7];

        let (sc, color) = if vis[byte_idx] & bit != 0 {
            tile_sc_color(tile_at_index(tiles, fi), true)
        } else if exp[byte_idx] & bit != 0 {
            tile_sc_color(tile_at_index(tiles, fi), false)
        } else {
            (SC_SPACE, c64::COLOR_BLACK)
        };

        unsafe {
            write_volatile(c64::SCREEN.add(si), sc);
            write_volatile(c64::COLOR_RAM.add(si), color);
        }
    }
}

/// Handle a viewport scroll.  Uses memory-copy for 1-tile shifts,
/// falls back to sparse render for larger deltas.  Always re-renders
/// items, entities, status bar, and messages.
///
/// `prev` is needed for the memory-copy path: the copy shifts old
/// entity/item glyphs embedded in screen RAM, creating ghosts.  We
/// erase them by restoring tiles at the old entity/item positions.
pub fn render_viewport_scroll(
    state: &MicroGameState,
    prev: &DiffState,
    new_vx: u8,
    new_vy: u8,
    old_vx: u8,
    old_vy: u8,
) {
    let dx = (new_vx as i8) - (old_vx as i8);
    let dy = (new_vy as i8) - (old_vy as i8);

    if dx >= -1 && dx <= 1 && dy >= -1 && dy <= 1 {
        // 1-tile scroll: memory-copy + edge render
        c64::sync_frame();
        if dx != 0 && dy != 0 {
            // Diagonal: single-pass to avoid visible intermediate state
            scroll_diagonal(dx, dy);
        } else {
            // Cardinal: bulk copy
            if dy != 0 {
                scroll_vertical(dy);
            }
            if dx != 0 {
                scroll_horizontal(dx);
            }
        }
        // Render newly revealed edges
        if dy == 1 {
            render_edge_row(state, new_vx, new_vy, VIEW_H - 1);
        } else if dy == -1 {
            render_edge_row(state, new_vx, new_vy, 0);
        }
        if dx == 1 {
            render_edge_col(state, new_vx, new_vy, VIEW_W - 1);
        } else if dx == -1 {
            render_edge_col(state, new_vx, new_vy, 0);
        }

        // The copy shifted old FOV lighting and entity glyphs.
        // Re-render all tiles that are or were visible to fix both.
        refresh_fov_area(state, prev, new_vx, new_vy);
    } else {
        // Large scroll: fall back to sparse render (rewrites all
        // explored/visible cells, so no ghost issue).
        render_map_sparse(state, new_vx, new_vy, old_vx, old_vy);
    }

    render_items(state, new_vx, new_vy);
    render_entities(state, new_vx, new_vy);
    render_status_bar(state);
    render_messages(state);
}

/// Re-render terrain for all viewport tiles that are currently visible
/// or were visible in the previous frame.  This fixes two issues after
/// a memory-copy scroll:
/// 1. FOV lighting — tiles that gained/lost visibility get correct colors
/// 2. Entity ghosts — old entity glyphs in the visible area are overwritten
fn refresh_fov_area(
    state: &MicroGameState,
    prev: &DiffState,
    vx: u8,
    vy: u8,
) {
    let vis = state.fov.visible_bytes();
    let tiles = &state.map.tiles;
    let map_w = state.fov.width as usize;

    let mut fov_row = (vy as usize) * map_w + (vx as usize);
    let mut scr_row: usize = 0;

    for _sy in 0..VIEW_H as usize {
        let mut fi = fov_row;
        let mut si = scr_row;

        for _sx in 0..VIEW_W as usize {
            let byte_idx = fi >> 3;
            let bit = BIT[fi & 7];

            let is_visible = vis[byte_idx] & bit != 0;
            let was_visible = prev.fov_visible[byte_idx] & bit != 0;

            if is_visible || was_visible {
                let (sc, color) = tile_sc_color(tile_at_index(tiles, fi), is_visible);

                unsafe {
                    write_volatile(c64::SCREEN.add(si), sc);
                    write_volatile(c64::COLOR_RAM.add(si), color);
                }
            }

            fi += 1;
            si += 1;
        }

        fov_row += map_w;
        scr_row += 40;
    }
}

/// Render ground items that are visible and within the viewport.
fn render_items(state: &MicroGameState, vx: u8, vy: u8) {
    for i in 0..(state.items.count as usize).min(MAX_ITEMS) {
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
        c64::draw_char(sx, sy, glyph, color);
    }
}

/// Render all entities that are visible and within the viewport.
/// Dead non-player entities are drawn as corpse glyphs (%).
/// Alive entities are drawn with their normal glyph/color.
fn render_entities(state: &MicroGameState, vx: u8, vy: u8) {
    for i in 0..(state.entities.count).min(MAX_ENTITIES as u8) {
        let idx = i as usize;
        let ex = state.entities.x[idx];
        let ey = state.entities.y[idx];
        if !state.fov.is_visible(ex, ey) {
            continue;
        }
        if ex < vx || ex >= vx + VIEW_W || ey < vy || ey >= vy + VIEW_H {
            continue;
        }

        let (glyph, color) = if !state.entities.alive[idx] {
            if i == PLAYER_IDX { continue; }
            (SC_CORPSE, c64::COLOR_BROWN)
        } else if i == PLAYER_IDX {
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

        c64::draw_char(ex - vx, ey - vy, glyph, color);
    }
}

/// Render the status bar on row 22.
#[inline(never)]
fn render_status_bar(state: &MicroGameState) {
    c64::fill_row(STATUS_ROW, SC_SPACE, c64::COLOR_BLACK);

    let hp = state.entities.hp[PLAYER_IDX as usize];
    let max_hp = state.entities.max_hp[PLAYER_IDX as usize];

    // "HP " label
    c64::draw_text(0, STATUS_ROW, b"HP ", c64::COLOR_WHITE);

    // HP bar: 8 chars wide, filled proportionally
    let bar_width: u8 = 8;
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
            c64::draw_sc(3 + i, STATUS_ROW, 0xA0, bar_color); // filled block
        } else {
            c64::draw_sc(3 + i, STATUS_ROW, 0x65, c64::COLOR_DGREY); // light shade
        }
    }

    // "HP/MaxHP" after bar
    let mut col = 3 + bar_width + 1;
    col += c64::draw_number(col, STATUS_ROW, hp, c64::COLOR_WHITE);
    c64::draw_char(col, STATUS_ROW, b'/', c64::COLOR_GREY);
    col += 1;
    col += c64::draw_number(col, STATUS_ROW, max_hp, c64::COLOR_WHITE);

    // ATK stat (effective total = base + equipment)
    col += 1;
    c64::draw_char(col, STATUS_ROW, b'A', c64::COLOR_GREY);
    col += 1;
    let eff_atk = state.effective_attack();
    col += c64::draw_number(col, STATUS_ROW, eff_atk, c64::COLOR_WHITE);

    // DEF stat (effective total = base + equipment)
    col += 1;
    c64::draw_char(col, STATUS_ROW, b'D', c64::COLOR_GREY);
    col += 1;
    let eff_def = state.effective_defense();
    col += c64::draw_number(col, STATUS_ROW, eff_def, c64::COLOR_WHITE);

    // Depth indicator (using > glyph to avoid confusion with D for DEF)
    col += 1;
    c64::draw_char(col, STATUS_ROW, b'>', c64::COLOR_GREY);
    col += 1;
    col += c64::draw_number(col, STATUS_ROW, state.depth, c64::COLOR_WHITE);
    c64::draw_char(col, STATUS_ROW, b'/', c64::COLOR_GREY);
    col += 1;
    c64::draw_number(col, STATUS_ROW, balance::TARGET_DEPTH, c64::COLOR_WHITE);

    // Kills counter — fixed position so it never vanishes
    c64::draw_char(30, STATUS_ROW, b'K', c64::COLOR_GREY);
    c64::draw_number(31, STATUS_ROW, state.kills, c64::COLOR_WHITE);

    // Turn counter — fixed position
    c64::draw_char(35, STATUS_ROW, b'T', c64::COLOR_GREY);
    c64::draw_number_u16(36, STATUS_ROW, state.turn_count, c64::COLOR_WHITE);
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
#[inline(never)]
fn format_event(event: GameEvent, buf: &mut [u8; 40]) {
    *buf = [b' '; 40];
    let _ = match event {
        GameEvent::Attack {
            attacker,
            defender,
            damage: _,
        } => {
            let p = copy_bytes(buf, 0, attacker.name().as_bytes());
            let p = copy_bytes(buf, p, b" hits ");
            copy_bytes(buf, p, defender.name().as_bytes())
        }
        GameEvent::HealthStatus { who, tier } => {
            let p = copy_bytes(buf, 0, who.name().as_bytes());
            let desc: &[u8] = match tier {
                HealthTier::Healthy => b": healthy",
                HealthTier::Moderate => b": damaged",
                HealthTier::Severe => b": wounded",
                HealthTier::AlmostDead => b": dying!",
            };
            copy_bytes(buf, p, desc)
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
        GameEvent::EquipWeapon { kind, bonus } | GameEvent::EquipArmor { kind, bonus } => {
            let p = copy_bytes(buf, 0, b"Equip ");
            let mut p = copy_bytes(buf, p, items::name(kind).as_bytes());
            // Inline " +N" to avoid extra function calls (tight .noinit budget).
            // Bonus values are single-digit (2-3) with current items.
            if p < 37 {
                buf[p] = b' ';
                buf[p + 1] = b'+';
                buf[p + 2] = b'0' + bonus;
                p += 3;
            }
            p
        }
        GameEvent::UnequipWeapon { kind } | GameEvent::UnequipArmor { kind } => {
            let p = copy_bytes(buf, 0, b"Unequip ");
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
        GameEvent::PickupItem { kind } => {
            let p = copy_bytes(buf, 0, b"Got ");
            copy_bytes(buf, p, items::name(kind).as_bytes())
        }
        GameEvent::DropItem { kind } => {
            let p = copy_bytes(buf, 0, b"Drop ");
            copy_bytes(buf, p, items::name(kind).as_bytes())
        }
        GameEvent::InventoryFull => copy_bytes(buf, 0, b"Inventory full!"),
        GameEvent::ItemsHere { kind, count } => {
            if count <= 1 {
                let p = copy_bytes(buf, 0, b"See: ");
                copy_bytes(buf, p, items::name(kind).as_bytes())
            } else {
                let p = copy_bytes(buf, 0, b"See ");
                let p = copy_num(buf, p, count);
                let p = copy_bytes(buf, p, b"x ");
                copy_bytes(buf, p, items::name(kind).as_bytes())
            }
        }
        GameEvent::Autorun => copy_bytes(buf, 0, b"Running..."),
        GameEvent::AutorunStop { cause } => {
            use roguelike_core::rules::message::AutorunStopCause;
            let msg = match cause {
                AutorunStopCause::WallReached => b"Path blocked." as &[u8],
                AutorunStopCause::MonsterSpotted => b"Monster spotted!",
                AutorunStopCause::DamageTaken => b"You take damage!",
                AutorunStopCause::GameOver => b"You have died!",
                AutorunStopCause::CorridorBranches => b"Path branches.",
                AutorunStopCause::MaxSteps => b"You stop running.",
                AutorunStopCause::PathComplete => b"Arrived.",
                AutorunStopCause::StairsFound => b"Stairs here.",
            };
            copy_bytes(buf, 0, msg)
        }
    };
}

/// Render the 2 most recent messages on rows 23-24.
#[inline(never)]
pub(crate) fn render_messages(state: &MicroGameState) {
    let mut buf = [b' '; 40];

    // Row 23: second most recent (dim)
    match state.log.recent(1) {
        Some(event) => {
            format_event(event, &mut buf);
            for i in 0..40u8 {
                c64::draw_char(i, MSG_ROW, buf[i as usize], c64::COLOR_GREY);
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
                c64::draw_char(i, MSG_ROW + 1, buf[i as usize], c64::COLOR_WHITE);
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
    /// The viewport is passed explicitly because the main loop may use
    /// `viewport_pos_lazy` (dead-zone) instead of always-center.
    pub fn snapshot(&mut self, state: &MicroGameState, viewport: (u8, u8)) {
        self.viewport = viewport;
        self.depth = state.depth;

        // Copy FOV visible bitfield
        self.fov_visible = *state.fov.visible_bytes();

        // Copy entity positions and pack alive flags
        let ec = (state.entities.count as usize).min(MAX_ENTITIES);
        self.entity_count = state.entities.count;
        self.entity_alive = [0; ENTITY_ALIVE_BYTES];
        for i in 0..ec {
            self.entity_x[i] = state.entities.x[i];
            self.entity_y[i] = state.entities.y[i];
            if state.entities.alive[i] {
                self.entity_alive[i >> 3] |= BIT[i & 7];
            }
        }

        // Copy item positions and pack alive flags
        let ic = (state.items.count as usize).min(MAX_ITEMS);
        self.item_count = state.items.count;
        self.item_alive = [0; ITEM_ALIVE_BYTES];
        for i in 0..ic {
            self.item_x[i] = state.items.x[i];
            self.item_y[i] = state.items.y[i];
            if state.items.alive[i] {
                self.item_alive[i >> 3] |= BIT[i & 7];
            }
        }
    }

    fn was_entity_alive(&self, i: usize) -> bool {
        self.entity_alive[i >> 3] & (BIT[i & 7]) != 0
    }

    fn was_item_alive(&self, i: usize) -> bool {
        self.item_alive[i >> 3] & (BIT[i & 7]) != 0
    }
}

/// Set a bit in the viewport dirty bitfield.
fn mark_dirty(dirty: &mut [u8; DIRTY_SIZE], sx: u8, sy: u8) {
    let idx = (sy as usize) * (VIEW_W as usize) + (sx as usize);
    dirty[idx >> 3] |= BIT[idx & 7];
}

/// Mark a world-coordinate position dirty if it falls within the viewport.
fn mark_dirty_world(dirty: &mut [u8; DIRTY_SIZE], vx: u8, vy: u8, wx: u8, wy: u8) {
    if wx >= vx && wx < vx + VIEW_W && wy >= vy && wy < vy + VIEW_H {
        mark_dirty(dirty, wx - vx, wy - vy);
    }
}

/// Differential render: only redraw cells that changed since the last frame.
///
/// Assumes the viewport has NOT scrolled and the depth has NOT changed —
/// the caller handles those cases separately via `render_viewport_scroll()`
/// or `render_all()`. Computes a dirty bitfield from FOV/entity/item
/// changes and redraws only those cells via `restore_tile()`.
pub fn render_diff(state: &MicroGameState, prev: &DiffState, vx: u8, vy: u8) {
    let mut dirty = [0u8; DIRTY_SIZE];

    // --- 1. FOV visibility changes ---
    // XOR old and new visible bitfields; differing bits indicate tiles
    // that gained or lost visibility and need redrawing.
    let vis = state.fov.visible_bytes();
    let map_w = state.fov.width as usize;
    let fov_bytes_used = ((map_w * (state.fov.height as usize) + 7) / 8).min(MAX_BITFIELD_SIZE);
    for byte_idx in 0..fov_bytes_used {
        let diff = prev.fov_visible[byte_idx] ^ vis[byte_idx];
        if diff == 0 {
            continue;
        }
        for bit in 0..8u8 {
            if diff & (BIT[bit as usize]) == 0 {
                continue;
            }
            let tile_idx = byte_idx * 8 + (bit as usize);
            // Use shift/mask for the common width=64 case to avoid
            // __udivhi3 (~100+ cycles per 16-bit divide on 6502).
            let (wy, wx) = if map_w == 64 {
                ((tile_idx >> 6) as u8, (tile_idx & 63) as u8)
            } else {
                ((tile_idx / map_w) as u8, (tile_idx % map_w) as u8)
            };
            mark_dirty_world(&mut dirty, vx, vy, wx, wy);
        }
    }

    // --- 2. Entity position changes ---
    // Mark old positions of entities that moved, died, or were removed.
    let prev_ec = (prev.entity_count as usize).min(MAX_ENTITIES);
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
    for i in 0..(state.entities.count as usize).min(MAX_ENTITIES) {
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
    let prev_ic = (prev.item_count as usize).min(MAX_ITEMS);
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
    for i in 0..(state.items.count as usize).min(MAX_ITEMS) {
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
    // Track sx/sy with running counters to avoid __udivhi3 (16-bit divide
    // by 40 per cell).  Skip-8 on zero bytes advances counters cheaply.
    let mut sx: u8 = 0;
    let mut sy: u8 = 0;
    for byte_idx in 0..DIRTY_SIZE {
        if dirty[byte_idx] == 0 {
            sx += 8;
            if sx >= VIEW_W {
                sx -= VIEW_W;
                sy += 1;
            }
            if sy >= VIEW_H { break; }
            continue;
        }
        for bit in 0..8u8 {
            if sy >= VIEW_H { break; }
            if dirty[byte_idx] & (BIT[bit as usize]) != 0 {
                restore_tile(state, vx, vy, sx + vx, sy + vy);
            }
            sx += 1;
            if sx >= VIEW_W {
                sx = 0;
                sy += 1;
            }
        }
    }

    // --- 5. Status bar and messages (always, cheap) ---
    render_status_bar(state);
    render_messages(state);
}

/// Erase the old player glyph and draw the new one instantly.
/// Called before the background render so the player sees immediate
/// feedback with no ghost at the old position.
pub fn draw_player_immediate(state: &MicroGameState, prev: &DiffState, vx: u8, vy: u8) {
    // Erase old position
    if prev.entity_count > 0 {
        let ox = prev.entity_x[PLAYER_IDX as usize];
        let oy = prev.entity_y[PLAYER_IDX as usize];
        if ox >= vx && ox < vx + VIEW_W && oy >= vy && oy < vy + VIEW_H {
            let (sc, color) = tile_appearance(state, ox, oy);
            c64::draw_sc(ox - vx, oy - vy, sc, color);
        }
    }
    // Draw new position
    let px = state.entities.x[PLAYER_IDX as usize];
    let py = state.entities.y[PLAYER_IDX as usize];
    if px >= vx && px < vx + VIEW_W && py >= vy && py < vy + VIEW_H {
        c64::draw_char(px - vx, py - vy, balance::PLAYER_GLYPH as u8, c64::COLOR_YELLOW);
    }
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
    c64::draw_char(sx, sy, b'X', c64::COLOR_YELLOW);

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
    c64::draw_sc(sx, sy, sc, color);
    let visible = state.fov.is_visible(wx, wy);

    // 2. Item layer (only if visible)
    if visible {
        for i in 0..(state.items.count as usize).min(MAX_ITEMS) {
            if state.items.alive[i] && state.items.x[i] == wx && state.items.y[i] == wy {
                let kind = state.items.kind[i];
                let glyph = items::glyph(kind) as u8;
                let c = game_color_to_c64(items::color(kind));
                c64::draw_char(sx, sy, glyph, c);
                break;
            }
        }

        // 3. Entity layer (alive entities + corpses occlude items)
        for i in 0..(state.entities.count).min(MAX_ENTITIES as u8) {
            let idx = i as usize;
            if state.entities.x[idx] != wx || state.entities.y[idx] != wy {
                continue;
            }
            let (glyph, c) = if !state.entities.alive[idx] {
                if i == PLAYER_IDX { continue; }
                (SC_CORPSE, c64::COLOR_BROWN)
            } else if i == PLAYER_IDX {
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
            c64::draw_char(sx, sy, glyph, c);
            break;
        }
    }
}

/// Draw the look cursor (yellow 'X') at world position (cx, cy).
pub fn draw_cursor(vx: u8, vy: u8, cx: u8, cy: u8) {
    let sx = cx - vx;
    let sy = cy - vy;
    c64::draw_char(sx, sy, b'X', c64::COLOR_YELLOW);
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
            // Entity or item on this tile
            let eidx = state.entities.entity_at(cx, cy);
            if eidx != NO_ENTITY {
                if p < 40 { buf[p] = b' '; p += 1; }
                if eidx == PLAYER_IDX {
                    p = copy_bytes(&mut buf, p, b"Player");
                } else if let Some(kind) = state.entities.kind[eidx as usize] {
                    p = copy_bytes(&mut buf, p, monster_table::name(kind).as_bytes());
                    let tier = health::health_tier(
                        state.entities.hp[eidx as usize],
                        state.entities.max_hp[eidx as usize],
                    );
                    let desc: &[u8] = match tier {
                        HealthTier::Healthy => b"",
                        HealthTier::Moderate => b" (damaged)",
                        HealthTier::Severe => b" (wounded)",
                        HealthTier::AlmostDead => b" (dying)",
                    };
                    p = copy_bytes(&mut buf, p, desc);
                }
            }

            for i in 0..(state.items.count as usize).min(MAX_ITEMS) {
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
        c64::draw_char(i, STATUS_ROW, buf[i as usize], c64::COLOR_CYAN);
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
                c64::draw_sc(col, row, SC_SPACE, c64::COLOR_BLACK);
            }
        }

        if is_selected {
            c64::draw_char(x, row, b'>', c64::COLOR_YELLOW);
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
            c64::draw_sc(x, y, SC_SPACE, c64::COLOR_BLACK);
        }
    }
}

// ---------------------------------------------------------------------------
// Game over, title, pause, and seed input screens
// ---------------------------------------------------------------------------

/// Shared end-of-game screen (death or victory).
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
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

    // Seed code: "{base36}-{W}x{H}" using only u16/u8 arithmetic.
    // Avoids pulling in u64 division builtins (~6 KB on 6502).
    c64::draw_text(bx + 2, by + 5, b"Seed: ", c64::COLOR_GREY);
    let mut col = bx + 8;
    // Base36-encode the u16 seed (max 4 digits: 0xFFFF = "1ekf")
    {
        const B36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut buf = [0u8; 4];
        let mut len: u8 = 0;
        let mut s = state.seed;
        if s == 0 {
            buf[0] = b'0';
            len = 1;
        } else {
            while s > 0 {
                buf[len as usize] = B36[(s % 36) as usize];
                s /= 36;
                len += 1;
            }
        }
        // Draw reversed
        for i in 0..len {
            c64::draw_char(col, by + 5, buf[(len - 1 - i) as usize], c64::COLOR_WHITE);
            col += 1;
        }
    }
    c64::draw_char(col, by + 5, b'-', c64::COLOR_GREY);
    col += 1;
    col += c64::draw_number(col, by + 5, state.map.width, c64::COLOR_WHITE);
    c64::draw_char(col, by + 5, b'x', c64::COLOR_GREY);
    col += 1;
    c64::draw_number(col, by + 5, state.map.height, c64::COLOR_WHITE);

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
pub fn render_title(selected: u8, has_save: bool) {
    c64::clear_screen();

    c64::draw_text(11, 4, b"R O G U E L I K E", c64::COLOR_WHITE);

    if has_save {
        let menu_items: [&[u8]; 3] = [b"CONTINUE", b"NEW GAME", b"ENTER SEED"];
        draw_menu(&menu_items, selected, 10, 10);
    } else {
        let menu_items: [&[u8]; 2] = [b"NEW GAME", b"ENTER SEED"];
        draw_menu(&menu_items, selected, 10, 10);
    }
}

/// Render "SAVING..." status overlay.
pub fn render_saving() {
    c64::draw_text(15, 12, b"SAVING...", c64::COLOR_YELLOW);
}

/// Render "LOADING..." status overlay.
pub fn render_loading_save() {
    c64::clear_screen();
    c64::draw_text(15, 12, b"LOADING...", c64::COLOR_LGREY);
}

/// Render a save/load error message. Waits for a keypress (caller handles).
pub fn render_save_error(msg: &[u8]) {
    let x = (40u8.saturating_sub(msg.len() as u8)) / 2;
    c64::draw_text(x, 14, msg, c64::COLOR_RED);
}

/// Inventory action bar actions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InvAction {
    Use,
    Equip,
    Unequip,
    Drop,
    Back,
}

/// All actions for consumable items.
const CONSUMABLE_ACTIONS: [InvAction; 3] = [InvAction::Use, InvAction::Drop, InvAction::Back];
/// All actions for equipment items in the bag (not yet equipped).
const EQUIPMENT_ACTIONS: [InvAction; 3] = [InvAction::Equip, InvAction::Drop, InvAction::Back];
/// Actions for currently equipped items.
const EQUIPPED_ACTIONS: [InvAction; 3] = [InvAction::Unequip, InvAction::Drop, InvAction::Back];

/// Get the action list for an item kind (in inventory bag).
pub fn actions_for_kind(kind: items::ItemKind) -> &'static [InvAction] {
    if items::is_consumable(kind) {
        &CONSUMABLE_ACTIONS
    } else {
        &EQUIPMENT_ACTIONS
    }
}

/// Get the action list for a currently equipped item.
pub fn actions_for_equipped() -> &'static [InvAction] {
    &EQUIPPED_ACTIONS
}

fn action_label(action: InvAction) -> &'static [u8] {
    match action {
        InvAction::Use => b"USE",
        InvAction::Equip => b"EQUIP",
        InvAction::Unequip => b"UNEQUIP",
        InvAction::Drop => b"DROP",
        InvAction::Back => b"BACK",
    }
}

/// Count selectable equipped items (0, 1, or 2).
pub fn equip_count(state: &MicroGameState) -> u8 {
    state.equipment.weapon.is_some() as u8 + state.equipment.armor.is_some() as u8
}

/// Render the inventory overlay on top of the game screen.
///
/// `selected` indexes into the combined list: equipped items (0..equip_count)
/// then inventory items (equip_count..).
/// `action_bar`: if Some, we're in Act mode — show the action bar with the
/// given actions and selected index. If None, show the keyboard hint.
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
pub fn render_inventory(
    state: &MicroGameState,
    selected: u8,
    action_bar: Option<(&[InvAction], u8)>,
) {
    render_all(state);

    let bx: u8 = 2;
    let by: u8 = 1;
    let bw: u8 = 36;
    let bh: u8 = 23;

    clear_rect(bx, by, bw, bh);
    c64::draw_text(bx + 2, by, b"INVENTORY", c64::COLOR_CYAN);

    let mut row: u8 = by + 2;
    let ec = equip_count(state);

    // Equipped items section — now selectable.
    if ec > 0 {
        c64::draw_text(bx + 2, row, b"EQUIPPED:", c64::COLOR_DGREY);
        row += 1;
        let mut equip_idx: u8 = 0;
        if let Some(kind) = state.equipment.weapon {
            let color = if selected == equip_idx {
                c64::COLOR_YELLOW
            } else {
                c64::COLOR_GREEN
            };
            c64::draw_text(bx + 3, row, b"W: ", color);
            c64::draw_text(bx + 6, row, items::name(kind).as_bytes(), color);
            row += 1;
            equip_idx += 1;
        }
        if let Some(kind) = state.equipment.armor {
            let color = if selected == equip_idx {
                c64::COLOR_YELLOW
            } else {
                c64::COLOR_GREEN
            };
            c64::draw_text(bx + 3, row, b"A: ", color);
            c64::draw_text(bx + 6, row, items::name(kind).as_bytes(), color);
            row += 1;
        }
        row += 1; // blank line separator
    }

    // Inventory bag items.
    let inv_selected = selected.wrapping_sub(ec);
    let mut item_count: u8 = 0;
    for (idx, (i, slot)) in state.inventory.iter().enumerate() {
        if row >= by + bh - 2 {
            break;
        }
        let letter = b'A' + i as u8;
        let col = bx + 2;
        let item_color = game_color_to_c64(items::color(slot.kind));
        let (label_color, name_color) = if idx as u8 == inv_selected {
            (c64::COLOR_YELLOW, c64::COLOR_YELLOW)
        } else {
            (c64::COLOR_LGREY, item_color)
        };
        c64::draw_char(col, row, letter, label_color);
        c64::draw_char(col + 1, row, b')', label_color);
        c64::draw_text(col + 3, row, items::name(slot.kind).as_bytes(), name_color);
        if slot.count > 1 {
            let name_len = items::name(slot.kind).len() as u8;
            let p = col + 3 + name_len + 1;
            c64::draw_char(p, row, b'X', c64::COLOR_GREY);
            c64::draw_number(p + 1, row, slot.count, c64::COLOR_GREY);
        }
        row += 1;
        item_count += 1;
    }

    if item_count == 0 && ec == 0 {
        c64::draw_text(bx + 2, row, b"EMPTY", c64::COLOR_DGREY);
    }

    // Bottom bar: action bar (Act mode) or keyboard hints (Browse mode).
    let bar_row = by + bh - 1;
    match action_bar {
        Some((actions, sel_action)) => {
            let mut x = bx + 2;
            for (i, &action) in actions.iter().enumerate() {
                let label = action_label(action);
                if i as u8 == sel_action {
                    // Selected action: highlighted with bracket markers
                    c64::draw_char(x, bar_row, b'[', c64::COLOR_YELLOW);
                    x += 1;
                    c64::draw_text(x, bar_row, label, c64::COLOR_YELLOW);
                    x += label.len() as u8;
                    c64::draw_char(x, bar_row, b']', c64::COLOR_YELLOW);
                    x += 2;
                } else {
                    c64::draw_text(x, bar_row, label, c64::COLOR_DGREY);
                    x += label.len() as u8 + 1;
                }
            }
        }
        None => {
            if selected < ec {
                c64::draw_text(bx + 2, bar_row, b"U:USE E:UNEQUIP D:DROP", c64::COLOR_DGREY);
            } else {
                c64::draw_text(bx + 2, bar_row, b"U:USE E:EQUIP D:DROP", c64::COLOR_DGREY);
            }
        }
    }
}

/// Render the pause menu overlay on top of the game screen.
pub fn render_pause(state: &MicroGameState, selected: u8) {
    render_all(state);

    let bx: u8 = 8;
    let by: u8 = 8;
    let bw: u8 = 24;
    let bh: u8 = 9;

    clear_rect(bx, by, bw, bh);
    c64::fill_row(by, 0xC0, c64::COLOR_CYAN);

    c64::draw_text(bx + 2, by + 1, b"PAUSED", c64::COLOR_CYAN);

    let menu_items: [&[u8]; 3] = [b"Resume", b"Save & Quit", b"Title Screen"];
    draw_menu(&menu_items, selected, bx + 4, by + 3);
}

/// Render the seed code text input dialog.
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
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
        c64::draw_char(field_x + i, field_y, b'_', c64::COLOR_DGREY);
    }

    for i in 0..len {
        let ch = buf[i as usize];
        let display = if ch >= b'a' && ch <= b'z' {
            ch - b'a' + b'A'
        } else {
            ch
        };
        c64::draw_char(field_x + i, field_y, display, c64::COLOR_WHITE);
    }

    if (len as u8) < field_w {
        c64::draw_sc(field_x + len, field_y, 0xA0, c64::COLOR_YELLOW);
    }
}

/// Render the loading screen shown during level generation.
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
pub fn render_loading() {
    c64::clear_screen();
    // "GENERATING..." = 13 chars, centered: x = (40-13)/2 ≈ 13
    c64::draw_text(13, 10, b"GENERATING...", c64::COLOR_LGREY);
}

/// Number of help pages.
pub const HELP_PAGES: u8 = 2;

/// Shared help header and footer. Called by each page function.
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
fn help_chrome(page: u8) {
    c64::clear_screen();
    c64::fill_row(0, 0xC0, c64::COLOR_CYAN);
    c64::draw_text(2, 0, b"HELP", c64::COLOR_BLACK);

    // Page indicator: "1/2" right-aligned in header
    c64::draw_char(35, 0, b'0' + page + 1, c64::COLOR_BLACK);
    c64::draw_char(36, 0, b'/', c64::COLOR_BLACK);
    c64::draw_char(37, 0, b'0' + HELP_PAGES, c64::COLOR_BLACK);

    // Footer: navigation hint
    c64::draw_text(2, 24, b"</>:PAGE  STOP:BACK", c64::COLOR_DGREY);
}

/// Help page 1: Controls.
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
fn help_page_controls() {
    help_chrome(0);

    c64::draw_text(2, 1, b"CONTROLS", c64::COLOR_WHITE);
    c64::draw_text(2, 3, b"WASD/ARROWS", c64::COLOR_LGREY);
    c64::draw_text(15, 3, b"MOVE / ATTACK", c64::COLOR_DGREY);
    c64::draw_text(2, 4, b"Q E Z C", c64::COLOR_LGREY);
    c64::draw_text(15, 4, b"DIAGONAL MOVE", c64::COLOR_DGREY);
    c64::draw_text(2, 5, b"SHIFT/JOY+DIR", c64::COLOR_LGREY);
    c64::draw_text(15, 5, b"AUTORUN", c64::COLOR_DGREY);
    c64::draw_text(2, 6, b"SPACE", c64::COLOR_LGREY);
    c64::draw_text(15, 6, b"WAIT ONE TURN", c64::COLOR_DGREY);
    c64::draw_text(2, 7, b"G", c64::COLOR_LGREY);
    c64::draw_text(15, 7, b"PICKUP ITEM", c64::COLOR_DGREY);
    c64::draw_text(2, 8, b"I", c64::COLOR_LGREY);
    c64::draw_text(15, 8, b"INVENTORY", c64::COLOR_DGREY);
    c64::draw_text(2, 9, b"X", c64::COLOR_LGREY);
    c64::draw_text(15, 9, b"LOOK MODE", c64::COLOR_DGREY);
    c64::draw_text(2, 10, b"P", c64::COLOR_LGREY);
    c64::draw_text(15, 10, b"MESSAGE LOG", c64::COLOR_DGREY);
    c64::draw_text(2, 11, b"RETURN", c64::COLOR_LGREY);
    c64::draw_text(15, 11, b"DESCEND STAIRS", c64::COLOR_DGREY);
    c64::draw_text(2, 12, b"RUN/STOP", c64::COLOR_LGREY);
    c64::draw_text(15, 12, b"PAUSE MENU", c64::COLOR_DGREY);
    c64::draw_text(2, 13, b"?", c64::COLOR_LGREY);
    c64::draw_text(15, 13, b"HELP", c64::COLOR_DGREY);
}

/// Format a monster stats row at compile time: "NAME        HP ATK DEF"
/// All stat values come from rules/ const fns — single source of truth.
/// Returns a fixed-width byte array suitable for draw_text (no runtime overhead).
const fn fmt_monster_row(name: &[u8], hp: u8, atk: u8, def: u8) -> [u8; 22] {
    let mut buf = [b' '; 22];
    // Name at position 0
    let mut i = 0;
    while i < name.len() && i < 10 {
        buf[i] = name[i];
        i += 1;
    }
    // HP at position 12 (right-aligned in 2 chars)
    if hp >= 10 { buf[12] = b'0' + hp / 10; }
    buf[13] = b'0' + hp % 10;
    // ATK at position 16
    if atk >= 10 { buf[16] = b'0' + atk / 10; }
    buf[17] = b'0' + atk % 10;
    // DEF at position 20
    if def >= 10 { buf[20] = b'0' + def / 10; }
    buf[21] = b'0' + def % 10;
    buf
}

/// Format an item effect string at compile time: "NAME            EFFECT N"
const fn fmt_item_row(name: &[u8], effect: &[u8], val: u8) -> [u8; 24] {
    let mut buf = [b' '; 24];
    let mut i = 0;
    while i < name.len() && i < 14 {
        buf[i] = name[i];
        i += 1;
    }
    // Effect label at position 16
    let mut j = 0;
    while j < effect.len() && 16 + j < 23 {
        buf[16 + j] = effect[j];
        j += 1;
    }
    // Value after effect label
    let pos = 16 + j;
    if val >= 10 { buf[pos] = b'0' + val / 10; buf[pos + 1] = b'0' + val % 10; }
    else { buf[pos] = b'0' + val; }
    buf
}

/// Help page 2: Combat + Monsters + Items.
/// Stats are baked from rules/ const fns at compile time — single source of truth,
/// zero runtime overhead (identical binary to hardcoded strings).
#[unsafe(link_section = ".hiramcode")]
#[inline(never)]
fn help_page_bestiary() {
    use monster_table::MonsterKind;

    // Compile-time stat rows — if balance changes, these update automatically.
    const GOBLIN_ROW: [u8; 22] = fmt_monster_row(b"GOBLIN",
        monster_table::max_hp(MonsterKind::Goblin),
        monster_table::attack(MonsterKind::Goblin),
        monster_table::defense(MonsterKind::Goblin));
    const ORC_ROW: [u8; 22] = fmt_monster_row(b"ORC",
        monster_table::max_hp(MonsterKind::Orc),
        monster_table::attack(MonsterKind::Orc),
        monster_table::defense(MonsterKind::Orc));
    const TROLL_ROW: [u8; 22] = fmt_monster_row(b"TROLL",
        monster_table::max_hp(MonsterKind::Troll),
        monster_table::attack(MonsterKind::Troll),
        monster_table::defense(MonsterKind::Troll));

    const POTION_ROW: [u8; 24] = fmt_item_row(b"HEALTH POTION",
        b"HEAL ", items::heal_amount(items::ItemKind::HealthPotion));
    const SWORD_ROW: [u8; 24] = fmt_item_row(b"SHORT SWORD",
        b"ATK +", items::attack_bonus(items::ItemKind::ShortSword));
    const ARMOR_ROW: [u8; 24] = fmt_item_row(b"LEATHER ARMOR",
        b"DEF +", items::defense_bonus(items::ItemKind::LeatherArmor));

    help_chrome(1);

    c64::draw_text(2, 1, b"COMBAT", c64::COLOR_WHITE);
    c64::draw_text(2, 2, b"DMG = ATK - DEF (MIN 0)", c64::COLOR_LGREY);
    c64::draw_text(2, 3, b"WALK INTO MONSTER TO ATTACK", c64::COLOR_LGREY);
    c64::draw_text(2, 4, b"HP REGEN: 1 EVERY 3 TURNS", c64::COLOR_LGREY);
    c64::draw_text(2, 5, b"REACH DEPTH 5 TO WIN", c64::COLOR_LGREY);

    c64::draw_text(2, 7, b"MONSTERS", c64::COLOR_WHITE);
    c64::draw_text(5, 8, b"NAME        HP ATK DEF", c64::COLOR_DGREY);

    c64::draw_char(2, 9, monster_table::glyph(MonsterKind::Goblin) as u8,
        game_color_to_c64(monster_table::color(MonsterKind::Goblin)));
    c64::draw_text(5, 9, &GOBLIN_ROW, c64::COLOR_LGREY);

    c64::draw_char(2, 10, monster_table::glyph(MonsterKind::Orc) as u8,
        game_color_to_c64(monster_table::color(MonsterKind::Orc)));
    c64::draw_text(5, 10, &ORC_ROW, c64::COLOR_LGREY);

    c64::draw_char(2, 11, monster_table::glyph(MonsterKind::Troll) as u8,
        game_color_to_c64(monster_table::color(MonsterKind::Troll)));
    c64::draw_text(5, 11, &TROLL_ROW, c64::COLOR_LGREY);

    c64::draw_text(2, 13, b"ITEMS", c64::COLOR_WHITE);
    c64::draw_text(5, 14, b"NAME            EFFECT", c64::COLOR_DGREY);

    c64::draw_char(2, 15, b'!', game_color_to_c64(items::color(items::ItemKind::HealthPotion)));
    c64::draw_text(5, 15, &POTION_ROW, c64::COLOR_LGREY);

    c64::draw_char(2, 16, b'/', game_color_to_c64(items::color(items::ItemKind::ShortSword)));
    c64::draw_text(5, 16, &SWORD_ROW, c64::COLOR_LGREY);

    c64::draw_char(2, 17, b'[', game_color_to_c64(items::color(items::ItemKind::LeatherArmor)));
    c64::draw_text(5, 17, &ARMOR_ROW, c64::COLOR_LGREY);
}

/// Render a help screen page. Called in a loop by the help page-flip handler.
pub fn render_help_page(page: u8) {
    match page {
        0 => help_page_controls(),
        _ => help_page_bestiary(),
    }
}

/// Render the message history overlay. Shows all stored messages
/// (oldest at top, newest at bottom). Dismissed by any key.
pub fn render_message_history(state: &MicroGameState) {
    c64::clear_screen();
    c64::fill_row(0, 0xC0, c64::COLOR_CYAN);
    c64::draw_text(2, 0, b"MESSAGE LOG", c64::COLOR_BLACK);

    let mut buf = [b' '; 40];

    // Count how many messages actually exist (log may not be full yet).
    let mut count: u8 = 0;
    while (count as usize) < MSG_COUNT {
        if state.log.recent(count).is_none() {
            break;
        }
        count += 1;
    }

    if count == 0 {
        c64::draw_text(2, 2, b"NO MESSAGES YET", c64::COLOR_DGREY);
    } else {
        // Display messages oldest-first, starting at row 2.
        // recent(count-1) is oldest, recent(0) is newest.
        let mut row: u8 = 2;
        let mut i = count;
        while i > 0 {
            i -= 1;
            if let Some(event) = state.log.recent(i) {
                format_event(event, &mut buf);
                // Fade older messages, brightest for newest
                let color = if i == 0 {
                    c64::COLOR_WHITE
                } else if i <= 2 {
                    c64::COLOR_LGREY
                } else {
                    c64::COLOR_GREY
                };
                for col in 0..40u8 {
                    c64::draw_char(col, row, buf[col as usize], color);
                }
                row += 1;
            }
        }
    }

    // Footer hint
    c64::draw_text(2, 24, b"PRESS ANY KEY", c64::COLOR_DGREY);
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
