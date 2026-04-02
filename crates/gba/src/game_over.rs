//! GBA game over / victory screen — overlay with stats, seed, and play-again menu.
//!
//! Death: dimmed game + red title + stats.
//! Victory: dimmed game + neon-animated green title + congratulations + stats.

use gba::prelude::*;

use roguelike_core::rules::game_view::GameView;

use crate::display;
use crate::format;
use crate::input::{self, MenuCommand};
use crate::menu;

/// Palbank for death title text.
const PALBANK_DEATH: u16 = 4; // Red

/// Palbank for victory title (animated neon — palbank 15).
const PALBANK_NEON: u16 = 15;

/// Palbank for stats text.
const PALBANK_STATS: u16 = 2; // Grey

/// Palbank for congratulatory text.
const PALBANK_CONGRATS: u16 = 2; // Grey

/// Palbank for selected menu item.
const PALBANK_SEL: u16 = 8; // Yellow

/// Palbank for unselected menu item.
const PALBANK_NORMAL: u16 = 2; // Grey

/// Sine LUT for neon animation (same as title_screen).
const SINE_LUT: [u8; 64] = [
    16, 17, 19, 21, 22, 24, 25, 27, 28, 29, 30, 30, 31, 31, 31, 31, 31, 31, 31, 30, 30, 29, 28,
    27, 25, 24, 22, 21, 19, 17, 16, 14, 12, 10, 9, 7, 6, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1,
    1, 2, 3, 4, 6, 7, 9, 10, 12, 14,
    // pad to 64 entries
    16, 16, 16, 16,
];
const NEON_PERIOD: u16 = 240;

const MENU_X: usize = 8;

/// What the player chose at game over.
pub enum GameOverAction {
    PlayAgain,
    TitleScreen,
}

/// Run the game over screen. Blocks until the player picks an action.
#[inline(never)]
pub fn run_game_over(state: &impl GameView) -> GameOverAction {
    let won = state.game_won();

    input::flush();
    menu::enable_dim_pct(87);
    display::clear_hud();

    // Render static content
    if won {
        render_victory(state);
    } else {
        render_death(state);
    }

    // Render menu items
    let items = ["Play Again", "Title Screen"];
    let mut selected: u8 = 0;
    let mut frame: u16 = 0;
    let mut needs_redraw = true;

    crate::cursor::init();

    loop {
        display::vblank_wait();
        frame = frame.wrapping_add(1);

        // Victory neon animation
        if won {
            animate_neon(frame);
        }

        if needs_redraw {
            render_menu_items(&items, selected);
            needs_redraw = false;
        }

        let cursor_row = 10 + selected as usize * 2;
        crate::cursor::update(MENU_X, cursor_row, frame, 0);

        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Up => {
                    if selected > 0 {
                        selected -= 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Down => {
                    if selected < 1 {
                        selected += 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Select => {
                    break;
                }
                MenuCommand::Back | MenuCommand::Start => {
                    selected = 1; // Title Screen
                    break;
                }
            }
        }
    }

    // Cleanup
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();
    menu::disable_dim();
    display::clear_hud();

    match selected {
        0 => GameOverAction::PlayAgain,
        _ => GameOverAction::TitleScreen,
    }
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

fn render_death(state: &impl GameView) {
    display::write_hud_centered(5, "You have been slain...", PALBANK_DEATH);
    render_stats(state, 7);
}

fn render_victory(state: &impl GameView) {
    // Setup neon palette for animated title
    setup_neon_palette();
    display::write_hud_centered(4, "You escaped!", PALBANK_NEON);
    display::write_hud_centered(5, "Well done, adventurer.", PALBANK_CONGRATS);
    render_stats(state, 7);
}

fn render_stats(state: &impl GameView, start_row: usize) {
    // Stats line: "Depth:N  Kills:N  Turns:N"
    let mut buf = [b' '; 30];
    let mut p = 0;
    p = format::write_str(&mut buf, p, "Depth:");
    p = format::write_u16(&mut buf, p, state.depth() as u16);
    p = format::write_str(&mut buf, p, "  Kills:");
    p = format::write_u16(&mut buf, p, state.kills() as u16);
    p = format::write_str(&mut buf, p, "  Turns:");
    let _ = format::write_u16(&mut buf, p, state.turn_count());

    let stats = core::str::from_utf8(&buf).unwrap_or("");
    display::write_hud_centered(start_row, stats.trim_end(), PALBANK_STATS);

    // Seed line
    let mut seed_buf = [b' '; 30];
    let sp = format::write_str(&mut seed_buf, 0, "Seed: ");
    let _ = write_seed_code(&mut seed_buf, sp, state);
    let seed_str = core::str::from_utf8(&seed_buf).unwrap_or("");
    display::write_hud_centered(start_row + 1, seed_str.trim_end(), PALBANK_STATS);
}

fn render_menu_items(items: &[&str; 2], selected: u8) {
    for (i, label) in items.iter().enumerate() {
        let row = 10 + i * 2;
        let pal = if i as u8 == selected {
            PALBANK_SEL
        } else {
            PALBANK_NORMAL
        };
        // Clear row in menu area
        for x in MENU_X..MENU_X + 20 {
            if x < display::SCREEN_COLS {
                display::write_hud_tile(x, row, b' ', 0);
            }
        }
        display::write_hud_string(MENU_X + 2, row, label, pal);
    }
}

// ---------------------------------------------------------------------------
// Seed encoding (u32 base36, avoids u64 division on ARM7)
// ---------------------------------------------------------------------------

const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn write_seed_code(buf: &mut [u8; 30], offset: usize, state: &impl GameView) -> usize {
    let seed = state.seed_u32();
    let (mw, mh) = state.map_dims();

    // Base36 encode the u32 seed
    let mut digits = [0u8; 7]; // max 7 base36 digits for u32
    let mut len = 0usize;
    let mut s = seed;
    if s == 0 {
        digits[0] = b'0';
        len = 1;
    } else {
        while s > 0 {
            digits[len] = BASE36[(s % 36) as usize];
            s /= 36;
            len += 1;
        }
    }

    // Write reversed (most significant first)
    let mut p = offset;
    for i in 0..len {
        if p < 30 {
            buf[p] = digits[len - 1 - i];
            p += 1;
        }
    }

    // Append dimensions: "-WxH"
    if p < 30 {
        buf[p] = b'-';
        p += 1;
    }
    p = format::write_u16(buf, p, mw as u16);
    if p < 30 {
        buf[p] = b'x';
        p += 1;
    }
    p = format::write_u16(buf, p, mh as u16);
    p
}

// ---------------------------------------------------------------------------
// Neon palette animation (for victory screen)
// ---------------------------------------------------------------------------

fn setup_neon_palette() {
    let pal = bg_palbank(PALBANK_NEON as usize);
    pal.index(0).write(Color::from_rgb(0, 0, 0));
    pal.index(1).write(Color::from_rgb(0, 31, 0)); // start green
}

fn animate_neon(frame: u16) {
    let idx = ((frame as u32 * 64 / NEON_PERIOD as u32) % 64) as usize;
    let t = SINE_LUT[idx] as u32;

    // Lerp between green (0,31,0) and cyan (0,25,31)
    let r = 0u32;
    let g = (31 * (31 - t) + 25 * t) / 31;
    let b = (31 * t) / 31;

    let color = Color::from_rgb(r.min(31) as u16, g.min(31) as u16, b.min(31) as u16);
    bg_palbank(PALBANK_NEON as usize).index(1).write(color);
}
