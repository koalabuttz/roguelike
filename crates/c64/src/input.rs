// Input handling — keyboard matrix scan and joystick.
//
// Returns GameCommand values from roguelike-core instead of raw u8 constants.
// Keyboard input uses direct CIA1 matrix scanning (no KERNAL dependency).

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

/// Read joystick port 2 for menu navigation.
/// Returns Up, Down, or Select (fire button).
fn read_joystick_menu() -> Option<MenuInput> {
    let (dir, fire) = read_joystick_raw();

    if fire {
        return Some(MenuInput::Select);
    }

    match dir {
        Some(Direction::North) => Some(MenuInput::Up),
        Some(Direction::South) => Some(MenuInput::Down),
        _ => None,
    }
}

/// Read joystick port 2 bits. Returns (direction, fire).
/// Isolates joystick lines by disabling keyboard column scanning.
fn read_joystick_raw() -> (Option<Direction>, bool) {
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;

    let up    = joy & 0x01 != 0;
    let down  = joy & 0x02 != 0;
    let left  = joy & 0x04 != 0;
    let right = joy & 0x08 != 0;
    let fire  = joy & 0x10 != 0;

    let dir = match (up, down, left, right) {
        (true,  false, false, false) => Some(Direction::North),
        (false, true,  false, false) => Some(Direction::South),
        (false, false, false, true)  => Some(Direction::East),
        (false, false, true,  false) => Some(Direction::West),
        (true,  false, false, true)  => Some(Direction::NorthEast),
        (true,  false, true,  false) => Some(Direction::NorthWest),
        (false, true,  false, true)  => Some(Direction::SouthEast),
        (false, true,  true,  false) => Some(Direction::SouthWest),
        _ => None,
    };

    (dir, fire)
}

/// Read joystick port 2. Returns game command if any direction/fire active.
fn read_joystick() -> Option<GameCommand> {
    let (dir, fire) = read_joystick_raw();

    if fire && dir.is_none() {
        return Some(GameCommand::Wait);
    }

    dir.map(GameCommand::Move)
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

/// Wait for a menu navigation input from keyboard or joystick.
/// Blocks until a valid input is received.
pub fn wait_for_menu_input() -> MenuInput {
    loop {
        let key = read_key();
        if key != 0 {
            if let Some(input) = key_to_menu(key) {
                return input;
            }
        }

        if let Some(input) = read_joystick_menu() {
            while joy_active() {}
            return input;
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
/// Blocks until a valid input is received.
pub fn wait_for_look_input() -> LookInput {
    loop {
        let key = read_key();
        if key != 0 {
            if let Some(dir) = key_to_direction(key) {
                return LookInput::Move(dir);
            }
            if matches!(key, KEY_X | KEY_RUNSTOP | KEY_RETURN) {
                return LookInput::Close;
            }
        }

        let (dir, fire) = read_joystick_raw();
        if fire {
            while joy_active() {}
            return LookInput::Close;
        }
        if let Some(d) = dir {
            while joy_active() {}
            return LookInput::Move(d);
        }
    }
}

/// Check if joystick port 2 has any active input.
fn joy_active() -> bool {
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;
    joy & 0x1F != 0
}
