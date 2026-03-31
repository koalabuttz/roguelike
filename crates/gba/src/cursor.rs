//! Shared OAM sprite cursor for menus and modals.
//!
//! Uses OAM entry 0 with the CP437 ► glyph (0x10) in yellow.
//! All positioning is in tile coordinates — pixel conversion is internal.

use gba::prelude::*;

/// Cursor bob animation offsets (indexed by `(frame >> 2) & 7`).
const BOB: [u16; 8] = [0, 1, 2, 2, 2, 1, 0, 0];

/// CP437 glyph for right-pointing triangle.
const GLYPH: u16 = 0x10;

/// Initialize OAM sprite 0 as the menu cursor.
/// Hides all 128 OAM entries to prevent stale sprites, then loads the
/// cursor tile into OBJ VRAM and sets up the OBJ palette.
pub fn init() {
    // Copy cursor glyph from BG charblock 0 → OBJ tile 0.
    let src: [u32; 8] = CHARBLOCK0_4BPP.index(GLYPH as usize).read();
    OBJ_TILES.index(0).write(src);

    // OBJ palette bank 0: index 1 = yellow.
    obj_palbank(0).index(1).write(Color::from_rgb(31, 31, 0));

    // Hide all OAM entries to prevent stale sprites from previous screens.
    hide_all();

    // Enable OBJ layer with 1D tile mapping.
    let dc = DISPCNT.read();
    DISPCNT.write(dc.with_show_obj(true).with_obj_vram_1d(true));
}

/// Update cursor position. `col` and `row` are in **tile** coordinates.
/// Converts to pixels internally. `h_offset` is a pixel offset for slide
/// animations (subtracted from x).
pub fn update(col: usize, row: usize, frame: u16, h_offset: u16) {
    let bob = BOB[((frame >> 2) as usize) & 7];
    let px = (col as u16 * 8).wrapping_add(bob).wrapping_sub(h_offset);
    let py = row as u16 * 8;

    OBJ_ATTR0.index(0).write(ObjAttr0::new().with_y(py & 0xFF));
    OBJ_ATTR1.index(0).write(ObjAttr1::new().with_x(px & 0x1FF));
    OBJ_ATTR2.index(0).write(ObjAttr2::new().with_tile_id(0).with_palbank(0));
}

/// Hide OAM entry 0 (the cursor).
pub fn hide() {
    OBJ_ATTR0
        .index(0)
        .write(ObjAttr0::new().with_style(ObjDisplayStyle::NotDisplayed));
}

/// Disable the OBJ layer in DISPCNT. Call during cleanup.
pub fn disable_obj_layer() {
    let dc = DISPCNT.read();
    DISPCNT.write(dc.with_show_obj(false));
}

/// Hide all 128 OAM entries.
fn hide_all() {
    let not_displayed = ObjAttr0::new().with_style(ObjDisplayStyle::NotDisplayed);
    for i in 0..128 {
        OBJ_ATTR0.index(i).write(not_displayed);
    }
}
