// Screen rendering — VIC-II character mode with viewport scrolling.
//
// Reads game state from &MicroGameState (roguelike-core::tier_micro).
// The 64x48 tile map is viewed through a 40x22 player-centered viewport.
//
// Layout:
//   Rows 0-21:  Map viewport (40x22 tiles from the 64x48 map)
//   Row 22:     Status bar (HP bar, kills, turns)
//   Rows 23-24: Message log (2 most recent GameEvents, formatted to PETSCII)

use crate::c64;
use roguelike_core::rules::balance;
use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::items;
use roguelike_core::rules::message::{GameEvent, SoundDistance};
use roguelike_core::rules::monster_table;
use roguelike_core::rules::seed_code::{self, MAX_MICRO_SEED_CODE_LEN};
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_WALL};
use roguelike_core::tier_micro::types::PLAYER_IDX;

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
fn viewport(state: &MicroGameState) -> (u8, u8) {
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

/// Full screen render: map + entities + status + messages.
pub fn render_all(state: &MicroGameState) {
    let (vx, vy) = viewport(state);
    render_map(state, vx, vy);
    render_entities(state, vx, vy);
    render_status_bar(state);
    render_messages(state);
}

/// Render the dungeon map tiles within the current viewport.
fn render_map(state: &MicroGameState, vx: u8, vy: u8) {
    for sy in 0..VIEW_H {
        for sx in 0..VIEW_W {
            let wx = sx + vx;
            let wy = sy + vy;
            let tile = state.map.tile_at(wx, wy);
            let visible = state.fov.is_visible(wx, wy);
            let explored = state.fov.is_explored(wx, wy);

            let (sc, color) = if visible {
                match tile {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_DGREY),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_CYAN),
                    TILE_WALL => {
                        if state.map.is_structural(wx, wy) {
                            (SC_WALL, c64::COLOR_LGREY)
                        } else {
                            (SC_SPACE, c64::COLOR_BLACK)
                        }
                    }
                    _ => (SC_SPACE, c64::COLOR_BLACK),
                }
            } else if explored {
                match tile {
                    TILE_FLOOR => (SC_FLOOR, c64::COLOR_BLUE),
                    TILE_STAIRS_DOWN => (SC_STAIRS, c64::COLOR_BLUE),
                    TILE_WALL => {
                        if state.map.is_structural(wx, wy) {
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

            c64::draw_char(sx, sy, sc, color);
        }
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
            let p = copy_bytes(buf, 0, b"The ");
            let p = copy_bytes(buf, p, who.name().as_bytes());
            copy_bytes(buf, p, b" notices you!")
        }
        GameEvent::DrinkPotion { kind: _, healed } => {
            let p = copy_bytes(buf, 0, b"Drank potion +");
            let p = copy_num(buf, p, healed);
            copy_bytes(buf, p, b" HP")
        }
        GameEvent::EquipWeapon { kind, bonus } => {
            let p = copy_bytes(buf, 0, b"Equip ");
            let p = copy_bytes(buf, p, items::name(kind).as_bytes());
            let p = copy_bytes(buf, p, b" +");
            let p = copy_num(buf, p, bonus);
            copy_bytes(buf, p, b" atk")
        }
        GameEvent::EquipArmor { kind, bonus } => {
            let p = copy_bytes(buf, 0, b"Equip ");
            let p = copy_bytes(buf, p, items::name(kind).as_bytes());
            let p = copy_bytes(buf, p, b" +");
            let p = copy_num(buf, p, bonus);
            copy_bytes(buf, p, b" def")
        }
        GameEvent::NoStairs => copy_bytes(buf, 0, b"No stairs here."),
        GameEvent::Descend { depth, target: _ } => {
            let p = copy_bytes(buf, 0, b"Descended to depth ");
            copy_num(buf, p, depth)
        }
        GameEvent::Victory { depth: _ } => copy_bytes(buf, 0, b"Victory!"),
        GameEvent::Welcome => copy_bytes(buf, 0, b"Welcome to the dungeon!"),
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

/// Draw a box border. Uses C64 PETSCII box-drawing characters.
fn draw_box(bx: u8, by: u8, bw: u8, bh: u8, border_color: u8) {
    // Clear box interior
    for y in by..(by + bh) {
        for x in bx..(bx + bw) {
            c64::draw_char(x, y, SC_SPACE, c64::COLOR_BLACK);
        }
    }

    // Horizontal borders
    for x in bx..(bx + bw) {
        c64::draw_char(x, by, 0xC0, border_color);
        c64::draw_char(x, by + bh - 1, 0xC0, border_color);
    }

    // Vertical borders
    for y in by..(by + bh) {
        c64::draw_char(bx, y, 0xDD, border_color);
        c64::draw_char(bx + bw - 1, y, 0xDD, border_color);
    }
}

// ---------------------------------------------------------------------------
// Game over, title, pause, and seed input screens
// ---------------------------------------------------------------------------

/// Draw a seed code (e.g. "16-64x48") at the given screen position.
fn draw_seed_code(x: u8, y: u8, seed: u16, width: u8, height: u8, color: u8) {
    let mut buf = [0u8; MAX_MICRO_SEED_CODE_LEN];
    let len = seed_code::encode_micro_to_buf(seed, width, height, &mut buf);
    for i in 0..len {
        c64::draw_char(x + i as u8, y, c64::to_screen_code(buf[i]), color);
    }
}

/// Render the game over screen overlay with menu items.
pub fn render_game_over(state: &MicroGameState, selected: u8) {
    let bx: u8 = 8;
    let by: u8 = 7;
    let bw: u8 = 24;
    let bh: u8 = 11;

    draw_box(bx, by, bw, bh, c64::COLOR_RED);

    c64::draw_text(bx + 5, by + 1, b"YOU HAVE DIED", c64::COLOR_RED);

    c64::draw_text(bx + 2, by + 3, b"Kills: ", c64::COLOR_GREY);
    c64::draw_number(bx + 9, by + 3, state.kills, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 4, b"Turns: ", c64::COLOR_GREY);
    c64::draw_number_u16(bx + 9, by + 4, state.turn_count, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 5, b"Seed: ", c64::COLOR_GREY);
    draw_seed_code(bx + 8, by + 5, state.seed, state.map.width, state.map.height, c64::COLOR_YELLOW);

    // Menu items inside the box
    let menu_items: [&[u8]; 2] = [b"Play Again", b"Title Screen"];
    draw_menu(&menu_items, selected, bx + 4, by + 7);
}

/// Render the victory screen overlay with menu items.
pub fn render_victory(state: &MicroGameState, selected: u8) {
    let bx: u8 = 8;
    let by: u8 = 7;
    let bw: u8 = 24;
    let bh: u8 = 11;

    draw_box(bx, by, bw, bh, c64::COLOR_GREEN);

    c64::draw_text(bx + 7, by + 1, b"VICTORY!", c64::COLOR_GREEN);

    c64::draw_text(bx + 2, by + 3, b"Kills: ", c64::COLOR_GREY);
    c64::draw_number(bx + 9, by + 3, state.kills, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 4, b"Turns: ", c64::COLOR_GREY);
    c64::draw_number_u16(bx + 9, by + 4, state.turn_count, c64::COLOR_WHITE);

    c64::draw_text(bx + 2, by + 5, b"Seed: ", c64::COLOR_GREY);
    draw_seed_code(bx + 8, by + 5, state.seed, state.map.width, state.map.height, c64::COLOR_YELLOW);

    // Menu items inside the box
    let menu_items: [&[u8]; 2] = [b"Play Again", b"Title Screen"];
    draw_menu(&menu_items, selected, bx + 4, by + 7);
}

/// Render the title screen with menu.
pub fn render_title(selected: u8) {
    c64::clear_screen();

    c64::draw_text(8, 3, b"========================", c64::COLOR_YELLOW);
    c64::draw_text(8, 4, b"   ROGUELIKE DUNGEON    ", c64::COLOR_WHITE);
    c64::draw_text(8, 5, b"       CRAWLER          ", c64::COLOR_WHITE);
    c64::draw_text(8, 6, b"========================", c64::COLOR_YELLOW);
    c64::draw_text(8, 8, b"    C64 + RUST-MOS      ", c64::COLOR_LGREY);

    // Menu items
    let menu_items: [&[u8]; 2] = [b"NEW GAME", b"ENTER SEED"];
    draw_menu(&menu_items, selected, 10, 11);

    // Condensed controls help
    c64::draw_text(4, 17, b"W/UP MOVE  Q/E DIAG", c64::COLOR_DGREY);
    c64::draw_text(4, 18, b"S/DN       Z/C", c64::COLOR_DGREY);
    c64::draw_text(4, 19, b"SPACE WAIT  RETURN DESCEND", c64::COLOR_DGREY);
    c64::draw_text(4, 20, b"JOY2 OK", c64::COLOR_DGREY);
}

/// Render the pause menu overlay on top of the game screen.
pub fn render_pause(state: &MicroGameState, selected: u8) {
    // Re-render the game underneath
    render_all(state);

    let bx: u8 = 8;
    let by: u8 = 8;
    let bw: u8 = 24;
    let bh: u8 = 8;

    draw_box(bx, by, bw, bh, c64::COLOR_CYAN);

    c64::draw_text(bx + 8, by + 1, b"PAUSED", c64::COLOR_CYAN);

    let menu_items: [&[u8]; 2] = [b"Resume", b"New Game"];
    draw_menu(&menu_items, selected, bx + 4, by + 3);
}

/// Render the seed code text input dialog.
pub fn render_seed_input(buf: &[u8], len: u8) {
    let bx: u8 = 5;
    let by: u8 = 9;
    let bw: u8 = 30;
    let bh: u8 = 7;

    draw_box(bx, by, bw, bh, c64::COLOR_CYAN);

    c64::draw_text(bx + 8, by + 1, b"ENTER SEED CODE", c64::COLOR_CYAN);

    // Input field background
    let field_x = bx + 3;
    let field_y = by + 3;
    let field_w: u8 = 16;
    for i in 0..field_w {
        c64::draw_char(field_x + i, field_y, c64::to_screen_code(b'_'), c64::COLOR_DGREY);
    }

    // Draw typed characters
    for i in 0..len {
        let ch = buf[i as usize];
        // Convert lowercase ASCII to uppercase for PETSCII display
        let display = if ch >= b'a' && ch <= b'z' {
            ch - b'a' + b'A'
        } else {
            ch
        };
        c64::draw_char(field_x + i, field_y, c64::to_screen_code(display), c64::COLOR_WHITE);
    }

    // Cursor
    if (len as u8) < field_w {
        c64::draw_char(field_x + len, field_y, 0xA0, c64::COLOR_YELLOW); // solid block cursor
    }

    c64::draw_text(bx + 3, by + 5, b"RETURN OK  RUN/STOP BACK", c64::COLOR_DGREY);
}

/// Render a brief error message overlay for invalid seed codes.
pub fn render_seed_error(msg: &[u8]) {
    let bx: u8 = 8;
    let by: u8 = 10;
    let bw: u8 = 24;
    let bh: u8 = 5;

    draw_box(bx, by, bw, bh, c64::COLOR_RED);

    c64::draw_text(bx + 7, by + 1, b"INVALID SEED", c64::COLOR_RED);
    // Center the error message (truncate if too long)
    let max_w = (bw - 4) as usize;
    let msg_len = if msg.len() > max_w { max_w } else { msg.len() };
    let msg_x = bx + 2 + ((max_w - msg_len) as u8) / 2;
    c64::draw_text(msg_x, by + 2, &msg[..msg_len], c64::COLOR_LGREY);
    c64::draw_text(bx + 4, by + 3, b"PRESS ANY KEY", c64::COLOR_DGREY);
}
