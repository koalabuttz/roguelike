// Input handling — keyboard and joystick.
//
// Reads the Kernal keyboard buffer ($0277, length at $C6) for key presses
// and CIA1 port A ($DC00) for joystick port 2. The Kernal IRQ handler
// scans the keyboard matrix every frame, so the buffer fills automatically.
//
// Joystick port 2 bits (active LOW):
//   bit 0 = Up, bit 1 = Down, bit 2 = Left, bit 3 = Right, bit 4 = Fire

use crate::c64;

// Game commands — simple constants, not an enum, to minimize code size
pub const CMD_NONE: u8 = 0;
pub const CMD_MOVE_N: u8 = 1;
pub const CMD_MOVE_S: u8 = 2;
pub const CMD_MOVE_E: u8 = 3;
pub const CMD_MOVE_W: u8 = 4;
pub const CMD_MOVE_NE: u8 = 5;
pub const CMD_MOVE_NW: u8 = 6;
pub const CMD_MOVE_SE: u8 = 7;
pub const CMD_MOVE_SW: u8 = 8;
pub const CMD_WAIT: u8 = 9;

// PETSCII key codes
const KEY_W: u8 = 0x57;       // W (unshifted = uppercase PETSCII)
const KEY_A: u8 = 0x41;       // A
const KEY_S: u8 = 0x53;       // S
const KEY_D: u8 = 0x44;       // D
const KEY_Q: u8 = 0x51;       // Q (move NW)
const KEY_E: u8 = 0x45;       // E (move NE)
const KEY_Z: u8 = 0x5A;       // Z (move SW)
const KEY_C: u8 = 0x43;       // C (move SE)
const KEY_SPACE: u8 = 0x20;   // Space (wait)
const KEY_UP: u8 = 0x91;      // Cursor up
const KEY_DOWN: u8 = 0x11;    // Cursor down
const KEY_LEFT: u8 = 0x9D;    // Cursor left
const KEY_RIGHT: u8 = 0x1D;   // Cursor right

/// Decode a dx, dy pair from a command constant.
pub fn cmd_delta(cmd: u8) -> (i8, i8) {
    match cmd {
        CMD_MOVE_N  => ( 0, -1),
        CMD_MOVE_S  => ( 0,  1),
        CMD_MOVE_E  => ( 1,  0),
        CMD_MOVE_W  => (-1,  0),
        CMD_MOVE_NE => ( 1, -1),
        CMD_MOVE_NW => (-1, -1),
        CMD_MOVE_SE => ( 1,  1),
        CMD_MOVE_SW => (-1,  1),
        _ => (0, 0),
    }
}

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
fn key_to_cmd(key: u8) -> u8 {
    match key {
        KEY_W | KEY_UP    => CMD_MOVE_N,
        KEY_S | KEY_DOWN  => CMD_MOVE_S,
        KEY_D | KEY_RIGHT => CMD_MOVE_E,
        KEY_A | KEY_LEFT  => CMD_MOVE_W,
        KEY_E             => CMD_MOVE_NE,
        KEY_Q             => CMD_MOVE_NW,
        KEY_C             => CMD_MOVE_SE,
        KEY_Z             => CMD_MOVE_SW,
        KEY_SPACE         => CMD_WAIT,
        _ => CMD_NONE,
    }
}

/// Read joystick port 2. Returns game command.
/// Sets keyboard columns HIGH first to avoid ghost readings.
fn read_joystick() -> u8 {
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
        return CMD_WAIT;
    }

    // Diagonal detection from simultaneous directions
    match (up, down, left, right) {
        (true,  false, false, false) => CMD_MOVE_N,
        (false, true,  false, false) => CMD_MOVE_S,
        (false, false, false, true)  => CMD_MOVE_E,
        (false, false, true,  false) => CMD_MOVE_W,
        (true,  false, false, true)  => CMD_MOVE_NE,
        (true,  false, true,  false) => CMD_MOVE_NW,
        (false, true,  false, true)  => CMD_MOVE_SE,
        (false, true,  true,  false) => CMD_MOVE_SW,
        _ => CMD_NONE,
    }
}

/// Wait for and return a game command from either keyboard or joystick.
/// Blocks until a valid command is received.
pub fn wait_for_input() -> u8 {
    loop {
        // Check keyboard via Kernal buffer (works if IRQs are running)
        let key = read_key();
        if key != 0 {
            let cmd = key_to_cmd(key);
            if cmd != CMD_NONE {
                return cmd;
            }
        }

        // Check joystick port 2
        let joy_cmd = read_joystick();
        if joy_cmd != CMD_NONE {
            // Debounce: wait for joystick release
            while joy_active() {}
            return joy_cmd;
        }
    }
}

/// Wait for any keypress or joystick action (for title screen, death screen).
/// Uses Kernal keyboard buffer + joystick Port 2 (via Port A, no sharing issues).
pub fn wait_any_key() {
    // Drain any buffered keys
    c64::poke(c64::KEYBUF_LEN, 0);

    // Wait for joystick release first (debounce)
    while joy_active() {}

    loop {
        // Kernal keyboard buffer (filled by IRQ handler)
        if c64::peek(c64::KEYBUF_LEN) > 0 {
            c64::poke(c64::KEYBUF_LEN, 0);
            return;
        }

        // Joystick port 2 via CIA1 Port A (no Port B conflict)
        if joy_active() {
            while joy_active() {}
            return;
        }
    }
}

/// Check if joystick port 2 has any active input.
/// Reads CIA1 Port A with keyboard columns disabled to avoid conflicts.
fn joy_active() -> bool {
    // Set all keyboard column lines HIGH (inactive) before reading joystick
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;
    joy & 0x1F != 0
}

/// Non-blocking check for any input. Returns CMD_NONE if nothing pressed.
pub fn poll_input() -> u8 {
    let key = read_key();
    if key != 0 {
        let cmd = key_to_cmd(key);
        if cmd != CMD_NONE { return cmd; }
    }
    read_joystick()
}
