//! GBA settings screen — toggle options with A, exit with B or START.
//!
//! Uses the shared dim/cursor helpers from [`menu`] but has its own input loop
//! because settings toggle in-place rather than selecting-and-exiting.
//!
//! B returns to the parent menu. START dismisses everything and resumes gameplay.

use crate::display;
use crate::input::{self, MenuCommand};
use crate::menu;
use crate::palette::{PALBANK_DIM, PALBANK_MSG};
use crate::saves;

/// Palbank for the selected setting row.
const PALBANK_SEL: u16 = 8; // Yellow

/// Palbank for the title.
const PALBANK_TITLE: u16 = 8; // Yellow

const MENU_X: usize = 4;
const TITLE_ROW: usize = 5;
const SEP_ROW: usize = 6;
const FIRST_ITEM_ROW: usize = 8;
const ITEM_SPACING: usize = 2;
const HINT_ROW: usize = 16;

/// Number of toggleable settings.
const SETTING_COUNT: u8 = 1;

/// How the settings screen was dismissed.
pub enum SettingsResult {
    /// B pressed — return to parent menu.
    Back,
    /// START pressed — dismiss all menus and resume gameplay.
    Resume,
}

/// Run the settings screen. Blocks until the player presses B or START.
#[inline(never)]
pub fn run_settings() -> SettingsResult {
    let mut selected: u8 = 0;
    let mut frame: u16 = 0;
    let mut needs_redraw = true;
    let mut result = SettingsResult::Back;

    input::flush(); // consume stale edges from the pause menu
    crate::cursor::init();
    menu::enable_dim();
    display::clear_hud();

    // Static chrome
    display::write_hud_string(MENU_X + 2, TITLE_ROW, "SETTINGS", PALBANK_TITLE);
    display::write_hud_separator(SEP_ROW, MENU_X, MENU_X + 22, PALBANK_DIM);
    display::write_hud_string(MENU_X, HINT_ROW, "A:Toggle  B:Back", PALBANK_DIM);

    loop {
        display::vblank_wait();
        frame = frame.wrapping_add(1);

        if needs_redraw {
            render_settings(selected);
            needs_redraw = false;
        }

        let row = FIRST_ITEM_ROW + selected as usize * ITEM_SPACING;
        crate::cursor::update(MENU_X, row, frame, 0);

        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Up => {
                    if selected > 0 {
                        selected -= 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Down => {
                    if selected < SETTING_COUNT - 1 {
                        selected += 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Select => {
                    toggle_setting(selected);
                    needs_redraw = true;
                }
                MenuCommand::Back => break,
                MenuCommand::Start => {
                    result = SettingsResult::Resume;
                    break;
                }
            }
        }
    }

    // Cleanup
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();
    menu::disable_dim();
    display::clear_hud();

    result
}

// ---------------------------------------------------------------------------
// Setting access (reads/writes persistent SRAM settings)
// ---------------------------------------------------------------------------

fn toggle_setting(index: u8) {
    let mut s = saves::settings();
    match index {
        0 => s.auto_pickup = !s.auto_pickup,
        _ => {}
    }
    saves::update_settings(s);
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_settings(selected: u8) {
    // Auto-pickup row
    let row = FIRST_ITEM_ROW;
    let pal = if selected == 0 { PALBANK_SEL } else { PALBANK_MSG };
    let val_pal = if selected == 0 { PALBANK_SEL } else { PALBANK_DIM };

    // Clear the row
    for x in MENU_X..MENU_X + 22 {
        display::write_hud_tile(x, row, b' ', 0);
    }

    display::write_hud_string(MENU_X + 2, row, "Auto-pickup", pal);
    if saves::settings().auto_pickup {
        display::write_hud_string(MENU_X + 17, row, "ON", val_pal);
    } else {
        display::write_hud_string(MENU_X + 17, row, "OFF", val_pal);
    }
}
