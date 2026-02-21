// Roguelike Dungeon Crawler — Commodore 64 Edition
// rust-mos proof of concept
//
// This is a ground-up reimplementation of the roguelike-core game logic
// in no_std Rust targeting the MOS 6510 via rust-mos/llvm-mos.
//
// POC goals:
//   1. Verify code size is within budget (< 16 KB for full game)
//   2. Verify screen rendering performance (< 1 frame for full redraw)
//   3. Verify indexed array access generates tight 6502 code
//   4. Verify the complete game loop works end-to-end

#![no_std]
#![no_main]

mod c64;
mod prng;
mod map;
mod fov;
mod entity;
mod combat;
mod ai;
mod render;
mod input;
mod msglog;

use core::panic::PanicInfo;

/// Panic handler — flash the border red (classic C64 crash indicator).
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        c64::poke(c64::VIC_BORDER, c64::COLOR_RED);
    }
}

// --- Game state ---

static mut TURN_COUNT: u16 = 0;
static mut KILLS: u8 = 0;
static mut GAME_OVER: bool = false;
static mut REGEN_INTERVAL: u8 = 3; // heal 1 HP every N turns

// --- Entry point ---

#[no_mangle]
pub extern "C" fn main() -> isize {
    c64::init_hardware();

    // Seed PRNG from CIA timer (human timing jitter provides entropy)
    let seed = c64::peek(c64::CIA1_TIMER_LO) as u16
        | ((c64::peek(unsafe { c64::CIA1_TIMER_LO.add(1) } as *const u8) as u16) << 8);
    prng::seed(if seed == 0 { 0xC64E } else { seed });

    // Title screen — wait for any keypress
    render::render_title();
    input::wait_any_key();

    // Initialize game
    new_game();

    // Main game loop
    loop {
        // Render current state
        render::render_all(unsafe { TURN_COUNT }, unsafe { KILLS });

        if unsafe { GAME_OVER } {
            render::render_game_over(
                unsafe { TURN_COUNT },
                unsafe { KILLS },
                prng::state(),
            );
            // Wait for any keypress, then restart
            input::wait_any_key();
            new_game();
            continue;
        }

        // Wait for player input
        let cmd = input::wait_for_input();

        // Process command
        let action_taken = handle_command(cmd);

        if action_taken {
            // Update FOV
            let px = entity::x(entity::PLAYER_IDX);
            let py = entity::y(entity::PLAYER_IDX);
            fov::compute_fov(px, py);

            // Run monster turns
            let player_died = ai::run_monster_turns();
            if player_died {
                unsafe { GAME_OVER = true; }
                msglog::add(b"You have died!");
            }

            // Increment turn counter
            unsafe { TURN_COUNT += 1; }

            // HP regeneration
            apply_regen();
        }
    }
}

/// Start a fresh game.
fn new_game() {
    unsafe {
        TURN_COUNT = 0;
        KILLS = 0;
        GAME_OVER = false;
    }

    entity::reset();
    msglog::reset();

    // Generate dungeon
    let (start_x, start_y) = map::generate();

    // Spawn player
    entity::spawn_player(start_x, start_y);

    // Spawn monsters in rooms
    entity::spawn_monsters();

    // Initial FOV
    fov::compute_fov(start_x, start_y);

    msglog::add(b"Welcome to the dungeon!");
}

/// Process a player command. Returns true if an action was taken (turn passes).
fn handle_command(cmd: u8) -> bool {
    match cmd {
        input::CMD_WAIT => true,
        input::CMD_MOVE_N | input::CMD_MOVE_S |
        input::CMD_MOVE_E | input::CMD_MOVE_W |
        input::CMD_MOVE_NE | input::CMD_MOVE_NW |
        input::CMD_MOVE_SE | input::CMD_MOVE_SW => {
            let (dx, dy) = input::cmd_delta(cmd);
            player_move_or_attack(dx, dy)
        }
        _ => false,
    }
}

/// Move the player by (dx, dy) or attack an entity in the way.
fn player_move_or_attack(dx: i8, dy: i8) -> bool {
    let px = entity::x(entity::PLAYER_IDX);
    let py = entity::y(entity::PLAYER_IDX);
    let nx = (px as i8 + dx) as u8;
    let ny = (py as i8 + dy) as u8;

    // Check for monster at target position
    let target = entity::monster_at(nx, ny);
    if target != entity::NO_ENTITY {
        let killed = combat::melee_attack(entity::PLAYER_IDX, target);
        if killed {
            unsafe { KILLS += 1; }
        }
        return true;
    }

    // Try to move
    if map::is_walkable(nx, ny) {
        entity::set_pos(entity::PLAYER_IDX, nx, ny);
        return true;
    }

    false
}

/// Heal 1 HP every regen_interval turns.
fn apply_regen() {
    unsafe {
        if GAME_OVER { return; }
        let hp = entity::hp(entity::PLAYER_IDX);
        let max_hp = entity::max_hp(entity::PLAYER_IDX);
        if hp < max_hp && TURN_COUNT % (REGEN_INTERVAL as u16) == 0 {
            entity::set_hp(entity::PLAYER_IDX, hp + 1);
        }
    }
}
