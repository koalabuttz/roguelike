//! GBA game loop: state machine driving CompactGameState.
//!
//! States: Playing → Looking → GameOver → (restart)
//! Game state lives in EWRAM via static MaybeUninit.

use core::mem::MaybeUninit;

use gba::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::rules::color::GameColor;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH, PLAYER_IDX};

use crate::display;
use crate::input;
use crate::palette::PALBANK_HIGHLIGHT;
use crate::render;

/// Game state in EWRAM (too large for IWRAM stack).
#[link_section = ".ewram"]
static mut GAME: MaybeUninit<CompactGameState> = MaybeUninit::uninit();

enum AppState {
    Playing,
    Looking { cx: i32, cy: i32 },
    GameOver,
}

/// Wait for VBlank (scanline >= 160), then wait for it to end.
fn vblank_wait() {
    while VCOUNT.read() < 160 {}
    while VCOUNT.read() >= 160 {}
}

/// Start a new game directly in EWRAM, avoiding large stack allocation.
fn start_game(seed: u32) {
    let ptr = unsafe { (&raw mut GAME).cast::<CompactGameState>() };
    unsafe { CompactGameState::new_into(ptr, seed, MAP_WIDTH, MAP_HEIGHT) };
    crate::debug::debug_log!("Game initialized: seed={}, map={}x{}", seed, MAP_WIDTH, MAP_HEIGHT);
}

/// Get a reference to the active game state.
/// Safety: must only be called after start_game().
fn game() -> &'static mut CompactGameState {
    unsafe { &mut *(&raw mut GAME).cast::<CompactGameState>() }
}

/// Read the GBA's free-running timer as a seed source.
fn read_timer_seed() -> u32 {
    TIMER0_RELOAD.write(0);
    TIMER0_CONTROL.write(TimerControl::new().with_enabled(true));

    let lo = TIMER0_COUNT.read() as u32;
    vblank_wait();
    let hi = TIMER0_COUNT.read() as u32;

    // Combine into a non-zero 32-bit seed (LFSR requires non-zero)
    ((hi << 16) | lo) | 1
}

/// Wait until any key is newly pressed (rising edge detection).
fn wait_for_key() {
    let mut prev: u16 = 0;
    loop {
        vblank_wait();
        let pressed = !KEYINPUT.read().to_u16() & 0x03FF;
        let edges = pressed & !prev;
        prev = pressed;
        if edges != 0 {
            break;
        }
    }
}

/// Main entry point — runs forever.
pub fn run() -> ! {
    show_title();
    wait_for_key();

    display::write_map_string(8, 14, "Generating...", GameColor::DarkGrey as u16);
    let seed = read_timer_seed();
    start_game(seed);
    render::render_game(game());

    let mut app_state = AppState::Playing;

    loop {
        vblank_wait();

        #[cfg(feature = "dev")]
        if !crate::stack_check::check_canary() {
            crate::display::write_map_string(0, 0, "STACK OVERFLOW", 4);
            crate::debug::debug_log_fatal!("Stack canary corrupted — overflow detected");
            loop {}
        }

        match app_state {
            AppState::Playing => {
                let cmd = match input::read_game_input() {
                    Some(c) => c,
                    None => continue,
                };

                let state = game();

                // UI commands — don't step the game
                match cmd {
                    GameCommand::Look => {
                        let cx = state.entities.x[PLAYER_IDX as usize];
                        let cy = state.entities.y[PLAYER_IDX as usize];
                        app_state = AppState::Looking { cx, cy };
                        continue;
                    }
                    GameCommand::Quit => {
                        show_title();
                        wait_for_key();
                        let seed = read_timer_seed();
                        start_game(seed);
                        render::render_game(game());
                        app_state = AppState::Playing;
                        continue;
                    }
                    GameCommand::OpenInventory => {
                        crate::inventory_ui::run_inventory(game());
                        render::render_game(game());
                        continue;
                    }
                    _ => {}
                }

                let result = state.step(cmd);

                if !result.action_taken {
                    continue;
                }

                render::render_game(state);

                if state.game_over || state.game_won {
                    app_state = AppState::GameOver;
                }
            }

            AppState::Looking { ref mut cx, ref mut cy } => {
                match input::read_look_input() {
                    Some(input::LookCommand::Move(dir)) => {
                        let (dx, dy) = dir.to_offset();
                        let state = game();
                        let nx = (*cx + dx).clamp(0, state.map.width - 1);
                        let ny = (*cy + dy).clamp(0, state.map.height - 1);
                        *cx = nx;
                        *cy = ny;

                        render::render_game(state);
                        render_look_cursor(state, nx, ny);
                    }
                    Some(input::LookCommand::Close) => {
                        render::render_game(game());
                        app_state = AppState::Playing;
                    }
                    None => {}
                }
            }

            AppState::GameOver => {
                show_game_over(game());
                wait_for_key();

                show_title();
                wait_for_key();

                let seed = read_timer_seed();
                start_game(seed);
                render::render_game(game());
                app_state = AppState::Playing;
            }
        }
    }
}

/// Show the look cursor on the map layer.
fn render_look_cursor(state: &CompactGameState, wx: i32, wy: i32) {
    let px = state.entities.x[PLAYER_IDX as usize];
    let py = state.entities.y[PLAYER_IDX as usize];
    let vp_w = display::SCREEN_COLS as i32;
    let vp_h = display::MAP_ROWS as i32;
    let vx = (px - vp_w / 2).clamp(0, (state.map.width - vp_w).max(0));
    let vy = (py - vp_h / 2).clamp(0, (state.map.height - vp_h).max(0));

    let sx = wx - vx;
    let sy = wy - vy;
    if sx >= 0 && sx < vp_w && sy >= 0 && sy < vp_h {
        display::write_map_tile(sx as usize, sy as usize, b'X', PALBANK_HIGHLIGHT);
    }
}

/// Show a simple title screen.
fn show_title() {
    for y in 0..20 {
        for x in 0..30 {
            display::write_map_tile(x, y, b' ', 0);
            display::write_hud_tile(x, y, b' ', 0);
        }
    }

    display::write_map_string(8, 8, "ROGUELIKE", GameColor::White as u16);
    display::write_map_string(5, 11, "Press any key to start", GameColor::Grey as u16);
}

/// Show game over screen.
fn show_game_over(state: &CompactGameState) {
    let msg = if state.game_won {
        "You escaped!"
    } else {
        "You have been slain..."
    };
    let x = (30 - msg.len()) / 2;
    display::write_map_string(x, 8, msg, GameColor::Red as u16);

    let mut buf = [b' '; 30];
    let mut p = 0;
    p = crate::format::write_str(&mut buf, p, "Depth:");
    p = crate::format::write_u16(&mut buf, p, state.depth as u16);
    p = crate::format::write_str(&mut buf, p, " Kills:");
    p = crate::format::write_u16(&mut buf, p, state.kills as u16);
    p = crate::format::write_str(&mut buf, p, " Turns:");
    let _ = crate::format::write_u16(&mut buf, p, state.turn_count);

    let stats = core::str::from_utf8(&buf).unwrap_or("");
    let sx = (30usize).saturating_sub(stats.trim_end().len()) / 2;
    display::write_map_string(sx, 10, stats.trim_end(), GameColor::Grey as u16);

    display::write_map_string(4, 13, "Press any key to continue", GameColor::DarkGrey as u16);
}
