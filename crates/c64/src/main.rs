// Roguelike Dungeon Crawler — Commodore 64 Edition
//
// Thin C64 frontend over roguelike-core::tier_micro. All game logic
// (map gen, FOV, entities, combat, AI, spawning, messages) comes from
// the shared core crate. This file handles hardware init, seed reading,
// and the main loop.

#![no_std]
#![no_main]
#![allow(static_mut_refs)] // Single-threaded bare metal — static mut is safe

mod c64;
mod render;
mod input;

use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use roguelike_core::tier_micro::game::MicroGameState;

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

/// Read a 16-bit seed from the CIA1 timer (human timing jitter = entropy).
fn read_cia_seed() -> u16 {
    let lo = c64::peek(c64::CIA1_TIMER_LO) as u16;
    let hi = c64::peek(unsafe { c64::CIA1_TIMER_LO.add(1) }) as u16;
    lo | (hi << 8)
}

#[no_mangle]
pub extern "C" fn main() -> isize {
    c64::init_hardware();

    let seed = read_cia_seed();
    render::render_title(seed);
    input::wait_any_key();

    // Initialize game state
    unsafe { STATE.write(MicroGameState::new(seed)); }
    let state = unsafe { STATE.assume_init_mut() };

    loop {
        render::render_all(state);

        if state.game_over {
            render::render_game_over(state);
            input::wait_any_key();
            *state = MicroGameState::new(read_cia_seed());
            continue;
        }

        let cmd = input::wait_for_input();
        state.step(cmd);
    }
}
