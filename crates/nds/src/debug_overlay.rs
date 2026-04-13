//! Engine A BG1 text overlay for debug info on the top (3D) screen.
//!
//! Uses the same font as Engine B (via `debug_hud::upload_font_to`) but
//! writes to Engine A's BG1 layer, which composites on top of the 3D
//! BG0 output. Transparent pixels (palette index 0) show the 3D scene
//! underneath.
//!
//! ## Priority
//!
//! BG0 (3D) is set to priority 1 in main.rs. BG1 (this overlay) is at
//! priority 0 (highest), so text draws on top of the 3D scene. Empty
//! tiles are palette index 0 = transparent, letting the 3D show through.
//!
//! Clean-room DS hardware configuration from GBATEK:
//!   GBATEK §DS Video Registers (DISPCNT, BG1CNT, palette RAM)
//!   GBATEK §DS VRAM Mapping

use crate::debug_hud;

/// Engine A BG1 control register (GBATEK §DS Video BG Control).
const REG_BG1CNT: *mut u16 = 0x0400_000A as *mut u16;

/// Engine A BG memory base (bank A mapped via VRAMCNT_A = 0x81).
const BG_A_BASE: usize = 0x0600_0000;

/// Character data base for BG1 (cbb 0): tile graphics.
const CHAR_BASE_A: *mut u16 = BG_A_BASE as *mut u16;

/// Map base for BG1 (mbb 8 = 16 KB offset): 32x32 tilemap.
const MAP_BASE_A: *mut u16 = (BG_A_BASE + 0x4000) as *mut u16;

/// Engine A palette RAM (separate from Engine B at 0x05000400).
const PALETTE_A: *mut u16 = 0x0500_0000 as *mut u16;

/// BG1CNT: priority 0, character base 0, map base 8 (0x0800).
const BG1CNT_VALUE: u16 = 8 << 8;

/// Initialize Engine A BG1 as a text overlay on the 3D screen.
///
/// Must be called after `init_display()` (VRAMCNT_A configured) and
/// `gx::init()`. BG1 enable bit in DISPCNT is set by main.rs.
pub fn init() {
    unsafe {
        REG_BG1CNT.write_volatile(BG1CNT_VALUE);

        // Engine A palette bank 0: transparent bg + white fg.
        PALETTE_A.add(0).write_volatile(0x0000);
        PALETTE_A.add(1).write_volatile(0x7FFF);
    }

    // Upload the same font to Engine A's character base.
    debug_hud::upload_font_to(CHAR_BASE_A);
    clear_tilemap();
}

/// Clear the BG1 tilemap (fill with transparent space tiles).
fn clear_tilemap() {
    for i in 0..(32 * 32) {
        unsafe {
            MAP_BASE_A.add(i).write_volatile(0);
        }
    }
}

/// Write an ASCII byte string into the BG1 tilemap at `(col, row)`.
pub fn write_text(col: u8, row: u8, text: &[u8]) {
    if row >= 32 {
        return;
    }
    let row_offset = row as usize * 32;
    for (i, &byte) in text.iter().enumerate() {
        let col_idx = col as usize + i;
        if col_idx >= 32 {
            break;
        }
        let tile = debug_hud::ascii_to_tile(byte);
        unsafe {
            MAP_BASE_A
                .add(row_offset + col_idx)
                .write_volatile(tile);
        }
    }
}
