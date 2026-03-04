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
use input::{LookInput, MenuInput};
use roguelike_core::command::{Direction, GameCommand};
use roguelike_core::rules::message::{Combatant, GameEvent};
use roguelike_core::rules::{balance, seed_code};
use roguelike_core::tier_micro::autorun::{MicroAutorunStop, MicroAutorunStepper, MicroStepOutcome};
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::map::TILE_STAIRS_DOWN;
use roguelike_core::tier_micro::types::{DEFAULT_MAP_HEIGHT, DEFAULT_MAP_WIDTH, PLAYER_IDX};

/// Panic handler — flash the border red (classic C64 crash indicator).
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        c64::poke(c64::VIC_BORDER, c64::COLOR_RED);
    }
}

/// Game state stored in a static — too large for the 6502 hardware stack
/// (256 bytes) but fine in main RAM. MaybeUninit avoids requiring Default.
/// Explicit link_section keeps it in main ram so the linker can overflow
/// smaller compiler-generated statics to the freed KERNAL region.
#[link_section = ".noinit.state"]
static mut STATE: MaybeUninit<MicroGameState> = MaybeUninit::uninit();

/// Previous-frame snapshot for differential rendering (~810 bytes).
/// Placed in freed KERNAL region alongside STATE.
#[link_section = ".noinit.state"]
static mut DIFF: render::DiffState = render::DiffState::new();

/// Application states for the main loop.
enum AppState {
    Title,
    Playing,
    Looking,
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
            render::render_seed_error();
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

/// Run the end-of-game screen (death or victory). Returns the next AppState.
fn run_end_screen(state: &MicroGameState) -> AppState {
    let mut selected: u8 = 0;
    let render = |s: &MicroGameState, sel: u8| {
        if s.game_won {
            render::render_victory(s, sel);
        } else {
            render::render_game_over(s, sel);
        }
    };
    render(state, selected);

    loop {
        match input::wait_for_menu_input() {
            MenuInput::Up => {
                if selected > 0 {
                    selected -= 1;
                    render(state, selected);
                }
            }
            MenuInput::Down => {
                if selected < 1 {
                    selected += 1;
                    render(state, selected);
                }
            }
            MenuInput::Select => {
                return match selected {
                    0 => AppState::Playing,  // Play Again
                    _ => AppState::Title,    // Title Screen
                };
            }
            MenuInput::Back => {} // No back from end screen
        }
    }
}

/// Combat events detected this turn, used to select screen shake + SFX.
struct CombatInfo {
    /// Player attacked or killed a monster.
    player_attacked: bool,
    /// Player was hit by a monster (or took damage).
    player_hurt: bool,
}

/// Scan events added this turn for combat involving the player.
/// `old_total` is the log total from before `step()`.
fn detect_combat(state: &MicroGameState, old_total: u16) -> CombatInfo {
    let new_events = state.log.total().wrapping_sub(old_total);
    let limit = if new_events > 8 { 8 } else { new_events as u8 };
    let mut info = CombatInfo { player_attacked: false, player_hurt: false };
    let mut i: u8 = 0;
    while i < limit {
        match state.log.recent(i) {
            Some(GameEvent::Attack { attacker: Combatant::Player, .. })
            | Some(GameEvent::Kill { attacker: Combatant::Player, .. })
            | Some(GameEvent::NoDamage { attacker: Combatant::Player, .. }) => {
                info.player_attacked = true;
            }
            Some(GameEvent::Attack { defender: Combatant::Player, .. })
            | Some(GameEvent::NoDamage { defender: Combatant::Player, .. })
            | Some(GameEvent::Kill { victim: Combatant::Player, .. }) => {
                info.player_hurt = true;
            }
            _ => {}
        }
        i += 1;
    }
    info
}

/// (dx, dy) offsets indexed by Direction discriminant. i8 for sign,
/// applied via u8 checked arithmetic — no widening to i32.
const DIR_OFFSETS: [(i8, i8); 8] = [
    ( 0, -1), // North     = 0
    ( 0,  1), // South     = 1
    ( 1,  0), // East      = 2
    (-1,  0), // West      = 3
    ( 1, -1), // NorthEast = 4
    (-1, -1), // NorthWest = 5
    ( 1,  1), // SouthEast = 6
    (-1,  1), // SouthWest = 7
];

/// Apply a signed offset to a u8 coordinate, clamping to [0, max).
fn apply_offset(pos: u8, delta: i8, max: u8) -> u8 {
    if delta > 0 {
        if pos + 1 < max { pos + 1 } else { pos }
    } else if delta < 0 {
        if pos > 0 { pos - 1 } else { pos }
    } else {
        pos
    }
}

/// Run look mode: move a cursor around the map to examine tiles.
/// Viewport follows the cursor. Does not consume game turns.
/// Uses differential rendering — only redraws the old/new cursor tiles
/// and the status bar, unless the viewport scrolls.
fn run_look_mode(state: &MicroGameState) {
    let pi = PLAYER_IDX as usize;
    let mut cx = state.entities.x[pi];
    let mut cy = state.entities.y[pi];

    // Initial full render
    let mut vx: u8;
    let mut vy: u8;
    (vx, vy) = render::look_viewport(state, cx, cy);
    render::render_look(state, vx, vy, cx, cy);

    loop {
        match input::wait_for_look_input() {
            LookInput::Move(dir) => {
                let (dx, dy) = DIR_OFFSETS[dir as usize];
                let nx = apply_offset(cx, dx, state.map.width);
                let ny = apply_offset(cy, dy, state.map.height);
                if nx == cx && ny == cy {
                    continue;
                }

                let (nvx, nvy) = render::look_viewport(state, nx, ny);
                if nvx != vx || nvy != vy {
                    // Viewport scrolled — full redraw
                    vx = nvx;
                    vy = nvy;
                    cx = nx;
                    cy = ny;
                    render::render_look(state, vx, vy, cx, cy);
                } else {
                    // Same viewport — differential update
                    render::restore_tile(state, vx, vy, cx, cy);
                    cx = nx;
                    cy = ny;
                    render::draw_cursor(vx, vy, cx, cy);
                    render::render_look_status(state, cx, cy);
                }
            }
            LookInput::Close => return,
        }
    }
}

/// Run autorun: skip to destination instantly, then render once.
/// Combat SFX/shake fires if the final step involved combat.
fn run_autorun(state: &mut MicroGameState, dir: Direction) {
    // Immediate feedback: show "Running..." via the game log.
    state.log.add(GameEvent::Autorun);
    render::render_messages(state);

    let mut stepper = MicroAutorunStepper::new(dir);
    let mut last_msg_total;

    let stop_reason = loop {
        last_msg_total = state.log.total();
        match stepper.next_step(state) {
            MicroStepOutcome::Continue => continue,
            MicroStepOutcome::Done(reason) => break reason,
        }
    };

    // Log why autorun stopped (unless combat/death events already explain it).
    match stop_reason {
        MicroAutorunStop::DamageTaken | MicroAutorunStop::GameOver => {}
        reason => {
            state.log.add(GameEvent::AutorunStop {
                cause: reason.to_cause(),
            });
        }
    }

    // Ensure FOV is current (last step may have skipped it).
    let pi = PLAYER_IDX as usize;
    state
        .fov
        .compute_fov(state.entities.x[pi], state.entities.y[pi], &state.map);

    // Combat feedback for the final step.
    let combat = detect_combat(state, last_msg_total);
    if combat.player_attacked || combat.player_hurt {
        c64::shake_start();
    }
    if combat.player_attacked {
        c64::sfx_attack();
    }
    if combat.player_hurt {
        c64::sfx_hurt();
    }
}

#[no_mangle]
pub extern "C" fn main() -> isize {
    c64::init_hardware();

    let mut app_state = AppState::Title;
    let mut current_width: u8 = DEFAULT_MAP_WIDTH;
    let mut current_height: u8 = DEFAULT_MAP_HEIGHT;

    loop {
        match app_state {
            AppState::Title => {
                let (seed, w, h) = run_title();
                current_width = w;
                current_height = h;
                render::render_loading();
                c64::spinner_start();
                let state = start_game(seed, w, h);
                c64::spinner_stop();
                render::render_all(state);
                let diff = unsafe { &mut DIFF };
                diff.snapshot(state, render::viewport_pos(state));
                c64::music_start();
                app_state = AppState::Playing;
            }
            AppState::Playing => {
                let state = unsafe { STATE.assume_init_mut() };

                if state.is_terminal() {
                    c64::music_stop();
                    render::render_all(state);
                    app_state = AppState::GameOver;
                    continue;
                }

                let cmd = input::wait_for_input();

                if cmd == GameCommand::Quit {
                    c64::music_stop();
                    app_state = AppState::Paused;
                    continue;
                }

                if cmd == GameCommand::Look {
                    app_state = AppState::Looking;
                    continue;
                }

                if let GameCommand::Autorun(dir) = cmd {
                    run_autorun(state, dir);
                    // Full re-render after autorun to ensure clean state.
                    let diff = unsafe { &mut DIFF };
                    render::render_all(state);
                    diff.snapshot(state, render::viewport_pos(state));
                    if state.is_terminal() {
                        c64::music_stop();
                        app_state = AppState::GameOver;
                    }
                    continue;
                }

                // Show loading spinner during descent (map generation is slow)
                let will_generate = cmd == GameCommand::Descend
                    && state.depth < balance::TARGET_DEPTH
                    && {
                        let pi = PLAYER_IDX as usize;
                        state.map.tile_at(state.entities.x[pi], state.entities.y[pi])
                            == TILE_STAIRS_DOWN
                    };
                if will_generate {
                    c64::music_fade_for_descent();
                    render::render_loading();
                    c64::spinner_start();
                    c64::sfx_descent();
                }

                let old_depth = state.depth;
                let msg_total = state.log.total();
                let result = state.step(cmd);

                if will_generate {
                    c64::spinner_stop();
                    c64::music_resume();
                }

                if !result.action_taken {
                    continue; // nothing changed, skip rendering
                }

                // Combat feedback: screen shake + SID sound effects.
                // IRQ-driven shake runs asynchronously during rendering.
                let combat = detect_combat(state, msg_total);
                if combat.player_attacked || combat.player_hurt {
                    c64::shake_start();
                }
                if combat.player_attacked {
                    c64::sfx_attack();
                }
                if combat.player_hurt {
                    c64::sfx_hurt();
                }

                let diff = unsafe { &mut DIFF };
                if state.depth != old_depth {
                    // Descent — full redraw (entire level changed)
                    let vp = render::viewport_pos(state);
                    render::render_all(state);
                    diff.snapshot(state, vp);
                } else {
                    // Dead-zone viewport: only scroll when near edge
                    let (old_vx, old_vy) = diff.viewport;
                    let (vx, vy) = render::viewport_pos_lazy(state, old_vx, old_vy);

                    if (vx, vy) != (old_vx, old_vy) {
                        // Viewport scrolled — memory-copy or sparse fallback
                        render::render_viewport_scroll(state, diff, vx, vy, old_vx, old_vy);
                    } else {
                        // No scroll — player-first then dirty-cell diff
                        render::draw_player_immediate(state, diff, vx, vy);
                        render::render_diff(state, diff, vx, vy);
                    }
                    diff.snapshot(state, (vx, vy));
                }

                if state.is_terminal() {
                    c64::music_stop();
                    app_state = AppState::GameOver;
                }
            }
            AppState::Looking => {
                let state = unsafe { STATE.assume_init_mut() };
                run_look_mode(state);
                render::render_all(state);
                let diff = unsafe { &mut DIFF };
                diff.snapshot(state, render::viewport_pos(state));
                app_state = AppState::Playing;
            }
            AppState::Paused => {
                let state = unsafe { STATE.assume_init_mut() };
                match run_pause(state) {
                    AppState::Playing => {
                        render::render_all(state);
                        let diff = unsafe { &mut DIFF };
                        diff.snapshot(state, render::viewport_pos(state));
                        c64::music_start();
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
                match run_end_screen(state) {
                    AppState::Playing => {
                        // Play Again — new random seed, same dimensions
                        let seed = read_cia_seed();
                        render::render_loading();
                        c64::spinner_start();
                        let state = start_game(seed, current_width, current_height);
                        c64::spinner_stop();
                        render::render_all(state);
                        let diff = unsafe { &mut DIFF };
                        diff.snapshot(state, render::viewport_pos(state));
                        c64::music_start();
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
