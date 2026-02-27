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
const KEY_RETURN: u8 = 0x0D;
const KEY_RUNSTOP: u8 = 0x03;
const KEY_DELETE: u8 = 0x14;

/// Menu navigation input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    Up,
    Down,
    Select,
    Back,
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
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;

    let up = joy & 0x01 != 0;
    let down = joy & 0x02 != 0;
    let fire = joy & 0x10 != 0;

    if fire {
        Some(MenuInput::Select)
    } else if up && !down {
        Some(MenuInput::Up)
    } else if down && !up {
        Some(MenuInput::Down)
    } else {
        None
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

/// Check if joystick port 2 has any active input.
fn joy_active() -> bool {
    c64::poke(c64::CIA1_PA, 0xFF);
    let joy = c64::peek(c64::CIA1_PA as *const u8) ^ 0xFF;
    joy & 0x1F != 0
}
