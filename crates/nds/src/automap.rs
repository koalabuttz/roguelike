//! 2D automap rendering on Engine B (bottom screen).
//!
//! Renders a terminal-identical top-down view of the dungeon with FOV
//! dimming, items, and entities. Uses the `GameView` trait for visual
//! parity with `crates/tui/src/render.rs`.
//!
//! ## Screen layout (32 columns x 24 rows)
//!
//! ```text
//! Rows  0-19: Automap viewport (32x20, centered on player)
//! Row  20:    Status bar (green)
//! Rows 21-23: Message log (3 lines, yellow)
//! ```

use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::{GameView, TileVisibility};

use crate::debug_hud;

/// Tile rows available for the map viewport.
pub(crate) const MAP_ROWS: usize = 20;

/// Tile columns (full screen width).
pub(crate) const MAP_COLS: usize = 32;

/// Palette bank for explored-but-not-visible tiles.
const PAL_DIM: u16 = 9;

/// Map GameColor to Engine B palette bank index.
fn game_color_to_pal(color: GameColor) -> u16 {
    match color {
        GameColor::White => 0,
        GameColor::Green => 1,
        GameColor::Yellow => 2,
        GameColor::DarkGrey => 3,
        GameColor::Cyan => 4,
        GameColor::Red => 5,
        GameColor::DarkGreen => 6,
        GameColor::Grey => 7,
        GameColor::DarkRed => 8,
        _ => 0, // fallback to white
    }
}

/// Compute the automap viewport origin in world coordinates.
///
/// Centers on the player, clamped to map edges so the viewport never
/// extends past the map boundary. Returns `(view_x, view_y)` in tile
/// coordinates — the world tile at the top-left corner of the viewport.
///
/// Thin wrapper over [`roguelike_core::rules::viewport::player_centered_i32`]
/// that narrows to `usize` for the automap's indexing conventions.
pub(crate) fn viewport_offset(px: i32, py: i32, map_w: i32, map_h: i32) -> (usize, usize) {
    let (vx, vy) = roguelike_core::rules::viewport::player_centered_i32(
        px,
        py,
        map_w,
        map_h,
        MAP_COLS as i32,
        MAP_ROWS as i32,
    );
    (vx as usize, vy as usize)
}

/// Render the full automap: terrain, items, entities.
///
/// Map tiles are drawn first, then items overlay visible cells, then
/// entities overlay everything (corpses first, living on top) — same
/// layering order as the terminal renderer.
pub fn render_automap(state: &impl GameView) {
    let (map_w, map_h) = state.map_dims();
    let (px, py) = state.player_xy();
    let (view_x, view_y) = viewport_offset(px, py, map_w, map_h);

    // Pass 1: Map tiles with FOV dimming.
    for screen_y in 0..MAP_ROWS {
        let world_y = (view_y + screen_y) as i32;
        for screen_x in 0..MAP_COLS {
            let world_x = (view_x + screen_x) as i32;

            if world_x >= map_w || world_y >= map_h {
                debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, 0, 0);
                continue;
            }

            let vis = state.tile_visibility(world_x, world_y);
            match vis {
                TileVisibility::Visible => {
                    let (glyph, fg) = state.render_tile(world_x, world_y);
                    let structural = state.tile_is_structural(world_x, world_y);
                    if glyph == '#' && !structural {
                        // Non-structural wall: black space (same as terminal)
                        debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, 0, 0);
                    } else {
                        let tile = debug_hud::ascii_to_tile(glyph as u8);
                        let pal = game_color_to_pal(fg);
                        debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, tile, pal);
                    }
                }
                TileVisibility::Explored => {
                    let (glyph, _fg) = state.render_tile(world_x, world_y);
                    let structural = state.tile_is_structural(world_x, world_y);
                    if glyph == '#' && !structural {
                        debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, 0, 0);
                    } else {
                        let tile = debug_hud::ascii_to_tile(glyph as u8);
                        debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, tile, PAL_DIM);
                    }
                }
                TileVisibility::Unexplored => {
                    debug_hud::write_tile_pal(screen_x as u8, screen_y as u8, 0, 0);
                }
            }
        }
    }

    // Pass 2: Items (visible only, drawn over map tiles).
    for i in 0..state.item_count() {
        if !state.item_alive(i) {
            continue;
        }
        let (ix, iy) = state.item_xy(i);
        if !state.is_visible(ix, iy) {
            continue;
        }
        let sx = (ix as usize).wrapping_sub(view_x);
        let sy = (iy as usize).wrapping_sub(view_y);
        if sx < MAP_COLS && sy < MAP_ROWS {
            let (glyph, fg) = state.render_item(i);
            let tile = debug_hud::ascii_to_tile(glyph as u8);
            let pal = game_color_to_pal(fg);
            debug_hud::write_tile_pal(sx as u8, sy as u8, tile, pal);
        }
    }

    // Pass 3: Entities — corpses first, then living (last writer wins).
    for pass in 0..2u8 {
        for i in 0..state.entity_count() {
            let alive = state.entity_alive(i);
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
            let sx = (ex as usize).wrapping_sub(view_x);
            let sy = (ey as usize).wrapping_sub(view_y);
            if sx < MAP_COLS && sy < MAP_ROWS {
                let (glyph, fg) = state.render_entity(i);
                let tile = debug_hud::ascii_to_tile(glyph as u8);
                let pal = game_color_to_pal(fg);
                debug_hud::write_tile_pal(sx as u8, sy as u8, tile, pal);
            }
        }
    }
}
