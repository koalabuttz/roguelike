//! GBA display initialization and tile writing helpers.
//!
//! Mode 0, two text backgrounds:
//! - BG0 (priority 1, screenblock 30): map viewport
//! - BG1 (priority 0, screenblock 31): HUD overlay (status bar + messages)
//!
//! Both share charblock 0 (CP437 font, 4bpp).

use gba::prelude::*;

use crate::palette;

/// Screenblock index for BG0 (map).
const MAP_SCREENBLOCK: u16 = 30;
/// Screenblock index for BG1 (HUD).
const HUD_SCREENBLOCK: u16 = 31;

/// Visible screen width in tiles.
pub const SCREEN_COLS: usize = 30;
/// Visible screen height in tiles.
pub const SCREEN_ROWS: usize = 20;
/// Map viewport height in tile rows.
pub const MAP_ROWS: usize = 17;
/// Status bar row on the HUD layer.
pub const STATUS_ROW: usize = 17;
/// First message log row on the HUD layer.
pub const MSG_ROW: usize = 18;
/// Number of message log rows.
pub const MSG_LINES: usize = 2;

/// One-time display setup: load font, configure BG layers, enable video.
pub fn init_display() {
    // Load CP437 font into charblock 0 as 4bpp tiles.
    // The BIOS BitUnPack SWI decompresses 1bpp → 4bpp in-place.
    Cga8x8Thick.bitunpack_4bpp(CHARBLOCK0_4BPP.as_region(), 0);

    // Initialize palette (all 15 palbanks).
    palette::init_palette();

    // BG0: map layer — charblock 0, screenblock 30, priority 1 (behind HUD)
    BG0CNT.write(
        BackgroundControl::new()
            .with_charblock(0)
            .with_screenblock(MAP_SCREENBLOCK)
            .with_priority(1),
    );

    // BG1: HUD layer — charblock 0, screenblock 31, priority 0 (in front)
    BG1CNT.write(
        BackgroundControl::new()
            .with_charblock(0)
            .with_screenblock(HUD_SCREENBLOCK)
            .with_priority(0),
    );

    // Clear both screenblocks to transparent (tile 0, palbank 0).
    let empty = TextEntry::new();
    for sb_idx in [MAP_SCREENBLOCK, HUD_SCREENBLOCK] {
        let sb = TEXT_SCREENBLOCKS.get_frame(sb_idx as usize).unwrap();
        for row in 0..32 {
            let r = sb.get_row(row).unwrap();
            for col in 0..32 {
                r.index(col).write(empty);
            }
        }
    }

    // Enable Mode 0 with BG0 + BG1 visible.
    DISPCNT.write(
        DisplayControl::new()
            .with_show_bg0(true)
            .with_show_bg1(true),
    );
}

/// Write a single tile to the map layer (BG0, screenblock 30).
pub fn write_map_tile(x: usize, y: usize, glyph: u8, palbank: u16) {
    let sb = TEXT_SCREENBLOCKS.get_frame(MAP_SCREENBLOCK as usize).unwrap();
    let entry = TextEntry::new()
        .with_tile(glyph as u16)
        .with_palbank(palbank);
    sb.get_row(y).unwrap().index(x).write(entry);
}

/// Write a single tile to the HUD layer (BG1, screenblock 31).
pub fn write_hud_tile(x: usize, y: usize, glyph: u8, palbank: u16) {
    let sb = TEXT_SCREENBLOCKS.get_frame(HUD_SCREENBLOCK as usize).unwrap();
    let entry = TextEntry::new()
        .with_tile(glyph as u16)
        .with_palbank(palbank);
    sb.get_row(y).unwrap().index(x).write(entry);
}

/// Write an ASCII string to the HUD layer starting at (x, y).
/// Truncates if the string exceeds the remaining columns.
pub fn write_hud_string(x: usize, y: usize, s: &str, palbank: u16) {
    let sb = TEXT_SCREENBLOCKS.get_frame(HUD_SCREENBLOCK as usize).unwrap();
    let row = sb.get_row(y).unwrap();
    for (i, byte) in s.bytes().enumerate() {
        let col = x + i;
        if col >= 32 {
            break;
        }
        let entry = TextEntry::new()
            .with_tile(byte as u16)
            .with_palbank(palbank);
        row.index(col).write(entry);
    }
}

/// Write an ASCII string to the map layer starting at (x, y).
pub fn write_map_string(x: usize, y: usize, s: &str, palbank: u16) {
    let sb = TEXT_SCREENBLOCKS.get_frame(MAP_SCREENBLOCK as usize).unwrap();
    let row = sb.get_row(y).unwrap();
    for (i, byte) in s.bytes().enumerate() {
        let col = x + i;
        if col >= 32 {
            break;
        }
        let entry = TextEntry::new()
            .with_tile(byte as u16)
            .with_palbank(palbank);
        row.index(col).write(entry);
    }
}

/// Clear the entire HUD layer (BG1) to transparent tiles.
pub fn clear_hud() {
    let sb = TEXT_SCREENBLOCKS.get_frame(HUD_SCREENBLOCK as usize).unwrap();
    let empty = TextEntry::new();
    for y in 0..SCREEN_ROWS {
        let row = sb.get_row(y).unwrap();
        for x in 0..32 {
            row.index(x).write(empty);
        }
    }
}
