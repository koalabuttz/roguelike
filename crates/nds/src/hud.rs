//! Game HUD rendering on Engine B (bottom screen).
//!
//! Renders player status and message log using the `GameView` trait,
//! below the automap viewport on the bottom screen.
//!
//! ## Screen layout (32 columns × 24 rows)
//!
//! ```text
//! Rows  0-19: Automap viewport — rendered by automap.rs
//! Row  20:    Status bar (HP/ATK/DEF/Depth/Turns)
//! Rows 21-23: Message log (3 lines, oldest top, newest bottom)
//! ```

use roguelike_core::rules::game_view::GameView;

use crate::debug_hud;
use crate::format;

/// Status bar row (below the automap viewport).
const STATUS_ROW: u8 = 20;

/// First message log row (below the status bar).
const MSG_START_ROW: u8 = 21;

/// Number of message lines to display.
const MSG_LINES: usize = 3;

/// Palette bank for status bar (green).
const PAL_STATUS: u16 = 1;

/// Palette bank for messages (yellow).
const PAL_MSG: u16 = 2;

/// Render the full game HUD: status bar + message log.
pub fn render_hud(state: &impl GameView) {
    render_status(state);
    render_messages(state);
}

/// Render the status bar at the bottom of the top screen.
///
/// Format: `HP:X/Y A:N D:N F:N T:NNN`
fn render_status(state: &impl GameView) {
    let mut buf = [b' '; 32];
    let mut p = 0;

    let (hp, max_hp) = state.player_hp();
    p = format::write_str(&mut buf, p, "HP:");
    p = format::write_u16(&mut buf, p, hp as u16);
    buf[p] = b'/';
    p += 1;
    p = format::write_u16(&mut buf, p, max_hp as u16);

    p = format::write_str(&mut buf, p, " A:");
    p = format::write_u16(&mut buf, p, state.effective_attack() as u16);

    p = format::write_str(&mut buf, p, " D:");
    p = format::write_u16(&mut buf, p, state.effective_defense() as u16);

    p = format::write_str(&mut buf, p, " F:");
    p = format::write_u16(&mut buf, p, state.depth() as u16);

    p = format::write_str(&mut buf, p, " T:");
    let _ = format::write_u16(&mut buf, p, state.turn_count());

    debug_hud::write_text_pal(0, STATUS_ROW, &buf, PAL_STATUS);
}

/// Render the last 4 messages above the status bar.
///
/// `recent_message(0)` = newest. Oldest message on top (row 19),
/// newest on bottom (row 22).
fn render_messages(state: &impl GameView) {
    for row in 0..MSG_LINES {
        let screen_row = MSG_START_ROW + row as u8;
        let msg_idx = (MSG_LINES - 1 - row) as u8;

        if let Some(event) = state.recent_message(msg_idx) {
            let mut buf = [b' '; 32];
            format::format_event(event, &mut buf);
            debug_hud::write_text_pal(0, screen_row, &buf, PAL_MSG);
        } else {
            let blank = [b' '; 32];
            debug_hud::write_text(0, screen_row, &blank);
        }
    }
}
