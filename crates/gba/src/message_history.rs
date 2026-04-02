//! GBA message history viewer — shows up to 8 recent messages.
//!
//! Accessed via L+R combo during gameplay. Oldest at top, newest at bottom
//! with color fading. B dismisses.

use roguelike_core::rules::game_view::GameView;

use crate::display;
use crate::format;
use crate::input::{self, MenuCommand};
use crate::menu;
use crate::palette::{PALBANK_DIM, PALBANK_MSG};

const PALBANK_TITLE: u16 = 8; // Yellow
const PALBANK_NEWEST: u16 = 1; // White

const TITLE_ROW: usize = 1;
const SEP_ROW: usize = 2;
const FIRST_MSG_ROW: usize = 3;
const HINT_ROW: usize = 17;

/// Run the message history overlay. Blocks until B or START is pressed.
#[inline(never)]
pub fn run_message_history(state: &impl GameView) {
    input::flush();
    menu::enable_dim();
    display::clear_hud();

    render_history(state);

    loop {
        display::vblank_wait();
        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Back | MenuCommand::Start | MenuCommand::Select => break,
                _ => {}
            }
        }
    }

    menu::disable_dim();
    display::clear_hud();
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_history(state: &impl GameView) {
    display::write_hud_string(4, TITLE_ROW, "MESSAGE LOG", PALBANK_TITLE);
    display::write_hud_separator(SEP_ROW, 2, 28, PALBANK_DIM);

    // Count available messages
    let mut count: u8 = 0;
    while count < 8 {
        if state.recent_message(count).is_none() {
            break;
        }
        count += 1;
    }

    if count == 0 {
        display::write_hud_string(4, FIRST_MSG_ROW, "No messages yet.", PALBANK_DIM);
    } else {
        // Display oldest first (n=count-1 at top), newest last (n=0 at bottom)
        let mut row = FIRST_MSG_ROW;
        let mut i = count;
        while i > 0 {
            i -= 1;
            if let Some(event) = state.recent_message(i) {
                let mut buf = [b' '; 30];
                format::format_event(event, &mut buf);

                let pal = match i {
                    0 => PALBANK_NEWEST,    // newest = white
                    1..=2 => PALBANK_MSG,   // recent = light grey
                    _ => PALBANK_DIM,       // older = dim grey
                };

                if let Ok(s) = core::str::from_utf8(&buf) {
                    display::write_hud_string(1, row, s.trim_end(), pal);
                }
                row += 1;
            }
        }
    }

    display::write_hud_string(2, HINT_ROW, "B:Back", PALBANK_DIM);
}
