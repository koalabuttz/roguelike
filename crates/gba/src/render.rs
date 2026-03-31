//! Game state rendering for GBA.
//!
//! Renders any `GameView` implementor onto the two BG layers:
//! - BG0 (screenblock 30): map viewport with tiles, items, entities
//! - BG1 (screenblock 31): status bar + message log

use roguelike_core::rules::balance::PLAYER_GLYPH;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::rules::items as item_rules;
use roguelike_core::rules::monster_table;
use roguelike_core::rules::tiles as tile_rules;

use crate::display::{self, MAP_ROWS, MSG_ROW, SCREEN_COLS, STATUS_ROW};
use crate::format;
use crate::palette::{PALBANK_DIM, PALBANK_MSG, PALBANK_STATUS};

/// Viewport width in tiles.
const VP_W: i32 = SCREEN_COLS as i32;
/// Viewport height in tiles.
const VP_H: i32 = MAP_ROWS as i32;

/// Compute viewport origin (top-left world coordinate), player-centered.
fn viewport_origin(state: &impl GameView) -> (i32, i32) {
    let (px, py) = state.player_xy();
    let (mw, mh) = state.map_dims();
    let vx = (px - VP_W / 2).clamp(0, (mw - VP_W).max(0));
    let vy = (py - VP_H / 2).clamp(0, (mh - VP_H).max(0));
    (vx, vy)
}

/// Full screen redraw from game state.
pub fn render_game(state: &impl GameView) {
    let (vx, vy) = viewport_origin(state);
    render_viewport(state, vx, vy);
    render_items(state, vx, vy);
    render_entities(state, vx, vy);
    render_status_bar(state);
    render_messages(state);
}

/// Render map tiles on BG0, with FOV visibility and explored dimming.
fn render_viewport(state: &impl GameView, vx: i32, vy: i32) {
    let (mw, mh) = state.map_dims();
    for sy in 0..VP_H {
        for sx in 0..VP_W {
            let wx = vx + sx;
            let wy = vy + sy;

            if wx < 0 || wx >= mw || wy < 0 || wy >= mh {
                // Out of map bounds — black
                display::write_map_tile(sx as usize, sy as usize, b' ', 0);
                continue;
            }

            let visible = state.is_visible(wx, wy);
            let explored = state.is_explored(wx, wy);

            if !visible && !explored {
                // Never seen — black
                display::write_map_tile(sx as usize, sy as usize, b' ', 0);
                continue;
            }

            let tile_u8 = state.tile_at(wx, wy);
            let (glyph, palbank) = match tile_rules::from_micro(tile_u8) {
                Some(kind) => {
                    let g = tile_rules::glyph(kind) as u8;
                    // Non-structural walls render as blank space (invisible interior walls)
                    if tile_u8 == 0 {
                        (b' ', 0)
                    } else if visible {
                        (g, tile_rules::color(kind) as u16)
                    } else {
                        (g, PALBANK_DIM)
                    }
                }
                None => (b' ', 0),
            };

            display::write_map_tile(sx as usize, sy as usize, glyph, palbank);
        }
    }
}

/// Overlay visible ground items on BG0.
fn render_items(state: &impl GameView, vx: i32, vy: i32) {
    for i in 0..state.item_count() {
        if !state.item_alive(i) {
            continue;
        }
        let (ix, iy) = state.item_xy(i);

        if !state.is_visible(ix, iy) {
            continue;
        }

        let sx = ix - vx;
        let sy = iy - vy;
        if sx < 0 || sx >= VP_W || sy < 0 || sy >= VP_H {
            continue;
        }

        let kind = state.item_kind_at(i);
        let glyph = item_rules::glyph(kind) as u8;
        let palbank = item_rules::color(kind) as u16;
        display::write_map_tile(sx as usize, sy as usize, glyph, palbank);
    }
}

/// Overlay visible entities (player + monsters) on BG0.
fn render_entities(state: &impl GameView, vx: i32, vy: i32) {
    // Draw corpses first, then living entities on top.
    for pass in 0..2u8 {
        for i in 0..state.entity_count() {
            let alive = state.entity_alive(i);
            // Pass 0: dead entities. Pass 1: alive entities.
            if pass == 0 && alive {
                continue;
            }
            if pass == 1 && !alive {
                continue;
            }

            let (ex, ey) = state.entity_xy(i);

            if !state.is_visible(ex, ey) {
                continue;
            }

            let sx = ex - vx;
            let sy = ey - vy;
            if sx < 0 || sx >= VP_W || sy < 0 || sy >= VP_H {
                continue;
            }

            let (glyph, palbank) = if i == 0 {
                (PLAYER_GLYPH as u8, GameColor::Green as u16)
            } else if !alive {
                (b'%', GameColor::DarkRed as u16)
            } else {
                match state.entity_kind(i) {
                    Some(kind) => {
                        (monster_table::glyph(kind) as u8, monster_table::color(kind) as u16)
                    }
                    None => (b'?', GameColor::White as u16),
                }
            };

            display::write_map_tile(sx as usize, sy as usize, glyph, palbank);
        }
    }
}

/// Render status bar on BG1 row 17.
fn render_status_bar(state: &impl GameView) {
    // Fill with dark blue background
    for x in 0..SCREEN_COLS {
        display::write_hud_tile(x, STATUS_ROW, b' ', PALBANK_STATUS);
    }

    let mut buf = [b' '; 30];
    let mut p = 0;

    // HP:current/max
    let (hp, max_hp) = state.player_hp();
    p = format::write_str(&mut buf, p, "HP:");
    p = format::write_u16(&mut buf, p, hp as u16);
    buf[p] = b'/';
    p += 1;
    p = format::write_u16(&mut buf, p, max_hp as u16);

    // ATK
    p = format::write_str(&mut buf, p, " A:");
    p = format::write_u16(&mut buf, p, state.effective_attack() as u16);

    // DEF
    p = format::write_str(&mut buf, p, " D:");
    p = format::write_u16(&mut buf, p, state.effective_defense() as u16);

    // Depth
    p = format::write_str(&mut buf, p, " F:");
    let _ = format::write_u16(&mut buf, p, state.depth() as u16);

    display::write_hud_string(0, STATUS_ROW, core::str::from_utf8(&buf).unwrap_or(""), PALBANK_STATUS);
}

/// Render last 2 messages on BG1 rows 18-19.
fn render_messages(state: &impl GameView) {
    for row in 0..2usize {
        // Clear row
        for x in 0..SCREEN_COLS {
            display::write_hud_tile(x, MSG_ROW + row, b' ', PALBANK_MSG);
        }

        if let Some(event) = state.recent_message(row as u8) {
            let mut buf = [b' '; 30];
            format::format_event(event, &mut buf);
            display::write_hud_string(
                0,
                MSG_ROW + row,
                core::str::from_utf8(&buf).unwrap_or(""),
                PALBANK_MSG,
            );
        }
    }
}
