//! Game state renderer for Android using a software pixel buffer.
//!
//! Phase 1 — colored rectangles:
//! Each tile, entity, and item is drawn as a solid filled rectangle
//! with a color derived from its `GameColor`. This proves the softbuffer
//! rendering pipeline end-to-end without requiring a glyph atlas.
//!
//! Phase 2 will replace the rectangles with 8x8 bitmap glyphs from
//! `roguelike-renderer3d::font`, scaled to cell size.
//!
//! # Layout (dynamic cell size from window dimensions)
//! ```text
//! Rows 0–19          : map viewport, VP_COLS × VP_ROWS cells
//! Row  VP_ROWS       : status bar (HP bar + stat indicators)
//! Row  VP_ROWS + 1   : message line (event indicator strip)
//! ```

use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::{GameView, TileVisibility};

// ── Display constants ─────────────────────────────────────────────────────────

/// Number of tile columns in the map viewport.
pub const VP_COLS: u32 = 40;
/// Number of tile rows in the map viewport.
pub const VP_ROWS: u32 = 20;
/// Total rows including status bar and message line.
const TOTAL_ROWS: u32 = VP_ROWS + 2;

// ── Palette ───────────────────────────────────────────────────────────────────

/// Map a `GameColor` to a softbuffer pixel (0x00RRGGBB, no alpha).
/// Tuned for LCD panels (most Android devices).
pub fn game_color_to_pixel(c: GameColor) -> u32 {
    match c {
        GameColor::Black => 0x000000,
        GameColor::White => 0xFFFFFF,
        GameColor::Grey => 0xAAAAAA,
        GameColor::DarkGrey => 0x555555,
        GameColor::Red => 0xFF4444,
        GameColor::DarkRed => 0xAA0000,
        GameColor::Green => 0x44FF44,
        GameColor::DarkGreen => 0x00AA00,
        GameColor::Yellow => 0xFFDD00,
        GameColor::DarkBlue => 0x0000AA,
        GameColor::Cyan => 0x44FFFF,
        GameColor::Rgb(r, g, b) => (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}

/// Dim a pixel to ~15% brightness (explored-but-not-visible tiles).
/// Uses integer math: 39/256 ~ 15.2%.
fn dim_pixel(pixel: u32) -> u32 {
    let r = (((pixel >> 16) & 0xFF) * 39) >> 8;
    let g = (((pixel >> 8) & 0xFF) * 39) >> 8;
    let b = ((pixel & 0xFF) * 39) >> 8;
    r << 16 | g << 8 | b
}

// ── Drawing primitives ───────────────────────────────────────────────────────

/// Fill a rectangle in the pixel buffer.
fn fill_rect(buf: &mut [u32], stride: u32, x: u32, y: u32, w: u32, h: u32, color: u32) {
    for row in y..y.saturating_add(h) {
        let start = (row * stride + x) as usize;
        let end = (row * stride + x.saturating_add(w)) as usize;
        if end <= buf.len() {
            buf[start..end].fill(color);
        }
    }
}

// ── Main render entry ─────────────────────────────────────────────────────────

/// Render a complete frame from game state into a pixel buffer.
///
/// `buf` is `width * height` pixels in 0x00RRGGBB format (softbuffer).
pub fn render_frame(buf: &mut [u32], width: u32, height: u32, state: &dyn GameView) {
    // Clear to black.
    buf.fill(0);

    let cell_w = width / VP_COLS;
    let cell_h = height / TOTAL_ROWS;
    if cell_w == 0 || cell_h == 0 {
        return;
    }

    let (vx, vy) = state.viewport_origin(VP_COLS as i32, VP_ROWS as i32);

    render_viewport(buf, width, cell_w, cell_h, state, vx, vy);
    render_items(buf, width, cell_w, cell_h, state, vx, vy);
    render_entities(buf, width, cell_w, cell_h, state, vx, vy);
    render_status_bar(buf, width, cell_w, cell_h, state);
    render_message(buf, width, cell_w, cell_h, state);
}

// ── Map viewport ─────────────────────────────────────────────────────────────

fn render_viewport(
    buf: &mut [u32],
    stride: u32,
    cell_w: u32,
    cell_h: u32,
    state: &dyn GameView,
    vx: i32,
    vy: i32,
) {
    let (mw, mh) = state.map_dims();
    for sy in 0..VP_ROWS {
        for sx in 0..VP_COLS {
            let wx = vx + sx as i32;
            let wy = vy + sy as i32;
            let px = sx * cell_w;
            let py = sy * cell_h;

            if wx < 0 || wx >= mw || wy < 0 || wy >= mh {
                // Out of map bounds — leave black.
                continue;
            }

            match state.tile_visibility(wx, wy) {
                TileVisibility::Unexplored => {
                    // Leave black.
                }
                TileVisibility::Explored => {
                    let (_, color) = state.render_tile(wx, wy);
                    if state.tile_is_structural(wx, wy) {
                        fill_rect(
                            buf,
                            stride,
                            px,
                            py,
                            cell_w,
                            cell_h,
                            dim_pixel(game_color_to_pixel(color)),
                        );
                    }
                    // Non-structural explored tiles stay black (floor fades away).
                }
                TileVisibility::Visible => {
                    let (_, color) = state.render_tile(wx, wy);
                    fill_rect(
                        buf,
                        stride,
                        px,
                        py,
                        cell_w,
                        cell_h,
                        game_color_to_pixel(color),
                    );
                }
            }
        }
    }
}

// ── Items ─────────────────────────────────────────────────────────────────────

fn render_items(
    buf: &mut [u32],
    stride: u32,
    cell_w: u32,
    cell_h: u32,
    state: &dyn GameView,
    vx: i32,
    vy: i32,
) {
    let inset_x = cell_w / 6;
    let inset_y = cell_h / 6;
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
        if sx < 0 || sx >= VP_COLS as i32 || sy < 0 || sy >= VP_ROWS as i32 {
            continue;
        }
        let (_, color) = state.render_item(i);
        let px = sx as u32 * cell_w + inset_x;
        let py = sy as u32 * cell_h + inset_y;
        fill_rect(
            buf,
            stride,
            px,
            py,
            cell_w - inset_x * 2,
            cell_h - inset_y * 2,
            game_color_to_pixel(color),
        );
    }
}

// ── Entities ─────────────────────────────────────────────────────────────────

fn render_entities(
    buf: &mut [u32],
    stride: u32,
    cell_w: u32,
    cell_h: u32,
    state: &dyn GameView,
    vx: i32,
    vy: i32,
) {
    // Two passes: corpses (small dim), then living entities on top.
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
            if sx < 0 || sx >= VP_COLS as i32 || sy < 0 || sy >= VP_ROWS as i32 {
                continue;
            }

            let (_, color) = state.render_entity(i);
            let pixel = game_color_to_pixel(color);

            if !alive {
                // Corpse: small dim rect.
                let inset = cell_w / 3;
                fill_rect(
                    buf,
                    stride,
                    sx as u32 * cell_w + inset,
                    sy as u32 * cell_h + inset,
                    cell_w - inset * 2,
                    cell_h - inset * 2,
                    dim_pixel(pixel),
                );
            } else {
                // Living entity: full cell.
                fill_rect(
                    buf,
                    stride,
                    sx as u32 * cell_w,
                    sy as u32 * cell_h,
                    cell_w,
                    cell_h,
                    pixel,
                );
            }
        }
    }
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(buf: &mut [u32], stride: u32, cell_w: u32, cell_h: u32, state: &dyn GameView) {
    let bar_y = VP_ROWS * cell_h;

    // Dark blue background strip.
    fill_rect(buf, stride, 0, bar_y, stride, cell_h, 0x000064);

    // HP bar: green/yellow/red fill proportional to hp/max_hp.
    let (hp, max_hp) = state.player_hp();
    if max_hp > 0 {
        let bar_w = cell_w * 5; // 5 cells wide
        let hp_pct = hp as u32 * 100 / max_hp as u32;
        let fill_w = bar_w * hp as u32 / max_hp as u32;
        let bar_color = if hp_pct > 60 {
            0x44FF44
        } else if hp_pct > 30 {
            0xFFDD00
        } else {
            0xFF4444
        };
        // HP bar background.
        let pad = cell_h / 6;
        fill_rect(
            buf,
            stride,
            pad,
            bar_y + pad,
            bar_w,
            cell_h - pad * 2,
            0x282828,
        );
        // HP bar fill.
        if fill_w > 0 {
            fill_rect(
                buf,
                stride,
                pad,
                bar_y + pad,
                fill_w,
                cell_h - pad * 2,
                bar_color,
            );
        }
    }

    // ATK indicator: orange squares.
    let atk = state.effective_attack().min(10) as u32;
    let indicator_x = cell_w * 6;
    let dot_w = cell_w / 3;
    let dot_gap = dot_w + dot_w / 2;
    let pad = cell_h / 6;
    for i in 0..atk {
        fill_rect(
            buf,
            stride,
            indicator_x + i * dot_gap,
            bar_y + pad,
            dot_w,
            cell_h / 2 - pad,
            0xFF8C00,
        );
    }

    // DEF indicator: blue squares.
    let def = state.effective_defense().min(10) as u32;
    for i in 0..def {
        fill_rect(
            buf,
            stride,
            indicator_x + i * dot_gap,
            bar_y + cell_h / 2,
            dot_w,
            cell_h / 2 - pad,
            0x448CFF,
        );
    }

    // Depth indicator: white dots.
    let depth = state.depth().min(10) as u32;
    let depth_x = indicator_x + 11 * dot_gap;
    for i in 0..depth {
        fill_rect(
            buf,
            stride,
            depth_x + i * dot_gap,
            bar_y + pad,
            dot_w,
            dot_w,
            0xC8C8C8,
        );
    }
}

// ── Message line ──────────────────────────────────────────────────────────────

fn render_message(buf: &mut [u32], stride: u32, _cell_w: u32, cell_h: u32, state: &dyn GameView) {
    let msg_y = (VP_ROWS + 1) * cell_h;

    // If there's a recent event, render a colored indicator strip.
    // Phase 2 will replace this with rendered text via the glyph atlas.
    if state.recent_message(0).is_some() {
        // Cyan thin line at bottom of message row — indicates something happened.
        let line_h = (cell_h / 8).max(1);
        fill_rect(
            buf,
            stride,
            0,
            msg_y + cell_h - line_h,
            stride,
            line_h,
            0x44FFFF,
        );
    }
}

// ── Coordinate helpers (used by input module) ────────────────────────────────

/// Convert pixel coordinates to viewport tile coordinates.
/// Returns `None` if the pixel is outside the map viewport.
pub fn pixel_to_tile(
    pixel_x: f64,
    pixel_y: f64,
    window_w: u32,
    window_h: u32,
) -> Option<(u32, u32)> {
    let cell_w = window_w / VP_COLS;
    let cell_h = window_h / TOTAL_ROWS;
    if cell_w == 0 || cell_h == 0 {
        return None;
    }

    let tx = pixel_x as u32 / cell_w;
    let ty = pixel_y as u32 / cell_h;

    if tx < VP_COLS && ty < VP_ROWS {
        Some((tx, ty))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_color_black_is_zero() {
        assert_eq!(game_color_to_pixel(GameColor::Black), 0x000000);
    }

    #[test]
    fn game_color_white_is_ffffff() {
        assert_eq!(game_color_to_pixel(GameColor::White), 0xFFFFFF);
    }

    #[test]
    fn game_color_rgb_packs_correctly() {
        assert_eq!(
            game_color_to_pixel(GameColor::Rgb(0x12, 0x34, 0x56)),
            0x123456
        );
    }

    #[test]
    fn dim_pixel_reduces_brightness() {
        let bright = 0xC8C8C8; // (200, 200, 200)
        let dimmed = dim_pixel(bright);
        let r = (dimmed >> 16) & 0xFF;
        let g = (dimmed >> 8) & 0xFF;
        let b = dimmed & 0xFF;
        assert!(r < 40, "r channel should be dim, got {r}");
        assert!(g < 40, "g channel should be dim, got {g}");
        assert!(b < 40, "b channel should be dim, got {b}");
    }

    #[test]
    fn pixel_to_tile_in_viewport() {
        // 800x440 window → cell_w=20, cell_h=20 (440/22=20)
        let result = pixel_to_tile(50.0, 30.0, 800, 440);
        assert_eq!(result, Some((2, 1))); // 50/20=2, 30/20=1
    }

    #[test]
    fn pixel_to_tile_outside_viewport() {
        // Pixel in the status bar area (row 20+).
        let result = pixel_to_tile(50.0, 410.0, 800, 440);
        assert_eq!(result, None); // 410/20=20 >= VP_ROWS
    }

    #[test]
    fn pixel_to_tile_zero_window() {
        assert_eq!(pixel_to_tile(0.0, 0.0, 0, 0), None);
    }

    #[test]
    fn fill_rect_stays_in_bounds() {
        let mut buf = vec![0u32; 100]; // 10x10
        fill_rect(&mut buf, 10, 8, 8, 5, 5, 0xFF);
        // Should fill only the in-bounds portion without panic.
        assert_eq!(buf[88], 0xFF); // (8,8)
        assert_eq!(buf[89], 0xFF); // (9,8)
    }
}
