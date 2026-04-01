//! Reusable menu system for GBA overlays.
//!
//! Provides [`run_menu`] — a blocking menu loop with OAM cursor, palette-based
//! selection highlighting, and optional BG0 darken. Used by pause menu,
//! and available for future screens (help, settings, message history).

use gba::prelude::*;

use crate::display;
use crate::input::{self, MenuCommand};

// ---------------------------------------------------------------------------
// Shared dim helpers (also used by inventory_ui)
// ---------------------------------------------------------------------------

/// Darken BG0 (map layer) using hardware blend at the given percentage (0–100).
/// Clamped to the BLDY range of 0–16 (each unit ≈ 6.25%).
pub(crate) fn enable_dim_pct(pct: u16) {
    BLDCNT.write(
        BlendControl::new()
            .with_mode(ColorEffectMode::Darken)
            .with_target1_bg0(true),
    );
    // Map 0–100% to BLDY 0–16
    BLDY.write((pct * 16 / 100).min(16) as u8);
}

/// Darken BG0 at the default 62% level (BLDY=10). Convenience wrapper.
pub(crate) fn enable_dim() {
    enable_dim_pct(62);
}

/// Disable hardware blend (restore normal BG0 brightness).
pub(crate) fn disable_dim() {
    BLDCNT.write(BlendControl::new());
}

// ---------------------------------------------------------------------------
// Menu API
// ---------------------------------------------------------------------------

/// Palbank for the selected menu item (yellow foreground).
const PALBANK_SEL: u16 = 8; // GameColor::Yellow

/// Palbank for unselected menu items (grey foreground).
const PALBANK_NORMAL: u16 = 2; // Grey

/// Palbank for the menu title.
const PALBANK_TITLE: u16 = 8; // Yellow

/// Palbank for the separator line.
const PALBANK_SEP: u16 = 11; // Dim

/// Configuration for a menu overlay.
pub struct MenuConfig<'a> {
    /// Title displayed above the menu items.
    pub title: &'a str,
    /// Menu item labels.
    pub items: &'a [&'a str],
    /// Left column of the menu area (items are indented +2 from here).
    pub x: usize,
    /// Top row (title row).
    pub y: usize,
    /// Vertical spacing between items (2 = every other row).
    pub spacing: usize,
    /// Whether to darken BG0 with hardware blend.
    pub dim_bg0: bool,
}

/// Result of running a menu.
pub enum MenuResult {
    /// User selected the item at this index.
    Selected(u8),
    /// User pressed B to cancel/go back.
    Cancelled,
}

/// Run a blocking menu overlay. Returns when the user selects or cancels.
///
/// Handles OAM cursor, optional BG0 darken, BG1 rendering, and input.
#[inline(never)]
pub fn run_menu(config: &MenuConfig) -> MenuResult {
    let count = config.items.len() as u8;
    if count == 0 {
        return MenuResult::Cancelled;
    }

    let mut selected: u8 = 0;
    let mut frame: u16 = 0;
    let mut needs_redraw = true;

    // Setup
    input::flush(); // consume stale edges from the previous screen
    crate::cursor::init();
    if config.dim_bg0 {
        enable_dim();
    }
    display::clear_hud();

    // Render static elements (title + separator)
    render_chrome(config);

    loop {
        display::vblank_wait();
        frame = frame.wrapping_add(1);

        if needs_redraw {
            render_items(config, selected);
            needs_redraw = false;
        }

        // Update OAM cursor position with bob animation
        let cursor_row = item_row(config, selected);
        crate::cursor::update(config.x, cursor_row, frame, 0);

        if let Some(cmd) = input::read_menu_input() {
            match cmd {
                MenuCommand::Up => {
                    if selected > 0 {
                        selected -= 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Down => {
                    if selected < count - 1 {
                        selected += 1;
                        needs_redraw = true;
                    }
                }
                MenuCommand::Select => {
                    cleanup(config);
                    return MenuResult::Selected(selected);
                }
                MenuCommand::Back | MenuCommand::Start => {
                    cleanup(config);
                    return MenuResult::Cancelled;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Row of the separator line (one row below the title).
fn sep_row(config: &MenuConfig) -> usize {
    config.y + 1
}

/// Row of a menu item by index.
fn item_row(config: &MenuConfig, index: u8) -> usize {
    config.y + 2 + index as usize * config.spacing
}

/// Render the title and separator (called once).
fn render_chrome(config: &MenuConfig) {
    // Title — centered within the menu area
    display::write_hud_string(config.x + 2, config.y, config.title, PALBANK_TITLE);

    // Separator line
    let sep_width = config.items.iter().map(|s| s.len()).max().unwrap_or(0) + 4;
    display::write_hud_separator(sep_row(config), config.x, config.x + sep_width, PALBANK_SEP);
}

/// Render all menu items with selection highlighting.
fn render_items(config: &MenuConfig, selected: u8) {
    for (i, label) in config.items.iter().enumerate() {
        let row = item_row(config, i as u8);
        let pal = if i as u8 == selected {
            PALBANK_SEL
        } else {
            PALBANK_NORMAL
        };

        // Clear the row in the menu area
        for x in config.x..config.x + 20 {
            if x < display::SCREEN_COLS {
                display::write_hud_tile(x, row, b' ', 0);
            }
        }

        // Write item text (indented 2 tiles from menu x for cursor space)
        display::write_hud_string(config.x + 2, row, label, pal);
    }
}

/// Restore hardware state after menu closes.
fn cleanup(config: &MenuConfig) {
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();
    if config.dim_bg0 {
        disable_dim();
    }
    BG1HOFS.write(0);
    display::clear_hud();
}
