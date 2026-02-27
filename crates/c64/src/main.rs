// Roguelike Dungeon Crawler — Commodore 64 Edition
//
// Thin C64 frontend over roguelike-core::tier_micro. All game logic
// (map gen, FOV, entities, combat, AI, spawning, messages) comes from
// the shared core crate. This file handles hardware init, seed reading,
// and the main loop state machine.

#![no_std]
#![no_main]
#![allow(static_mut_refs)] // Single-threaded bare metal — static mut is safe

mod c64;
mod render;
mod input;

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use input::MenuInput;
use roguelike_core::command::GameCommand;
use roguelike_core::rules::seed_code;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::types::{DEFAULT_MAP_HEIGHT, DEFAULT_MAP_WIDTH};

/// Panic handler — flash the border red (classic C64 crash indicator).
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        c64::poke(c64::VIC_BORDER, c64::COLOR_RED);
    }
}

/// Game state stored in a static — too large for the 6502 hardware stack
/// (256 bytes) but fine in BSS. MaybeUninit avoids requiring Default.
static mut STATE: MaybeUninit<MicroGameState> = MaybeUninit::uninit();

/// Application states for the main loop.
enum AppState {
    Title,
    Playing,
    Paused,
    GameOver,
}

/// Read a 16-bit seed from the CIA1 timer (human timing jitter = entropy).
fn read_cia_seed() -> u16 {
    let lo = c64::peek(c64::CIA1_TIMER_LO) as u16;
    let hi = c64::peek(unsafe { c64::CIA1_TIMER_LO.add(1) }) as u16;
    lo | (hi << 8)
}

/// Start a new game with the given seed and dimensions.
/// Writes to the global STATE and returns a mutable reference.
fn start_game(seed: u16, width: u8, height: u8) -> &'static mut MicroGameState {
    unsafe {
        STATE.write(MicroGameState::new(seed, width, height));
        STATE.assume_init_mut()
    }
}

/// Run the title menu. Returns the seed and dimensions for the new game.
fn run_title() -> (u16, u8, u8) {
    let mut selected: u8 = 0;
    render::render_title(selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render::render_title(selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render::render_title(selected);
                }
            }
            MenuInput::Select => {
                match selected {
                    0 => {
                        // New Game — random seed, default dims
                        return (read_cia_seed(), DEFAULT_MAP_WIDTH, DEFAULT_MAP_HEIGHT);
                    }
                    1 => {
                        // Enter Seed
                        if let Some((seed, w, h)) = run_seed_input() {
                            return (seed, w, h);
                        }
                        // Cancelled — redraw title
                        render::render_title(selected);
                    }
                    _ => {}
                }
            }
            MenuInput::Back => {
                // No back action on title — nowhere to go
            }
        }
    }
}

/// Run the seed input dialog. Returns decoded params or None if cancelled.
fn run_seed_input() -> Option<(u16, u8, u8)> {
    let mut buf = [0u8; 12];

    render::render_seed_input(&[], 0);

    let len = input::read_seed_input(&mut buf, |typed, len| {
        render::render_seed_input(typed, len);
    })?;

    // Try to decode the seed code
    match seed_code::decode_micro_from_bytes(&buf[..len as usize]) {
        Ok(params) => Some((params.seed, params.width, params.height)),
        Err(_) => {
            render::render_seed_error(b"Bad code format");
            input::wait_for_menu_input(); // wait for any key
            None
        }
    }
}

/// Run the pause menu. Returns the next AppState.
fn run_pause(state: &MicroGameState) -> AppState {
    let mut selected: u8 = 0;
    render::render_pause(state, selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render::render_pause(state, selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render::render_pause(state, selected);
                }
            }
            MenuInput::Select => {
                return match selected {
                    0 => AppState::Playing, // Resume
                    _ => AppState::Title,   // New Game → go to title
                };
            }
            MenuInput::Back => {
                return AppState::Playing; // Back = Resume
            }
        }
    }
}

/// Run the game over menu. Returns the next AppState.
fn run_game_over(state: &MicroGameState) -> AppState {
    let mut selected: u8 = 0;
    render::render_game_over(state, selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render::render_game_over(state, selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render::render_game_over(state, selected);
                }
            }
            MenuInput::Select => {
                return match selected {
                    0 => AppState::Playing,  // Play Again
                    _ => AppState::Title,    // Title Screen
                };
            }
            MenuInput::Back => {} // No back from game over
        }
    }
}

#[no_mangle]
pub extern "C" fn main() -> isize {
    c64::init_hardware();

    let mut app_state = AppState::Title;
    // Track current game params for Play Again
    let mut current_seed: u16 = 0;
    let mut current_width: u8 = DEFAULT_MAP_WIDTH;
    let mut current_height: u8 = DEFAULT_MAP_HEIGHT;

    loop {
        match app_state {
            AppState::Title => {
                let (seed, w, h) = run_title();
                current_seed = seed;
                current_width = w;
                current_height = h;
                let state = start_game(seed, w, h);
                render::render_all(state);
                app_state = AppState::Playing;
            }
            AppState::Playing => {
                let state = unsafe { STATE.assume_init_mut() };

                if state.game_over {
                    render::render_all(state);
                    app_state = AppState::GameOver;
                    continue;
                }

                let cmd = input::wait_for_input();

                if cmd == GameCommand::Quit {
                    app_state = AppState::Paused;
                    continue;
                }

                state.step(cmd);
                render::render_all(state);

                if state.game_over {
                    app_state = AppState::GameOver;
                }
            }
            AppState::Paused => {
                let state = unsafe { STATE.assume_init_mut() };
                match run_pause(state) {
                    AppState::Playing => {
                        render::render_all(state);
                        app_state = AppState::Playing;
                    }
                    AppState::Title => {
                        app_state = AppState::Title;
                    }
                    other => app_state = other,
                }
            }
            AppState::GameOver => {
                let state = unsafe { STATE.assume_init_mut() };
                match run_game_over(state) {
                    AppState::Playing => {
                        // Play Again — new random seed, same dimensions
                        let seed = read_cia_seed();
                        current_seed = seed;
                        let state = start_game(seed, current_width, current_height);
                        render::render_all(state);
                        app_state = AppState::Playing;
                    }
                    AppState::Title => {
                        app_state = AppState::Title;
                    }
                    other => app_state = other,
                }
            }
        }
    }
}
