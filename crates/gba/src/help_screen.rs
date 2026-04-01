//! GBA help screen — single-page controls reference and gameplay hints.
//!
//! Accessed from the pause menu. No selectable items — just render and wait
//! for B (back to pause) or START (resume game).

use crate::display;
use crate::input::{self, MenuCommand};
use crate::menu;
use crate::palette::PALBANK_DIM;

/// Palbank for section headers.
const PALBANK_TITLE: u16 = 8; // Yellow

/// Palbank for button labels (left column).
const PALBANK_KEY: u16 = 1; // White

/// Palbank for action descriptions (right column).
const PALBANK_DESC: u16 = 2; // Grey

/// Palbank for gameplay hint text.
const PALBANK_HINT: u16 = 2; // Grey

const TITLE_ROW: usize = 1;
const SEP_ROW: usize = 2;
const CONTROLS_START: usize = 3;
const KEY_COL: usize = 2;
const DESC_COL: usize = 12;
const HINT_START: usize = 12;
const HINT_ROW: usize = 17;

/// How the help screen was dismissed.
pub enum HelpResult {
    /// B pressed — return to pause menu.
    Back,
    /// START pressed — dismiss all menus and resume gameplay.
    Resume,
}

/// Run the help screen. Blocks until the player presses B or START.
#[inline(never)]
pub fn run_help() -> HelpResult {
    input::flush();
    menu::enable_dim();
    display::clear_hud();

    render_help();

    let mut result = HelpResult::Back;
    loop {
        display::vblank_wait();
        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Back => break,
                MenuCommand::Start => {
                    result = HelpResult::Resume;
                    break;
                }
                _ => {} // nothing to select
            }
        }
    }

    menu::disable_dim();
    display::clear_hud();
    result
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_help() {
    // Title + separator
    display::write_hud_string(KEY_COL + 2, TITLE_ROW, "HELP", PALBANK_TITLE);
    display::write_hud_separator(SEP_ROW, KEY_COL, KEY_COL + 22, PALBANK_DIM);

    // Controls
    let controls: [(&str, &str); 8] = [
        ("D-pad", "Move/Attack"),
        ("D-pad+L", "Autorun"),
        ("A", "Interact"),
        ("B", "Look"),
        ("START", "Pause"),
        ("SELECT", "Inventory"),
        ("R", "Auto-explore"),
        ("L", "Wait"),
    ];

    for (i, (key, desc)) in controls.iter().enumerate() {
        let row = CONTROLS_START + i;
        display::write_hud_string(KEY_COL, row, key, PALBANK_KEY);
        display::write_hud_string(DESC_COL, row, desc, PALBANK_DESC);
    }

    // Gameplay hints
    display::write_hud_string(KEY_COL, HINT_START, "Bump monsters to fight.", PALBANK_HINT);
    display::write_hud_string(KEY_COL, HINT_START + 1, "Descend stairs to go deeper.", PALBANK_HINT);
    display::write_hud_string(KEY_COL, HINT_START + 2, "Find a way out.", PALBANK_HINT);

    // Dismiss hint
    display::write_hud_string(KEY_COL, HINT_ROW, "B:Back", PALBANK_DIM);
}
