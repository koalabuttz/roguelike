// Input handling — keyboard and joystick.
//
// Returns GameCommand values from roguelike-core instead of raw u8 constants.
// Hardware reading (Kernal buffer, CIA1 joystick) is unchanged from the POC.

use crate::c64;
use roguelike_core::command::{Direction, GameCommand};

// PETSCII key codes
const KEY_W: u8 = 0x57;
const KEY_A: u8 = 0x41;
const KEY_S: u8 = 0x53;
const KEY_D: u8 = 0x44;
const KEY_Q: u8 = 0x51;       // NW
const KEY_E: u8 = 0x45;       // NE
const KEY_Z: u8 = 0x5A;       // SW
const KEY_C: u8 = 0x43;       // SE
const KEY_SPACE: u8 = 0x20;
const KEY_UP: u8 = 0x91;
const KEY_DOWN: u8 = 0x11;
const KEY_LEFT: u8 = 0x9D;
const KEY_RIGHT: u8 = 0x1D;

/// Check keyboard buffer for a keypress. Returns PETSCII code or 0.
fn read_key() -> u8 {
    let count = c64::peek(c64::KEYBUF_LEN);
    if count > 0 {
        let key = c64::peek(c64::KEYBUF);
        c64::poke(c64::KEYBUF_LEN, 0); // consume entire buffer
        key
    } else {
        0
    }
}

/// Translate keyboard PETSCII code to game command.
fn key_to_cmd(key: u8) -> Option<GameCommand> {
    match key {
        KEY_W | KEY_UP    => Some(GameCommand::Move(Direction::North)),
        KEY_S | KEY_DOWN  => Some(GameCommand::Move(Direction::South)),
        KEY_D | KEY_RIGHT => Some(GameCommand::Move(Direction::East)),
        KEY_A | KEY_LEFT  => Some(GameCommand::Move(Direction::West)),
        KEY_E             => Some(GameCommand::Move(Direction::NorthEast)),
        KEY_Q             => Some(GameCommand::Move(Direction::NorthWest)),
        KEY_C             => Some(GameCommand::Move(Direction::SouthEast)),
        KEY_Z             => Some(GameCommand::Move(Direction::SouthWest)),
        KEY_SPACE         => Some(GameCommand::Wait),
        _ => None,
    }
}

/// Read joystick port 2. Returns game command if any direction/fire active.
/// Sets keyboard columns HIGH first to avoid ghost readings.
fn read_joystick() -> Option<GameCommand> {
    // Disable keyboard column scanning to isolate joystick lines
    c64::poke(c64::CIA1_PA, 0xFF);
    // Read CIA1 Port A — joystick bits are active LOW
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF; // invert: now 1=active

    let up    = joy & 0x01 != 0;
    let down  = joy & 0x02 != 0;
    let left  = joy & 0x04 != 0;
    let right = joy & 0x08 != 0;
    let fire  = joy & 0x10 != 0;

    // Fire with no direction = wait
    if fire && !up && !down && !left && !right {
        return Some(GameCommand::Wait);
    }

    // Diagonal detection from simultaneous directions
    match (up, down, left, right) {
        (true,  false, false, false) => Some(GameCommand::Move(Direction::North)),
        (false, true,  false, false) => Some(GameCommand::Move(Direction::South)),
        (false, false, false, true)  => Some(GameCommand::Move(Direction::East)),
        (false, false, true,  false) => Some(GameCommand::Move(Direction::West)),
        (true,  false, false, true)  => Some(GameCommand::Move(Direction::NorthEast)),
        (true,  false, true,  false) => Some(GameCommand::Move(Direction::NorthWest)),
        (false, true,  false, true)  => Some(GameCommand::Move(Direction::SouthEast)),
        (false, true,  true,  false) => Some(GameCommand::Move(Direction::SouthWest)),
        _ => None,
    }
}

/// Wait for and return a game command from either keyboard or joystick.
/// Blocks until a valid command is received.
pub fn wait_for_input() -> GameCommand {
    loop {
        let key = read_key();
        if key != 0 {
            if let Some(cmd) = key_to_cmd(key) {
                return cmd;
            }
        }

        if let Some(cmd) = read_joystick() {
            // Debounce: wait for joystick release
            while joy_active() {}
            return cmd;
        }
    }
}

/// Wait for any keypress or joystick action (for title screen, death screen).
pub fn wait_any_key() {
    // Drain any buffered keys
    c64::poke(c64::KEYBUF_LEN, 0);

    // Wait for joystick release first (debounce)
    while joy_active() {}

    loop {
        if c64::peek(c64::KEYBUF_LEN) > 0 {
            c64::poke(c64::KEYBUF_LEN, 0);
            return;
        }

        if joy_active() {
            while joy_active() {}
            return;
        }
    }
}

/// Check if joystick port 2 has any active input.
fn joy_active() -> bool {
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;
    joy & 0x1F != 0
}
