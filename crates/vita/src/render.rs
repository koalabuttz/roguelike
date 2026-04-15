//! Game state renderer for the PS Vita using vita2d.
//!
//! Phase 1 — colored rectangles:
//! Each tile, entity, and item is drawn as a solid filled rectangle
//! with a color derived from its `GameColor`. This proves the vita2d
//! rendering pipeline end-to-end without requiring a glyph atlas.
//!
//! Phase 4 will replace the rectangles with textured glyph blits from
//! an embedded bitmap font atlas, adding the quadratic FOV brightness
//! falloff and dirty-rect optimization described in docs/platforms/vita-port.md.
//!
//! # Layout (960×544 screen, 24×24 cell size)
//! ```text
//! Rows 0–19  (480 px) : map viewport, 40×20 cells
//! Row  20    ( 24 px) : status bar   (HP / ATK / DEF / Depth / Turns)
//! Row  21    ( 24 px) : message line (most recent event)
//! ```

use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::{GameView, TileVisibility};

use crate::vita2d::{self, rgba, Vita2d};

// ── Display constants ─────────────────────────────────────────────────────────

/// Pixel width of one character cell. At 24 px, gives 40 columns on 960 px.
pub const CELL_W: f32 = 24.0;
/// Pixel height of one character cell. At 24 px, gives 22 rows on 544 px.
pub const CELL_H: f32 = 24.0;

/// Number of tile columns in the map viewport.
pub const VP_COLS: i32 = 40;
/// Number of tile rows in the map viewport.
pub const VP_ROWS: i32 = 20;

/// Y pixel position of the status bar row (row 20).
const STATUS_Y: f32 = VP_ROWS as f32 * CELL_H; // 480.0
/// Y pixel position of the message row (row 21).
const MSG_Y: f32 = STATUS_Y + CELL_H; // 504.0

// ── Palette ───────────────────────────────────────────────────────────────────

/// Map a `GameColor` to an OLED-tuned vita2d RGBA color (fully opaque).
///
/// Values are tuned for the Vita's OLED panel: saturated foreground colors
/// against pure black (OLED pixels off), matching the design spec in
/// docs/platforms/vita-port.md §GameColor Mapping.
pub fn game_color_to_rgba(c: GameColor) -> u32 {
    match c {
        GameColor::Black => rgba(0, 0, 0, 255),
        GameColor::White => rgba(255, 255, 255, 255),
        GameColor::Grey => rgba(170, 170, 170, 255),
        GameColor::DarkGrey => rgba(85, 85, 85, 255),
        GameColor::Red => rgba(255, 68, 68, 255),
        GameColor::DarkRed => rgba(170, 0, 0, 255),
        GameColor::Green => rgba(68, 255, 68, 255),
        GameColor::DarkGreen => rgba(0, 170, 0, 255),
        GameColor::Yellow => rgba(255, 221, 0, 255),
        GameColor::DarkBlue => rgba(0, 0, 170, 255),
        GameColor::Cyan => rgba(68, 255, 255, 255),
        GameColor::Rgb(r, g, b) => rgba(r, g, b, 255),
    }
}

/// Dim a vita2d color to ~15% brightness (explored-but-not-visible tiles).
/// On OLED, 15% is genuinely dim — not the "dark grey on LCD backlight"
/// that terminal emulators produce.
/// Uses integer math: 39/256 ≈ 15.2%.
fn dim_color(color: u32) -> u32 {
    let r = ((color & 0xFF) * 39) >> 8;
    let g = (((color >> 8) & 0xFF) * 39) >> 8;
    let b = (((color >> 16) & 0xFF) * 39) >> 8;
    r | (g << 8) | (b << 16) | 0xFF000000
}

// ── Viewport ─────────────────────────────────────────────────────────────────

fn viewport_origin(state: &dyn GameView) -> (i32, i32) {
    state.viewport_origin(VP_COLS, VP_ROWS)
}

fn in_viewport(sx: i32, sy: i32) -> bool {
    (0..VP_COLS).contains(&sx) && (0..VP_ROWS).contains(&sy)
}

// ── Main render entry ─────────────────────────────────────────────────────────

/// Render a complete frame from game state.
///
/// Must be called between `vita2d.start_frame()` and `vita2d.end_frame()`.
pub fn render_frame(vita2d: &Vita2d, state: &dyn GameView) {
    let (vx, vy) = viewport_origin(state);
    render_viewport(vita2d, state, vx, vy);
    render_items(vita2d, state, vx, vy);
    render_entities(vita2d, state, vx, vy);
    render_status_bar(vita2d, state);
    render_message(vita2d, state);
}

// ── Map viewport ─────────────────────────────────────────────────────────────

fn render_viewport(vita2d: &Vita2d, state: &dyn GameView, vx: i32, vy: i32) {
    let (mw, mh) = state.map_dims();
    for sy in 0..VP_ROWS {
        for sx in 0..VP_COLS {
            let wx = vx + sx;
            let wy = vy + sy;
            let px = sx as f32 * CELL_W;
            let py = sy as f32 * CELL_H;

            if wx < 0 || wx >= mw || wy < 0 || wy >= mh {
                vita2d.draw_rect(px, py, CELL_W, CELL_H, vita2d::BLACK);
                continue;
            }

            let vis = state.tile_visibility(wx, wy);
            match vis {
                TileVisibility::Unexplored => {
                    // OLED: truly off pixels — draw nothing (background is
                    // already black from vita2d_clear_screen).
                    // Still draw a black rect to cover any previous frame
                    // artifacts in case dirty-rect optimization is added later.
                    vita2d.draw_rect(px, py, CELL_W, CELL_H, vita2d::BLACK);
                }
                TileVisibility::Explored => {
                    let (_, color) = state.render_tile(wx, wy);
                    let structural = state.tile_is_structural(wx, wy);
                    if structural {
                        vita2d.draw_rect(px, py, CELL_W, CELL_H, dim_color(game_color_to_rgba(color)));
                    } else {
                        vita2d.draw_rect(px, py, CELL_W, CELL_H, vita2d::BLACK);
                    }
                }
                TileVisibility::Visible => {
                    let (_, color) = state.render_tile(wx, wy);
                    vita2d.draw_rect(px, py, CELL_W, CELL_H, game_color_to_rgba(color));
                }
            }
        }
    }
}

// ── Items ─────────────────────────────────────────────────────────────────────

fn render_items(vita2d: &Vita2d, state: &dyn GameView, vx: i32, vy: i32) {
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
        if !in_viewport(sx, sy) {
            continue;
        }
        let (_, color) = state.render_item(i);
        // Items drawn as a smaller rect centered in the cell (16×16 out of 24×24).
        let px = sx as f32 * CELL_W + 4.0;
        let py = sy as f32 * CELL_H + 4.0;
        vita2d.draw_rect(px, py, CELL_W - 8.0, CELL_H - 8.0, game_color_to_rgba(color));
    }
}

// ── Entities ─────────────────────────────────────────────────────────────────

fn render_entities(vita2d: &Vita2d, state: &dyn GameView, vx: i32, vy: i32) {
    // Two passes: corpses (smaller dim rects) then living entities on top.
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
            let sx = ex - vx;
            let sy = ey - vy;
            if !in_viewport(sx, sy) {
                continue;
            }

            let (_, color) = state.render_entity(i);
            let rgba_color = game_color_to_rgba(color);

            if !alive {
                // Corpse: small dim rect
                let px = sx as f32 * CELL_W + 8.0;
                let py = sy as f32 * CELL_H + 8.0;
                vita2d.draw_rect(px, py, CELL_W - 16.0, CELL_H - 16.0, dim_color(rgba_color));
            } else {
                // Living entity: full cell rect
                let px = sx as f32 * CELL_W;
                let py = sy as f32 * CELL_H;
                vita2d.draw_rect(px, py, CELL_W, CELL_H, rgba_color);
            }
        }
    }
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(vita2d: &Vita2d, state: &dyn GameView) {
    // Dark blue background strip
    let bg = rgba(0, 0, 100, 255);
    vita2d.draw_rect(0.0, STATUS_Y, 960.0, CELL_H, bg);

    // HP bar: green fill proportional to hp/max_hp
    let (hp, max_hp) = state.player_hp();
    if max_hp > 0 {
        let bar_w = 120.0_f32;
        let hp_pct = hp as f32 / max_hp as f32;
        let fill_w = bar_w * hp_pct;
        let bar_color = if hp_pct > 0.6 {
            rgba(68, 255, 68, 255)
        } else if hp_pct > 0.3 {
            rgba(255, 221, 0, 255)
        } else {
            rgba(255, 68, 68, 255)
        };
        // HP bar background (dark)
        vita2d.draw_rect(4.0, STATUS_Y + 4.0, bar_w, CELL_H - 8.0, rgba(40, 40, 40, 255));
        // HP bar fill
        if fill_w > 0.0 {
            vita2d.draw_rect(4.0, STATUS_Y + 4.0, fill_w, CELL_H - 8.0, bar_color);
        }
    }

    // Stat indicators as colored dots/squares to the right of the HP bar.
    // Phase 2 will replace these with rendered text glyphs.
    let atk = state.effective_attack();
    let def = state.effective_defense();
    let depth = state.depth();

    // ATK indicator: orange squares proportional to attack value (capped at 10)
    let atk_capped = (atk as usize).min(10);
    for i in 0..atk_capped {
        vita2d.draw_rect(134.0 + i as f32 * 8.0, STATUS_Y + 4.0, 6.0, 7.0, rgba(255, 140, 0, 255));
    }

    // DEF indicator: blue squares (offset right of ATK)
    let def_capped = (def as usize).min(10);
    for i in 0..def_capped {
        vita2d.draw_rect(134.0 + i as f32 * 8.0, STATUS_Y + 13.0, 6.0, 7.0, rgba(68, 140, 255, 255));
    }

    // Depth indicator: a white dot per floor (capped at 10)
    let depth_capped = (depth as usize).min(10);
    for i in 0..depth_capped {
        vita2d.draw_rect(224.0 + i as f32 * 10.0, STATUS_Y + 8.0, 8.0, 8.0, rgba(200, 200, 200, 255));
    }
}

// ── Message line ──────────────────────────────────────────────────────────────

fn render_message(vita2d: &Vita2d, state: &dyn GameView) {
    // Black background
    vita2d.draw_rect(0.0, MSG_Y, 960.0, CELL_H, vita2d::BLACK);

    // If there's a recent event, render a colored indicator strip.
    // Phase 2 will replace this with rendered text via the glyph atlas.
    if state.recent_message(0).is_some() {
        // Cyan thin line at the bottom of the message row — indicates
        // something happened this turn. Simple but visible in Phase 1.
        vita2d.draw_rect(0.0, MSG_Y + CELL_H - 3.0, 960.0, 3.0, rgba(68, 255, 255, 128));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_color_reduces_brightness() {
        let bright = rgba(200, 200, 200, 255);
        let dimmed = dim_color(bright);
        // Each channel should be ~15% of original
        let r = (dimmed & 0xFF) as u8;
        let g = ((dimmed >> 8) & 0xFF) as u8;
        let b = ((dimmed >> 16) & 0xFF) as u8;
        assert!(r < 40, "r channel should be dim, got {r}");
        assert!(g < 40, "g channel should be dim, got {g}");
        assert!(b < 40, "b channel should be dim, got {b}");
        // Alpha should still be 255
        assert_eq!((dimmed >> 24) & 0xFF, 0xFF);
    }

    #[test]
    fn viewport_origin_clamps_to_zero() {
        // When the player is near (0,0), viewport origin should clamp to 0.
        // We test the math directly without a real GameView.
        let px = 5_i32;
        let py = 3_i32;
        let (mw, mh) = (80_i32, 40_i32);
        let vx = (px - VP_COLS / 2).clamp(0, (mw - VP_COLS).max(0));
        let vy = (py - VP_ROWS / 2).clamp(0, (mh - VP_ROWS).max(0));
        assert_eq!(vx, 0);
        assert_eq!(vy, 0);
    }

    #[test]
    fn viewport_origin_clamps_to_max() {
        let px = 79_i32;
        let py = 39_i32;
        let (mw, mh) = (80_i32, 40_i32);
        let vx = (px - VP_COLS / 2).clamp(0, (mw - VP_COLS).max(0));
        let vy = (py - VP_ROWS / 2).clamp(0, (mh - VP_ROWS).max(0));
        assert_eq!(vx, mw - VP_COLS);
        assert_eq!(vy, mh - VP_ROWS);
    }

    #[test]
    fn game_color_to_rgba_black_is_black() {
        assert_eq!(game_color_to_rgba(GameColor::Black), vita2d::BLACK);
    }

    #[test]
    fn game_color_to_rgba_white_is_white() {
        assert_eq!(game_color_to_rgba(GameColor::White), vita2d::WHITE);
    }
}
