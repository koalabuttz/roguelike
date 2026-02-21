// Screen rendering — VIC-II character mode.
//
// Writes directly to screen memory ($0400) and color RAM ($D800).
// Uses the default C64 character set (uppercase/graphics mode).
//
// Layout:
//   Rows 0-21:  Map area (40x22 tiles)
//   Row 22:     Status bar (HP bar, kills, explored%)
//   Rows 23-24: Message log (2 most recent messages)
//
// For the POC we do a full redraw each frame. The production version
// would use dirty-rectangle tracking (compare to previous frame buffer,
// only update changed cells). At ~20 cycles per cell × 1000 cells =
// ~20,000 cycles = ~20ms. Visible on a profiler but under one frame.

use crate::c64;
use crate::map;
use crate::fov;
use crate::entity;
use crate::msglog;

// Screen codes for map tiles
const SC_SPACE: u8 = 0x20;      // space (unexplored)
const SC_FLOOR: u8 = 0x2E;      // . (floor)
const SC_WALL: u8 = 0xA0;       // reverse space = solid block (wall)
const SC_CORPSE: u8 = 0x25;     // % (dead entity)

const STATUS_ROW: u8 = 22;
const MSG_ROW: u8 = 23;

/// Full screen render: map + entities + status + messages.
pub fn render_all(turn: u16, kills: u8) {
    render_map();
    render_entities();
    render_status_bar(turn, kills);
    msglog::render(MSG_ROW);
}

/// Render the dungeon map tiles.
fn render_map() {
    for y in 0..map::MAP_H {
        for x in 0..map::MAP_W {
            let tile = map::tile_at(x, y);
            let visible = fov::is_visible(x, y);
            let explored = fov::is_explored(x, y);

            let (sc, color) = if visible {
                match tile {
                    map::TILE_FLOOR => (SC_FLOOR, c64::COLOR_DGREY),
                    map::TILE_WALL => {
                        if map::is_structural(x, y) {
                            (SC_WALL, c64::COLOR_LGREY)
                        } else {
                            (SC_SPACE, c64::COLOR_BLACK)
                        }
                    }
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else if explored {
                match tile {
                    map::TILE_FLOOR => (SC_FLOOR, c64::COLOR_BLUE),
                    map::TILE_WALL => {
                        if map::is_structural(x, y) {
                            (SC_WALL, c64::COLOR_BLUE)
                        } else {
                            (SC_SPACE, c64::COLOR_BLACK)
                        }
                    }
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else {
                (SC_SPACE, c64::COLOR_BLACK)
            };

            c64::draw_char(x, y, sc, color);
        }
    }
}

/// Render all alive entities that are in the player's FOV.
fn render_entities() {
    let count = entity::count();
    for i in 0..count {
        if !entity::is_alive(i) { continue; }
        let ex = entity::x(i);
        let ey = entity::y(i);
        if fov::is_visible(ex, ey) {
            c64::draw_char(ex, ey, entity::glyph(i), entity::color(i));
        }
    }
}

/// Render the status bar on row 22.
fn render_status_bar(turn: u16, kills: u8) {
    // Clear the row
    c64::fill_row(STATUS_ROW, SC_SPACE, c64::COLOR_BLACK);

    let hp = entity::hp(entity::PLAYER_IDX);
    let max_hp = entity::max_hp(entity::PLAYER_IDX);

    // "HP " label
    c64::draw_text(0, STATUS_ROW, b"HP ", c64::COLOR_WHITE);

    // HP bar: 12 chars wide, filled proportionally
    let bar_width: u8 = 12;
    let filled = if max_hp > 0 {
        ((hp as u16) * (bar_width as u16) / (max_hp as u16)) as u8
    } else { 0 };

    // Choose bar color based on health percentage
    let bar_color = if hp * 100 / max_hp > 60 {
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

    // Kills counter
    col += 2;
    c64::draw_text(col, STATUS_ROW, b"K:", c64::COLOR_GREY);
    col += 2;
    col += c64::draw_number(col, STATUS_ROW, kills, c64::COLOR_WHITE);

    // Turn counter (right-aligned)
    let _ = col; // suppress unused warning
    c64::draw_text(33, STATUS_ROW, b"T:", c64::COLOR_GREY);
    // Display turn as 16-bit: split into hi/lo
    let turn_lo = (turn % 256) as u8;
    let turn_hi = (turn / 256) as u8;
    if turn_hi > 0 {
        let w = c64::draw_number(35, STATUS_ROW, turn_hi, c64::COLOR_WHITE);
        // Pad low byte with leading zero if needed
        if turn_lo < 100 {
            c64::draw_char(35 + w, STATUS_ROW, c64::to_screen_code(b'0'), c64::COLOR_WHITE);
            if turn_lo < 10 {
                c64::draw_char(36 + w, STATUS_ROW, c64::to_screen_code(b'0'), c64::COLOR_WHITE);
                c64::draw_number(37 + w, STATUS_ROW, turn_lo, c64::COLOR_WHITE);
            } else {
                c64::draw_number(36 + w, STATUS_ROW, turn_lo, c64::COLOR_WHITE);
            }
        } else {
            c64::draw_number(35 + w, STATUS_ROW, turn_lo, c64::COLOR_WHITE);
        }
    } else {
        c64::draw_number(35, STATUS_ROW, turn_lo, c64::COLOR_WHITE);
    }
}

/// Render the game over screen overlay.
pub fn render_game_over(turn: u16, kills: u8, seed: u16) {
    // Draw a box in the center
    let bx: u8 = 8;
    let by: u8 = 8;
    let bw: u8 = 24;
    let bh: u8 = 7;

    for y in by..(by + bh) {
        for x in bx..(bx + bw) {
            c64::draw_char(x, y, SC_SPACE, c64::COLOR_BLACK);
        }
    }

    // Border (using + and - and |)
    for x in bx..(bx + bw) {
        c64::draw_char(x, by, 0xC0, c64::COLOR_RED);          // horizontal line
        c64::draw_char(x, by + bh - 1, 0xC0, c64::COLOR_RED);
    }
    for y in by..(by + bh) {
        c64::draw_char(bx, y, 0xDD, c64::COLOR_RED);            // vertical line
        c64::draw_char(bx + bw - 1, y, 0xDD, c64::COLOR_RED);
    }

    // Text
    c64::draw_text(bx + 5, by + 1, b"YOU HAVE DIED", c64::COLOR_RED);

    c64::draw_text(bx + 2, by + 3, b"Kills: ", c64::COLOR_GREY);
    c64::draw_number(bx + 9, by + 3, kills, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 4, b"Turns: ", c64::COLOR_GREY);
    // Simple 16-bit display for game over
    c64::draw_number(bx + 9, by + 4, (turn / 256) as u8, c64::COLOR_WHITE);
    c64::draw_number(bx + 12, by + 4, (turn % 256) as u8, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 5, b"Seed: ", c64::COLOR_GREY);
    // Display seed as hex
    draw_hex16(bx + 8, by + 5, seed, c64::COLOR_YELLOW);
}

/// Draw a 16-bit value as 4 hex digits.
fn draw_hex16(x: u8, y: u8, val: u16, color: u8) {
    let digits = [
        (val >> 12) as u8 & 0x0F,
        (val >> 8) as u8 & 0x0F,
        (val >> 4) as u8 & 0x0F,
        val as u8 & 0x0F,
    ];
    for (i, &d) in digits.iter().enumerate() {
        let ch = if d < 10 { b'0' + d } else { b'A' + d - 10 };
        c64::draw_char(x + i as u8, y, c64::to_screen_code(ch), color);
    }
}

/// Render a title screen. Returns when player presses a key.
pub fn render_title() {
    c64::clear_screen();

    // Simple PETSCII art title
    c64::draw_text(8, 3,  b"========================", c64::COLOR_YELLOW);
    c64::draw_text(8, 4,  b"   ROGUELIKE DUNGEON    ", c64::COLOR_WHITE);
    c64::draw_text(8, 5,  b"       CRAWLER          ", c64::COLOR_WHITE);
    c64::draw_text(8, 6,  b"========================", c64::COLOR_YELLOW);
    c64::draw_text(8, 8,  b"   RUST-MOS POC BUILD   ", c64::COLOR_LGREY);

    c64::draw_text(6, 12, b"CONTROLS:", c64::COLOR_CYAN);
    c64::draw_text(6, 14, b"WASD/ARROWS  MOVE", c64::COLOR_LGREY);
    c64::draw_text(6, 15, b"QEZC         DIAGONALS", c64::COLOR_LGREY);
    c64::draw_text(6, 16, b"SPACE        WAIT", c64::COLOR_LGREY);
    c64::draw_text(6, 17, b"JOYSTICK 2   ALSO WORKS", c64::COLOR_LGREY);

    c64::draw_text(6, 20, b"PRESS ANY KEY TO BEGIN", c64::COLOR_GREEN);
    c64::draw_text(6, 22, b"SEED: ", c64::COLOR_GREY);
    draw_hex16(12, 22, crate::prng::state(), c64::COLOR_YELLOW);
}
