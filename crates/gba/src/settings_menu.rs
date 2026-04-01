//! GBA settings screen — toggle options with A, exit with B.
//!
//! Uses the shared dim/cursor helpers from [`menu`] but has its own input loop
//! because settings toggle in-place rather than selecting-and-exiting.

use crate::display;
use crate::game_loop;
use crate::input::{self, MenuCommand};
use crate::menu;
use crate::palette::{PALBANK_DIM, PALBANK_MSG};

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

/// Run the settings screen. Blocks until the player presses B.
#[inline(never)]
pub fn run_settings() {
    let mut selected: u8 = 0;
    let mut frame: u16 = 0;
    let mut needs_redraw = true;

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
            }
        }
    }

    // Cleanup
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();
    menu::disable_dim();
    display::clear_hud();
}

// ---------------------------------------------------------------------------
// Setting access
// ---------------------------------------------------------------------------

fn get_auto_pickup() -> bool {
    if game_loop::is_micro() {
        game_loop::game_micro().auto_pickup
    } else {
        game_loop::game_compact().auto_pickup
    }
}

fn set_auto_pickup(val: bool) {
    if game_loop::is_micro() {
        game_loop::game_micro().auto_pickup = val;
    } else {
        game_loop::game_compact().auto_pickup = val;
    }
}

fn toggle_setting(index: u8) {
    match index {
        0 => set_auto_pickup(!get_auto_pickup()),
        _ => {}
    }
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
    if get_auto_pickup() {
        display::write_hud_string(MENU_X + 17, row, "ON", val_pal);
    } else {
        display::write_hud_string(MENU_X + 17, row, "OFF", val_pal);
    }
}
