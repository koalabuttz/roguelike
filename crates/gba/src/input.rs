//! GBA joypad input: button-to-command mapping with edge detection and repeat.
//!
//! Read `KEYINPUT` once per frame, detect rising edges for action buttons,
//! and apply initial-delay + repeat-rate for D-pad movement.

use gba::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::rules::direction::Direction;

/// Look-mode input (local equivalent of core's LookCommand, which is std-only).
pub enum LookCommand {
    Move(Direction),
    Close,
}

/// Menu navigation input (local equivalent of core's MenuCommand, which is std-only).
pub enum MenuCommand {
    Up,
    Down,
    Select,
    Back,
    /// START button — used to dismiss nested menus and resume gameplay.
    Start,
}

/// Inventory modal input.
pub enum InventoryInput {
    Up,
    Down,
    /// A = context-smart primary action (Use/Equip/Unequip).
    Primary,
    /// L = open secondary submenu (Drop, Combine).
    Secondary,
    /// B = close inventory.
    Close,
}

/// Submenu popup input (2-3 item menu, no repeat needed).
pub enum SubmenuInput {
    Up,
    Down,
    /// A = confirm selection.
    Confirm,
    /// B = cancel back to browse.
    Cancel,
}

/// Frames before held D-pad starts repeating (~200ms at 60fps).
const INITIAL_DELAY: u16 = 12;
/// Frames between repeats once repeating (~67ms at 60fps).
const REPEAT_RATE: u16 = 4;

/// Input state persisted across frames.
/// Accessed via raw pointer — safe on single-threaded GBA.
static mut INPUT_STATE: InputState = InputState::new();

/// Get a mutable pointer to the input state.
/// Safety: GBA is single-threaded, single-core. No interrupts touch this.
fn state() -> &'static mut InputState {
    unsafe { &mut *(&raw mut INPUT_STATE) }
}

struct InputState {
    /// Previous frame's key state (inverted: 1 = pressed).
    prev_pressed: u16,
    /// Frame counter for D-pad repeat.
    repeat_counter: u16,
    /// Whether we're past the initial delay (in repeat phase).
    repeating: bool,
}

impl InputState {
    const fn new() -> Self {
        Self {
            prev_pressed: 0,
            repeat_counter: 0,
            repeating: false,
        }
    }
}

/// Read current pressed state as a bitmask (1 = pressed).
/// Inverts the raw low-active KEYINPUT register.
fn read_pressed() -> u16 {
    !KEYINPUT.read().to_u16() & 0x03FF // mask to 10 button bits
}

/// Returns true if any new button was pressed since the last call.
/// Uses edge detection so held buttons from the trigger don't immediately interrupt.
/// Call once per frame during autorun/auto-explore loops.
pub fn any_new_press() -> bool {
    let pressed = read_pressed();
    let s = state();
    let edges = pressed & !s.prev_pressed;
    s.prev_pressed = pressed;
    edges != 0
}

/// Consume any pending edges by snapshotting the current pressed state.
/// Call at the start of a new screen/modal to prevent input bleed-through.
pub fn flush() {
    let s = state();
    s.prev_pressed = read_pressed();
    s.repeat_counter = 0;
    s.repeating = false;
}

// Bit positions matching KeyInput field order.
const BIT_A: u16 = 1 << 0;
const BIT_B: u16 = 1 << 1;
const BIT_SELECT: u16 = 1 << 2;
const BIT_START: u16 = 1 << 3;
const BIT_RIGHT: u16 = 1 << 4;
const BIT_LEFT: u16 = 1 << 5;
const BIT_UP: u16 = 1 << 6;
const BIT_DOWN: u16 = 1 << 7;
const BIT_R: u16 = 1 << 8;
const BIT_L: u16 = 1 << 9;

/// Extract a Direction from D-pad state, supporting 8-way diagonals.
fn read_direction(pressed: u16) -> Option<Direction> {
    let dx: i32 = if pressed & BIT_RIGHT != 0 {
        1
    } else if pressed & BIT_LEFT != 0 {
        -1
    } else {
        0
    };
    let dy: i32 = if pressed & BIT_DOWN != 0 {
        1
    } else if pressed & BIT_UP != 0 {
        -1
    } else {
        0
    };
    Direction::from_offset(dx, dy)
}

/// Read gameplay input. Call once per frame during the game loop.
///
/// Returns `Some(GameCommand)` if a button was pressed (edge-triggered for
/// actions, with repeat for D-pad movement).
pub fn read_game_input() -> Option<GameCommand> {
    let pressed = read_pressed();

    let state = state();
    let edges = pressed & !state.prev_pressed;
    let dir = read_direction(pressed);
    let l_held = pressed & BIT_L != 0;

    // D-pad: edge-triggered with repeat
    let dir_cmd = if dir.is_some() {
        let dir_edges = edges & (BIT_UP | BIT_DOWN | BIT_LEFT | BIT_RIGHT);
        if dir_edges != 0 {
            // New direction pressed — emit immediately, reset repeat timer
            state.repeat_counter = 0;
            state.repeating = false;
            true
        } else {
            // Direction held — apply repeat logic
            state.repeat_counter += 1;
            let threshold = if state.repeating {
                REPEAT_RATE
            } else {
                INITIAL_DELAY
            };
            if state.repeat_counter >= threshold {
                state.repeat_counter = 0;
                state.repeating = true;
                true
            } else {
                false
            }
        }
    } else {
        // No direction held — reset repeat state
        state.repeat_counter = 0;
        state.repeating = false;
        false
    };

    state.prev_pressed = pressed;

    // Direction commands (highest priority — movement is the primary action)
    if dir_cmd {
        if let Some(d) = dir {
            return if l_held {
                Some(GameCommand::Autorun(d))
            } else {
                Some(GameCommand::Move(d))
            };
        }
    }

    // Edge-triggered action buttons
    if edges & BIT_A != 0 {
        return Some(GameCommand::Interact);
    }
    if edges & BIT_START != 0 {
        return Some(GameCommand::Quit); // opens pause menu (handled in game_loop)
    }
    if edges & BIT_B != 0 {
        return Some(GameCommand::Look);
    }
    if edges & BIT_SELECT != 0 {
        return Some(GameCommand::OpenInventory);
    }
    // L+R combo = Message History (check before individual L/R)
    if pressed & BIT_L != 0 && pressed & BIT_R != 0 && edges & (BIT_L | BIT_R) != 0 {
        return Some(GameCommand::MessageHistory);
    }
    if edges & BIT_R != 0 {
        return Some(GameCommand::AutoExplore);
    }
    // L alone (no D-pad) = Wait
    if edges & BIT_L != 0 && dir.is_none() {
        return Some(GameCommand::Wait);
    }

    None
}

/// Read menu navigation input. Call once per frame on menu screens.
pub fn read_menu_input() -> Option<MenuCommand> {
    let pressed = read_pressed();
    let state = state();
    let edges = pressed & !state.prev_pressed;
    state.prev_pressed = pressed;

    if edges & BIT_UP != 0 {
        return Some(MenuCommand::Up);
    }
    if edges & BIT_DOWN != 0 {
        return Some(MenuCommand::Down);
    }
    if edges & BIT_A != 0 {
        return Some(MenuCommand::Select);
    }
    if edges & BIT_B != 0 {
        return Some(MenuCommand::Back);
    }
    if edges & BIT_START != 0 {
        return Some(MenuCommand::Start);
    }

    None
}

/// Read look-mode input. Call once per frame while look cursor is active.
pub fn read_look_input() -> Option<LookCommand> {
    let pressed = read_pressed();
    let state = state();
    let edges = pressed & !state.prev_pressed;
    let dir = read_direction(pressed);

    // D-pad with repeat (same logic as gameplay)
    let dir_cmd = if dir.is_some() {
        let dir_edges = edges & (BIT_UP | BIT_DOWN | BIT_LEFT | BIT_RIGHT);
        if dir_edges != 0 {
            state.repeat_counter = 0;
            state.repeating = false;
            true
        } else {
            state.repeat_counter += 1;
            let threshold = if state.repeating {
                REPEAT_RATE
            } else {
                INITIAL_DELAY
            };
            if state.repeat_counter >= threshold {
                state.repeat_counter = 0;
                state.repeating = true;
                true
            } else {
                false
            }
        }
    } else {
        state.repeat_counter = 0;
        state.repeating = false;
        false
    };

    state.prev_pressed = pressed;

    if dir_cmd {
        if let Some(d) = dir {
            return Some(LookCommand::Move(d));
        }
    }

    if edges & BIT_B != 0 {
        return Some(LookCommand::Close);
    }

    None
}

/// Read inventory modal input. D-pad up/down with repeat, edge-triggered buttons.
pub fn read_inventory_input() -> Option<InventoryInput> {
    let pressed = read_pressed();
    let state = state();
    let edges = pressed & !state.prev_pressed;

    // D-pad up/down with repeat (same pattern as game/look input)
    let ud = pressed & (BIT_UP | BIT_DOWN);
    let dir_cmd = if ud != 0 {
        let dir_edges = edges & (BIT_UP | BIT_DOWN);
        if dir_edges != 0 {
            state.repeat_counter = 0;
            state.repeating = false;
            true
        } else {
            state.repeat_counter += 1;
            let threshold = if state.repeating { REPEAT_RATE } else { INITIAL_DELAY };
            if state.repeat_counter >= threshold {
                state.repeat_counter = 0;
                state.repeating = true;
                true
            } else {
                false
            }
        }
    } else {
        state.repeat_counter = 0;
        state.repeating = false;
        false
    };

    state.prev_pressed = pressed;

    if dir_cmd {
        if pressed & BIT_UP != 0 {
            return Some(InventoryInput::Up);
        }
        if pressed & BIT_DOWN != 0 {
            return Some(InventoryInput::Down);
        }
    }

    // Edge-triggered action buttons
    if edges & BIT_A != 0 {
        return Some(InventoryInput::Primary);
    }
    if edges & BIT_L != 0 {
        return Some(InventoryInput::Secondary);
    }
    if edges & BIT_B != 0 {
        return Some(InventoryInput::Close);
    }

    None
}

/// Read submenu popup input. Edge-triggered only (small menu, no repeat needed).
pub fn read_submenu_input() -> Option<SubmenuInput> {
    let pressed = read_pressed();
    let state = state();
    let edges = pressed & !state.prev_pressed;
    state.prev_pressed = pressed;

    if edges & BIT_UP != 0 {
        return Some(SubmenuInput::Up);
    }
    if edges & BIT_DOWN != 0 {
        return Some(SubmenuInput::Down);
    }
    if edges & BIT_A != 0 {
        return Some(SubmenuInput::Confirm);
    }
    if edges & BIT_B != 0 {
        return Some(SubmenuInput::Cancel);
    }

    None
}
