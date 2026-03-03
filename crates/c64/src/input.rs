// Input handling — keyboard matrix scan and joystick.
//
// Returns GameCommand values from roguelike-core instead of raw u8 constants.
// Keyboard input uses direct CIA1 matrix scanning (no KERNAL dependency).
// Joystick uses edge detection + auto-repeat to filter noise/drift.

use crate::c64;
use roguelike_core::command::{Direction, GameCommand};

// Key codes from c64::scan_keyboard() — uppercase ASCII or PETSCII control codes.
const KEY_W: u8 = b'W';
const KEY_A: u8 = b'A';
const KEY_S: u8 = b'S';
const KEY_D: u8 = b'D';
const KEY_Q: u8 = b'Q';
const KEY_E: u8 = b'E';
const KEY_Z: u8 = b'Z';
const KEY_C: u8 = b'C';
const KEY_X: u8 = b'X';
const KEY_SPACE: u8 = c64::PETSCII_SPACE;
const KEY_UP: u8 = c64::PETSCII_UP;
const KEY_DOWN: u8 = c64::PETSCII_DOWN;
const KEY_LEFT: u8 = c64::PETSCII_LEFT;
const KEY_RIGHT: u8 = c64::PETSCII_RIGHT;
const KEY_RETURN: u8 = c64::PETSCII_RETURN;
const KEY_RUNSTOP: u8 = c64::PETSCII_STOP;
const KEY_DELETE: u8 = c64::PETSCII_DELETE;

/// Menu navigation input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    Up,
    Down,
    Select,
    Back,
}

/// Scan keyboard matrix for a new keypress. Returns key code or 0.
fn read_key() -> u8 {
    c64::scan_keyboard()
}

/// Map PETSCII key code to a Direction. Shared by game and look input.
fn key_to_direction(key: u8) -> Option<Direction> {
    match key {
        KEY_W | KEY_UP    => Some(Direction::North),
        KEY_S | KEY_DOWN  => Some(Direction::South),
        KEY_D | KEY_RIGHT => Some(Direction::East),
        KEY_A | KEY_LEFT  => Some(Direction::West),
        KEY_E             => Some(Direction::NorthEast),
        KEY_Q             => Some(Direction::NorthWest),
        KEY_C             => Some(Direction::SouthEast),
        KEY_Z             => Some(Direction::SouthWest),
        _ => None,
    }
}

/// Translate keyboard PETSCII code to game command.
fn key_to_cmd(key: u8) -> Option<GameCommand> {
    if let Some(dir) = key_to_direction(key) {
        return Some(GameCommand::Move(dir));
    }
    match key {
        KEY_X             => Some(GameCommand::Look),
        KEY_SPACE         => Some(GameCommand::Wait),
        KEY_RETURN        => Some(GameCommand::Descend),
        KEY_RUNSTOP       => Some(GameCommand::Quit),
        _ => None,
    }
}

/// Translate keyboard PETSCII code to menu input.
fn key_to_menu(key: u8) -> Option<MenuInput> {
    match key {
        KEY_W | KEY_UP => Some(MenuInput::Up),
        KEY_S | KEY_DOWN => Some(MenuInput::Down),
        KEY_RETURN | KEY_SPACE => Some(MenuInput::Select),
        KEY_RUNSTOP => Some(MenuInput::Back),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Joystick — edge detection + auto-repeat
// ---------------------------------------------------------------------------

/// Frames before held joystick starts repeating (~200ms at 60 Hz).
const JOY_INITIAL_DELAY: u8 = 12;
/// Frames between repeats once held (~67ms at 60 Hz).
const JOY_REPEAT_RATE: u8 = 4;

/// Previous joystick state for edge detection (5 bits: up/down/left/right/fire).
static mut PREV_JOY: u8 = 0;
/// Frames remaining before next repeat fires.
static mut JOY_DELAY: u8 = 0;

/// Read joystick port 2 as a 5-bit bitmask (bits 0-4 = up/down/left/right/fire).
fn joy_bits() -> u8 {
    c64::poke(c64::CIA1_PA, 0xFF);
    (c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF) & 0x1F
}

/// Decode 5-bit joystick state into (direction, fire).
/// Uses direct bit-pattern match instead of tuple destructuring to avoid
/// MOS compiler codegen issues with multi-field tuple pattern matching.
fn decode_joy(bits: u8) -> (Option<Direction>, bool) {
    let fire = bits & 0x10 != 0;

    // Match on direction bits (0-3) directly: bit0=up, bit1=down, bit2=left, bit3=right
    let dir = match bits & 0x0F {
        0x01 => Some(Direction::North),
        0x02 => Some(Direction::South),
        0x08 => Some(Direction::East),
        0x04 => Some(Direction::West),
        0x09 => Some(Direction::NorthEast),
        0x05 => Some(Direction::NorthWest),
        0x0A => Some(Direction::SouthEast),
        0x06 => Some(Direction::SouthWest),
        _    => None,
    };

    (dir, fire)
}

/// Joystick poll with edge detection and auto-repeat. Call once per frame.
/// Returns the active bits if the joystick should fire this frame.
fn joy_repeat() -> Option<u8> {
    let current = joy_bits();
    let prev = unsafe { PREV_JOY };

    if current == 0 {
        unsafe { PREV_JOY = 0; JOY_DELAY = 0; }
        return None;
    }

    if current != prev {
        // New direction — act immediately, start repeat countdown
        unsafe { PREV_JOY = current; JOY_DELAY = JOY_INITIAL_DELAY; }
        return Some(current);
    }

    // Same direction held — count down for repeat
    unsafe {
        if JOY_DELAY > 0 {
            JOY_DELAY -= 1;
            return None;
        }
        JOY_DELAY = JOY_REPEAT_RATE;
    }
    Some(current)
}

/// Joystick poll with edge detection only (no repeat). Call once per frame.
/// Returns the active bits only on a state change.
fn joy_edge() -> Option<u8> {
    let current = joy_bits();
    let prev = unsafe { PREV_JOY };
    unsafe { PREV_JOY = current; JOY_DELAY = 0; }

    if current != prev && current != 0 {
        Some(current)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Public input functions
// ---------------------------------------------------------------------------

/// Wait for and return a game command from either keyboard or joystick.
/// Blocks until a valid command is received. Polls once per frame.
pub fn wait_for_input() -> GameCommand {
    loop {
        c64::wait_next_frame();
        c64::music_auto_tick();

        let key = read_key();
        if key != 0 {
            if let Some(cmd) = key_to_cmd(key) {
                return cmd;
            }
        }

        if let Some(bits) = joy_repeat() {
            let (dir, fire) = decode_joy(bits);
            if fire && dir.is_none() {
                return GameCommand::Wait;
            }
            if let Some(d) = dir {
                return GameCommand::Move(d);
            }
        }
    }
}

/// Wait for a menu navigation input from keyboard or joystick.
/// Blocks until a valid input is received. Joystick uses edge-only
/// (no repeat) to prevent accidental menu scrolling.
pub fn wait_for_menu_input() -> MenuInput {
    loop {
        c64::wait_next_frame();

        let key = read_key();
        if key != 0 {
            if let Some(input) = key_to_menu(key) {
                return input;
            }
        }

        if let Some(bits) = joy_edge() {
            let (dir, fire) = decode_joy(bits);
            if fire { return MenuInput::Select; }
            match dir {
                Some(Direction::North) => return MenuInput::Up,
                Some(Direction::South) => return MenuInput::Down,
                _ => {}
            }
        }
    }
}

/// Read a seed code string via keyboard input.
///
/// Renders the input field using the provided callback after each keypress.
/// Returns `Some(len)` on Enter (confirm), `None` on Run/Stop (cancel).
/// `buf` receives lowercase ASCII bytes suitable for `decode_micro_from_bytes`.
pub fn read_seed_input(
    buf: &mut [u8; 12],
    mut render_fn: impl FnMut(&[u8], u8),
) -> Option<u8> {
    let mut len: u8 = 0;
    render_fn(&buf[..0], 0);

    loop {
        let key = read_key();
        if key == 0 {
            continue;
        }

        match key {
            KEY_RETURN => {
                if len > 0 {
                    return Some(len);
                }
            }
            KEY_RUNSTOP => return None,
            KEY_DELETE => {
                if len > 0 {
                    len -= 1;
                    render_fn(&buf[..len as usize], len);
                }
            }
            _ => {
                // Map PETSCII to lowercase ASCII for base36 compatibility
                let ascii = match key {
                    b'A'..=b'Z' => key - b'A' + b'a', // uppercase → lowercase
                    b'a'..=b'z' => key,                // already lowercase (shifted mode)
                    b'0'..=b'9' => key,
                    b'-' => key,
                    _ => continue, // ignore non-alphanumeric
                };

                if (len as usize) < buf.len() {
                    buf[len as usize] = ascii;
                    len += 1;
                    render_fn(&buf[..len as usize], len);
                }
            }
        }
    }
}

/// Look mode input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LookInput {
    Move(Direction),
    Close,
}

/// Wait for a look mode input from keyboard or joystick.
/// Blocks until a valid input is received. Joystick uses edge + repeat.
pub fn wait_for_look_input() -> LookInput {
    loop {
        c64::wait_next_frame();
        c64::music_auto_tick();

        let key = read_key();
        if key != 0 {
            if let Some(dir) = key_to_direction(key) {
                return LookInput::Move(dir);
            }
            if matches!(key, KEY_X | KEY_RUNSTOP | KEY_RETURN) {
                return LookInput::Close;
            }
        }

        if let Some(bits) = joy_repeat() {
            let (dir, fire) = decode_joy(bits);
            if fire { return LookInput::Close; }
            if let Some(d) = dir { return LookInput::Move(d); }
        }
    }
}
