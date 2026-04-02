//! GBA scrollable message history viewer.
//!
//! Accessed via L+R combo during gameplay. Shows up to MSG_COUNT recent
//! messages with scroll support. Oldest at top, newest at bottom.
//! Up/Down to scroll, B or START to dismiss.

use roguelike_core::rules::game_view::GameView;

use crate::display;
use crate::format;
use crate::input::{self, MenuCommand};
use crate::menu;
use crate::palette::{PALBANK_DIM, PALBANK_MSG};

const PALBANK_TITLE: u16 = 8; // Yellow
const PALBANK_NEWEST: u16 = 1; // White
const PALBANK_OLDER: u16 = 2; // Grey (20,20,20) — readable but dimmer

const TITLE_ROW: usize = 1;
const SEP_ROW: usize = 2;
const FIRST_MSG_ROW: usize = 3;
/// Number of message rows visible at once (rows 3–16).
const VISIBLE_ROWS: usize = 14;
const HINT_ROW: usize = 18;

/// Run the message history overlay. Blocks until B or START is pressed.
#[inline(never)]
pub fn run_message_history(state: &impl GameView) {
    input::flush();
    menu::enable_dim();
    display::clear_hud();

    // Count available messages
    let mut count: u16 = 0;
    while count < 255 {
        if state.recent_message(count as u8).is_none() {
            break;
        }
        count += 1;
    }

    // scroll_offset: 0 = bottom (newest visible), increases = scroll up
    let max_scroll: u16 = count.saturating_sub(VISIBLE_ROWS as u16);
    let mut scroll: u16 = 0;

    render_history(state, count, scroll, max_scroll);

    loop {
        display::vblank_wait();
        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Back | MenuCommand::Start | MenuCommand::Select => break,
                MenuCommand::Up => {
                    if scroll < max_scroll {
                        scroll += 1;
                        render_history(state, count, scroll, max_scroll);
                    }
                }
                MenuCommand::Down => {
                    if scroll > 0 {
                        scroll -= 1;
                        render_history(state, count, scroll, max_scroll);
                    }
                }
            }
        }
    }

    menu::disable_dim();
    display::clear_hud();
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_history(state: &impl GameView, count: u16, scroll: u16, max_scroll: u16) {
    display::write_hud_string(4, TITLE_ROW, "MESSAGE LOG", PALBANK_TITLE);

    // Scroll indicator in title row
    if max_scroll > 0 {
        let mut pos_buf = [b' '; 10];
        // Show "N/M" where N = page position, M = total messages
        let showing_top = count.saturating_sub(scroll + VISIBLE_ROWS as u16) + 1;
        let pos = format_scroll_indicator(&mut pos_buf, showing_top, count);
        if let Ok(s) = core::str::from_utf8(&pos_buf[..pos]) {
            display::write_hud_string(display::SCREEN_COLS - pos - 1, TITLE_ROW, s, PALBANK_DIM);
        }
    }

    display::write_hud_separator(SEP_ROW, 2, 28, PALBANK_DIM);

    if count == 0 {
        display::write_hud_string(4, FIRST_MSG_ROW, "No messages yet.", PALBANK_DIM);
        for row in (FIRST_MSG_ROW + 1)..(FIRST_MSG_ROW + VISIBLE_ROWS) {
            display::clear_hud_row(row);
        }
    } else {
        let visible = (count as usize).min(VISIBLE_ROWS);
        // The oldest visible message index into recent_message():
        // scroll=0: show recent(0)..recent(visible-1), newest at bottom
        // scroll=N: show recent(N)..recent(N+visible-1), scrolled up
        for row_idx in 0..VISIBLE_ROWS {
            let screen_row = FIRST_MSG_ROW + row_idx;
            if row_idx >= visible && scroll == 0 {
                display::clear_hud_row(screen_row);
                continue;
            }
            // Map screen row to message index:
            // Bottom row (row_idx = visible-1) shows recent(scroll)
            // Top row (row_idx = 0) shows recent(scroll + visible - 1)
            let msg_idx = scroll + (visible as u16 - 1 - row_idx as u16);
            if msg_idx >= count {
                display::clear_hud_row(screen_row);
                continue;
            }
            if let Some(event) = state.recent_message(msg_idx as u8) {
                let mut buf = [b' '; 30];
                format::format_event(event, &mut buf);

                // Color: newest = white, recent = light grey, older = grey
                let pal = if msg_idx == 0 {
                    PALBANK_NEWEST
                } else if msg_idx <= 2 {
                    PALBANK_MSG
                } else {
                    PALBANK_OLDER
                };

                display::clear_hud_row(screen_row);
                if let Ok(s) = core::str::from_utf8(&buf) {
                    display::write_hud_string(1, screen_row, s.trim_end(), pal);
                }
            } else {
                display::clear_hud_row(screen_row);
            }
        }
    }

    // Hint row
    display::clear_hud_row(HINT_ROW);
    if max_scroll > 0 {
        let mut hint = "B:Back";
        if scroll < max_scroll && scroll > 0 {
            hint = "Up/Dn:Scroll  B:Back";
        } else if scroll < max_scroll {
            hint = "Up:Scroll  B:Back";
        } else if scroll > 0 {
            hint = "Dn:Scroll  B:Back";
        }
        display::write_hud_string(2, HINT_ROW, hint, PALBANK_DIM);
    } else {
        display::write_hud_string(2, HINT_ROW, "B:Back", PALBANK_DIM);
    }

    // Scroll arrows
    if scroll < max_scroll {
        display::write_hud_tile(0, FIRST_MSG_ROW, 0x1E, PALBANK_DIM); // ▲
    }
    if scroll > 0 {
        display::write_hud_tile(0, FIRST_MSG_ROW + VISIBLE_ROWS - 1, 0x1F, PALBANK_DIM); // ▼
    }
}

/// Format "N/M" into buf, returns length written.
fn format_scroll_indicator(buf: &mut [u8; 10], top: u16, total: u16) -> usize {
    let mut pos = 0;
    pos += write_u16_into(&mut buf[pos..], top);
    buf[pos] = b'/';
    pos += 1;
    pos += write_u16_into(&mut buf[pos..], total);
    pos
}

/// Write a u16 as decimal digits into buf, returns length.
fn write_u16_into(buf: &mut [u8], val: u16) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut digits = [0u8; 5];
    let mut n = val;
    let mut len = 0;
    while n > 0 {
        digits[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    for i in 0..len {
        buf[i] = digits[len - 1 - i];
    }
    len
}
