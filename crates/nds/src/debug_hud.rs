//! Engine B tile-mode text HUD for debug display on the top screen.
//!
//! Clean-room DS hardware configuration from GBATEK:
//!   §DS Video Registers (DISPCNT_B, VRAMCNT_C, BG0CNT_B, palette RAM)
//!   §DS Video 2D Backgrounds / Tile Formats
//!
//! ## Layout
//!
//! - **VRAM bank C** is mapped as Engine B BG (`VRAMCNT_C = 0x84`, MST=4)
//! - Engine B BG memory lives at `0x0620_0000`
//! - **Character base 0**: tile graphics at `0x0620_0000` (16 KB region, holds
//!   up to 512 4bpp tiles)
//! - **Map base 8**: 32×32 tilemap at `0x0620_4000` (2 KB, each entry a u16)
//! - **Engine B palette**: 16-color bank 0 at `0x0500_0400`; only indices 0
//!   (transparent/black) and 1 (white) are used
//!
//! ## Font
//!
//! A minimal 1bpp 8×8 font covering: space, digits 0-9, F, P, S, M, ':', '.'
//! Each glyph is 8 bytes (1 byte per row, MSB = leftmost pixel). At init
//! time the font is converted to DS 4bpp tile format (32 bytes per tile:
//! 8 rows × 4 bytes per row, low nibble = left pixel, high nibble = right).
//!
//! ## Tile 0 = space, tile 1..=10 = digits, ...
//!
//! See [`ascii_to_tile`] for the full mapping.
//!
//! The module is standalone no_std and performs all hardware access via
//! volatile pointer writes.

// ---------------------------------------------------------------------------
// Engine B register addresses (GBATEK §DS Video Registers)
// ---------------------------------------------------------------------------

/// VRAM bank C control — bit 7 enable, bits 0-2 MST.
/// Value `0x84` = enable + MST 4 (Engine B BG at 0x06200000).
const VRAMCNT_C: *mut u8 = 0x0400_0242 as *mut u8;

/// Engine B display control. Same bit layout as Engine A DISPCNT.
const DISPCNT_B: *mut u32 = 0x0400_1000 as *mut u32;

/// Engine B BG0 control.
const BG0CNT_B: *mut u16 = 0x0400_1008 as *mut u16;

/// Engine B palette RAM. 16 banks × 16 colors for 4bpp tile mode, one u16
/// per color slot.
const PALETTE_B: *mut u16 = 0x0500_0400 as *mut u16;

/// Engine B BG memory base (after VRAMCNT_C = 0x84 maps bank C here).
const BG_B_BASE: usize = 0x0620_0000;

/// Character data base (cbb 0), accessed as u16 words — ARM9 writes to
/// BG VRAM must be 16-bit or 32-bit; u8 writes are silently dropped when
/// the bank is mapped as BG memory (as opposed to LCDC mode).
const CHAR_BASE: *mut u16 = BG_B_BASE as *mut u16;

/// Map base (mbb 8): 2 KB slot holding the 32×32 tilemap.
/// Located 8 × 2 KB = 16 KB into Engine B BG memory, past the char data.
const MAP_BASE: *mut u16 = (BG_B_BASE + 0x4000) as *mut u16;

// ---------------------------------------------------------------------------
// Configuration values
// ---------------------------------------------------------------------------

/// VRAMCNT_C = enable + MST 4 (Engine B BG) + offset 0.
const VRAMCNT_C_ENGINE_B_BG: u8 = 0x84;

/// DISPCNT_B = graphics display mode (bits 16-17 = 01) + BG0 enable (bit 8)
/// + BG mode 0 (bits 0-2 = 000).
const DISPCNT_B_VALUE: u32 = (1 << 16) | (1 << 8);

/// BG0CNT_B = priority 0, character base 0, 16-color tiles, map base 8,
/// screen size 0 (32×32).
const BG0CNT_B_VALUE: u16 = 8 << 8;

// ---------------------------------------------------------------------------
// Font data — 1bpp 8×8, MSB = leftmost pixel
// ---------------------------------------------------------------------------

type Glyph = [u8; 8];

/// Space: all empty.
const G_SPACE: Glyph = [0; 8];

#[rustfmt::skip]
const G_0: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1001_1000,
    0b_1010_1000,
    0b_1100_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_1: Glyph = [
    0b_0010_0000,
    0b_0110_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_2: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_1111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_3: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_0000_1000,
    0b_0011_0000,
    0b_0000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_4: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_1000,
    0b_0000_1000,
    0b_0000_1000,
    0b_0000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_5: Glyph = [
    0b_1111_1000,
    0b_1000_0000,
    0b_1111_0000,
    0b_0000_1000,
    0b_0000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_6: Glyph = [
    0b_0011_0000,
    0b_0100_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_7: Glyph = [
    0b_1111_1000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_8: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_9: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_F: Glyph = [
    0b_1111_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_P: Glyph = [
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_S: Glyph = [
    0b_0111_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0111_0000,
    0b_0000_1000,
    0b_0000_1000,
    0b_1111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_M: Glyph = [
    0b_1000_1000,
    0b_1101_1000,
    0b_1010_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_COLON: Glyph = [
    0b_0000_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_DOT: Glyph = [
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0110_0000,
    0b_0000_0000,
];

/// Font ordered so tile index matches [`ascii_to_tile`].
const FONT: [Glyph; 17] = [
    G_SPACE, // 0
    G_0,     // 1
    G_1,     // 2
    G_2,     // 3
    G_3,     // 4
    G_4,     // 5
    G_5,     // 6
    G_6,     // 7
    G_7,     // 8
    G_8,     // 9
    G_9,     // 10
    G_F,     // 11
    G_P,     // 12
    G_S,     // 13
    G_M,     // 14
    G_COLON, // 15
    G_DOT,   // 16
];

/// Map an ASCII byte to the tile index in [`FONT`].
/// Unknown characters fall back to the space tile.
#[inline]
fn ascii_to_tile(c: u8) -> u16 {
    match c {
        b' ' => 0,
        b'0'..=b'9' => 1 + (c - b'0') as u16,
        b'F' => 11,
        b'P' => 12,
        b'S' => 13,
        b'M' => 14,
        b':' => 15,
        b'.' => 16,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Font upload: 1bpp → 4bpp DS tile format
// ---------------------------------------------------------------------------

/// Convert one 1bpp row (MSB = leftmost pixel) into 4 bytes of DS 4bpp
/// tile data (low nibble = left pixel, high nibble = right pixel).
#[inline]
fn row_1bpp_to_4bpp(src: u8) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (pair, byte) in out.iter_mut().enumerate() {
        let left_bit = (src >> (7 - pair * 2)) & 1;
        let right_bit = (src >> (6 - pair * 2)) & 1;
        *byte = (right_bit << 4) | left_bit;
    }
    out
}

/// Write the full font into VRAM character base as 4bpp tiles.
///
/// Each tile is 32 bytes (8 rows × 4 bytes) = 16 u16 words (8 rows × 2
/// words). We emit the data as `u16` writes because ARM9 u8 writes are
/// silently dropped when a VRAM bank is mapped as BG memory.
fn upload_font() {
    for (tile_idx, glyph) in FONT.iter().enumerate() {
        // 16 u16 words per tile, sequential.
        let tile_word_base = unsafe { CHAR_BASE.add(tile_idx * 16) };
        for (row_idx, &row_byte) in glyph.iter().enumerate() {
            let row_4bpp = row_1bpp_to_4bpp(row_byte);
            // Pack two bytes per u16, little-endian (ARM9 is LE), so the
            // memory layout ends up as row_4bpp[0..4] at sequential bytes.
            let word0 = (row_4bpp[0] as u16) | ((row_4bpp[1] as u16) << 8);
            let word1 = (row_4bpp[2] as u16) | ((row_4bpp[3] as u16) << 8);
            unsafe {
                tile_word_base.add(row_idx * 2).write_volatile(word0);
                tile_word_base.add(row_idx * 2 + 1).write_volatile(word1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tilemap operations
// ---------------------------------------------------------------------------

/// Fill the 32×32 tilemap with the space tile (= 0).
fn clear_tilemap() {
    for i in 0..(32 * 32) {
        unsafe {
            MAP_BASE.add(i).write_volatile(0);
        }
    }
}

/// Write an ASCII byte string into the tilemap at `(col, row)`.
/// Text that runs off the right edge of the row is silently clipped.
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
        let tile = ascii_to_tile(byte);
        unsafe {
            MAP_BASE
                .add(row_offset + col_idx)
                .write_volatile(tile);
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Stand up Engine B with a tile-mode text HUD on BG0.
///
/// Must be called after `init_display()` (which powers on Engine B via
/// POWCNT1) and before any `write_text` call.
pub fn init() {
    unsafe {
        // Map VRAM bank C → Engine B BG at 0x06200000.
        VRAMCNT_C.write_volatile(VRAMCNT_C_ENGINE_B_BG);

        // Configure Engine B display + BG0.
        DISPCNT_B.write_volatile(DISPCNT_B_VALUE);
        BG0CNT_B.write_volatile(BG0CNT_B_VALUE);

        // Engine B palette bank 0: index 0 transparent/black, index 1 white.
        // RGB555 is symmetric for pure black and pure white so no swizzle
        // is needed.
        PALETTE_B.add(0).write_volatile(0x0000); // black (transparent)
        PALETTE_B.add(1).write_volatile(0x7FFF); // white
    }

    upload_font();
    clear_tilemap();
}

// ---------------------------------------------------------------------------
// No-std formatting helpers
// (adapted from crates/gba/src/format.rs — kept local to this module so
//  crates/nds has no dependency on the standalone GBA workspace)
// ---------------------------------------------------------------------------

/// Write a u32 as decimal ASCII into `buf` starting at `pos`.
/// Returns the new position after the digits. Silently stops if the
/// buffer fills up.
pub fn write_u32_dec(buf: &mut [u8], pos: usize, val: u32) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
        }
        return pos + 1;
    }

    // Extract digits in reverse (up to 10 digits for a u32).
    let mut digits = [0u8; 10];
    let mut n = val;
    let mut count = 0;
    while n > 0 {
        digits[count] = b'0' + (n % 10) as u8;
        n /= 10;
        count += 1;
    }

    let mut p = pos;
    for i in (0..count).rev() {
        if p < buf.len() {
            buf[p] = digits[i];
        }
        p += 1;
    }
    p
}

/// Write a `&str` (as bytes) into `buf` starting at `pos`.
/// Returns the new position after the string.
pub fn write_str(buf: &mut [u8], pos: usize, s: &[u8]) -> usize {
    let mut p = pos;
    for &byte in s {
        if p < buf.len() {
            buf[p] = byte;
        }
        p += 1;
    }
    p
}
