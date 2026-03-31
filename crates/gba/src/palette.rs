//! Palette constants and initialization for GBA display.
//!
//! Maps `GameColor` to GBA palbanks (4bpp mode). Banks 0-10 are a direct
//! mapping from `GameColor as u8`. Banks 11-14 are UI-specific states.

use gba::prelude::*;
use roguelike_core::rules::color::GameColor;

/// Palbank for explored-but-not-visible tiles (dimmed).
pub const PALBANK_DIM: u16 = 11;
/// Palbank for the status bar (white on dark blue).
pub const PALBANK_STATUS: u16 = 12;
/// Palbank for message log text (light grey on black).
pub const PALBANK_MSG: u16 = 13;
/// Palbank for look cursor / selection highlight (inverse video).
pub const PALBANK_HIGHLIGHT: u16 = 14;

/// Number of palbanks we initialize.
const PALBANK_COUNT: usize = 15;

/// RGB555 color pairs: (background, foreground) for each palbank.
/// Index in this array = palbank number.
const PALETTE_ENTRIES: [(Color, Color); PALBANK_COUNT] = [
    // 0  Black — empty/unused tiles
    (Color::from_rgb(0, 0, 0), Color::from_rgb(0, 0, 0)),
    // 1  White — structural walls, default text
    (Color::from_rgb(0, 0, 0), Color::from_rgb(31, 31, 31)),
    // 2  Grey — UI secondary text
    (Color::from_rgb(0, 0, 0), Color::from_rgb(20, 20, 20)),
    // 3  DarkGrey — floor dots
    (Color::from_rgb(0, 0, 0), Color::from_rgb(12, 12, 12)),
    // 4  Red — Orc, danger messages
    (Color::from_rgb(0, 0, 0), Color::from_rgb(31, 0, 0)),
    // 5  DarkRed — future monsters
    (Color::from_rgb(0, 0, 0), Color::from_rgb(20, 0, 0)),
    // 6  Green — player '@', Goblin
    (Color::from_rgb(0, 0, 0), Color::from_rgb(0, 31, 0)),
    // 7  DarkGreen — Troll
    (Color::from_rgb(0, 0, 0), Color::from_rgb(0, 18, 0)),
    // 8  Yellow — items, warnings
    (Color::from_rgb(0, 0, 0), Color::from_rgb(31, 31, 0)),
    // 9  DarkBlue — spare
    (Color::from_rgb(0, 0, 0), Color::from_rgb(0, 0, 20)),
    // 10 Cyan — stairs '>'
    (Color::from_rgb(0, 0, 0), Color::from_rgb(0, 31, 31)),
    // 11 Dim — explored-but-not-visible
    (Color::from_rgb(0, 0, 0), Color::from_rgb(8, 8, 12)),
    // 12 StatusBar — white on dark blue
    (Color::from_rgb(0, 0, 14), Color::from_rgb(31, 31, 31)),
    // 13 MsgLog — light grey on black
    (Color::from_rgb(0, 0, 0), Color::from_rgb(28, 28, 28)),
    // 14 Highlight — inverse video (black on yellow)
    (Color::from_rgb(31, 31, 0), Color::from_rgb(0, 0, 0)),
];

/// Write all palette entries to BG_PALETTE MMIO.
pub fn init_palette() {
    BACKDROP_COLOR.write(Color::from_rgb(0, 0, 0));

    for (bank, &(bg, fg)) in PALETTE_ENTRIES.iter().enumerate() {
        let palbank = bg_palbank(bank);
        palbank.index(0).write(bg);
        palbank.index(1).write(fg);
    }
}

/// Linearly interpolate between two RGB555 colors.
/// `t` ranges from 0 (= color `a`) to 31 (= color `b`). All math in u32.
pub fn lerp_color(a: Color, b: Color, t: u32) -> Color {
    let a0 = a.0 as u32;
    let b0 = b.0 as u32;
    let ar = a0 & 0x1F;
    let ag = (a0 >> 5) & 0x1F;
    let ab = (a0 >> 10) & 0x1F;
    let br = b0 & 0x1F;
    let bg = (b0 >> 5) & 0x1F;
    let bb = (b0 >> 10) & 0x1F;
    let r = (ar * (31 - t) + br * t) / 31;
    let g = (ag * (31 - t) + bg * t) / 31;
    let b_ch = (ab * (31 - t) + bb * t) / 31;
    Color::from_rgb(r as u16, g as u16, b_ch as u16)
}

/// Convert a `GameColor` + visibility state to a palbank index.
///
/// For visible tiles, the palbank is the `GameColor` discriminant directly.
/// For explored-but-not-visible tiles, returns `PALBANK_DIM`.
pub fn game_color_to_palbank(color: GameColor, visible: bool) -> u16 {
    if visible {
        color as u16
    } else {
        PALBANK_DIM
    }
}
