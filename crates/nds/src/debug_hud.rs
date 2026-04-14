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
//! A 1bpp 8×8 font covering: space, digits 0-9, A-Z, and common symbols
//! (`:`, `.`, `/`, `!`, `+`, `-`, `(`, `)`). Lowercase a-z maps to uppercase.
//! Each glyph is 8 bytes (1 byte per row, MSB = leftmost pixel). At init
//! time the font is converted to DS 4bpp tile format (32 bytes per tile:
//! 8 rows × 4 bytes per row, low nibble = left pixel, high nibble = right).
//!
//! ## Tile 0 = space, tiles 1-10 = digits, tiles 11-36 = A-Z, tiles 37-44 = symbols
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
const G_A: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_B: Glyph = [
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_C: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_D: Glyph = [
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_E: Glyph = [
    0b_1111_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_1000,
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

// --- New glyphs for full A-Z coverage + symbols ---

#[rustfmt::skip]
const G_G: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_0000,
    0b_1011_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_H: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_I: Glyph = [
    0b_0111_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_J: Glyph = [
    0b_0011_1000,
    0b_0001_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_1001_0000,
    0b_0110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_K: Glyph = [
    0b_1000_1000,
    0b_1001_0000,
    0b_1010_0000,
    0b_1100_0000,
    0b_1010_0000,
    0b_1001_0000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_L: Glyph = [
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_N: Glyph = [
    0b_1000_1000,
    0b_1100_1000,
    0b_1010_1000,
    0b_1001_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_O: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_Q: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1010_1000,
    0b_1001_0000,
    0b_0110_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_R: Glyph = [
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_1010_0000,
    0b_1001_0000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_T: Glyph = [
    0b_1111_1000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_U: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_V: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0101_0000,
    0b_0101_0000,
    0b_0010_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_W: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1010_1000,
    0b_1010_1000,
    0b_1101_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_X: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_0101_0000,
    0b_0010_0000,
    0b_0101_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_Y: Glyph = [
    0b_1000_1000,
    0b_1000_1000,
    0b_0101_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_Z: Glyph = [
    0b_1111_1000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_1000_0000,
    0b_1111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_SLASH: Glyph = [
    0b_0000_0000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_1000_0000,
    0b_0000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_EXCL: Glyph = [
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
    0b_0010_0000,
    0b_0000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_PLUS: Glyph = [
    0b_0000_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_1111_1000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_DASH: Glyph = [
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_1111_1000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LPAREN: Glyph = [
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0010_0000,
    0b_0001_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_RPAREN: Glyph = [
    0b_0100_0000,
    0b_0010_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_0000_0000,
];

// --- Additional symbol glyphs for automap ---

#[rustfmt::skip]
const G_HASH: Glyph = [
    0b_0101_0000,
    0b_0101_0000,
    0b_1111_1000,
    0b_0101_0000,
    0b_1111_1000,
    0b_0101_0000,
    0b_0101_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_GT: Glyph = [
    0b_0100_0000,
    0b_0010_0000,
    0b_0001_0000,
    0b_0000_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_AT: Glyph = [
    0b_0111_0000,
    0b_1000_1000,
    0b_1010_1000,
    0b_1011_1000,
    0b_1010_0000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LBRACKET: Glyph = [
    0b_0111_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_PERCENT: Glyph = [
    0b_1100_0000,
    0b_1100_1000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_1001_1000,
    0b_0001_1000,
    0b_0000_0000,
];

// --- Lowercase glyphs: full-height, visually distinct from uppercase ---
// Design strategy: structural differences where possible (a vs A),
// narrower forms for inherently similar shapes (o vs O).

#[rustfmt::skip]
const G_LA: Glyph = [  // round bowl (A = pointed peak)
    0b_0111_0000,
    0b_1000_1000,
    0b_0000_1000,
    0b_0111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LB: Glyph = [  // left stem + right bowl (B = double bowls)
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LC: Glyph = [  // narrower C (3px vs 5px wide)
    0b_0011_0000,
    0b_0100_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0100_0000,
    0b_0011_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LD: Glyph = [  // mirror of b (right stem + left bowl)
    0b_0000_1000,
    0b_0000_1000,
    0b_0111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LE: Glyph = [  // round + crossbar (E = three parallel bars)
    0b_0111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LF: Glyph = [  // hooked top + low crossbar (F = straight + high bar)
    0b_0011_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_1111_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LG: Glyph = [  // bowl + descender (G = horizontal spur)
    0b_0111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LH: Glyph = [  // left stem + arch (H = two stems + crossbar)
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LI: Glyph = [  // dot + thin column (I = serif column)
    0b_0010_0000,
    0b_0000_0000,
    0b_0110_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LJ: Glyph = [  // dot + hook (J = full hook column)
    0b_0001_0000,
    0b_0000_0000,
    0b_0011_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_1001_0000,
    0b_0110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LK: Glyph = [  // left stem + kicks (K = centered diagonals)
    0b_1000_0000,
    0b_1000_0000,
    0b_1001_0000,
    0b_1010_0000,
    0b_1100_0000,
    0b_1010_0000,
    0b_1001_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LL: Glyph = [  // thin column + base (L = right angle)
    0b_0110_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0010_0000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LM: Glyph = [  // triple arch (M = pointed peaks)
    0b_1101_0000,
    0b_1010_1000,
    0b_1010_1000,
    0b_1010_1000,
    0b_1010_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LN: Glyph = [  // arch (N = diagonal stroke)
    0b_1011_0000,
    0b_1100_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LO: Glyph = [  // narrower oval (O = 5px wide, o = 4px wide)
    0b_0110_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_0110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LP: Glyph = [  // stem up, bowl down (P = bowl on top)
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LQ: Glyph = [  // mirror of p (Q = circle + diagonal tail)
    0b_0000_1000,
    0b_0000_1000,
    0b_0000_1000,
    0b_0111_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LR: Glyph = [  // stub arch (R = full diagonal leg)
    0b_1011_0000,
    0b_1100_1000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LS: Glyph = [  // narrower S (4px vs 5px wide)
    0b_0110_0000,
    0b_1000_0000,
    0b_1000_0000,
    0b_0110_0000,
    0b_0001_0000,
    0b_0001_0000,
    0b_1110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LT: Glyph = [  // column + crossbar (T = top-heavy bar)
    0b_0100_0000,
    0b_0100_0000,
    0b_1111_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_0011_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LU: Glyph = [  // bowl + tail (U = symmetric bowl)
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1001_1000,
    0b_0110_1000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LV: Glyph = [  // narrower V (4px wide)
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_0110_0000,
    0b_0110_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LW: Glyph = [  // converging bottom (W = diverging bottom)
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_1010_1000,
    0b_1010_1000,
    0b_1010_1000,
    0b_0101_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LX: Glyph = [  // narrower crossing (4px wide)
    0b_1001_0000,
    0b_1001_0000,
    0b_0110_0000,
    0b_0110_0000,
    0b_0110_0000,
    0b_1001_0000,
    0b_1001_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LY: Glyph = [  // fork + descender tail (Y = symmetric fork)
    0b_1000_1000,
    0b_1000_1000,
    0b_1000_1000,
    0b_0111_1000,
    0b_0000_1000,
    0b_1000_1000,
    0b_0111_0000,
    0b_0000_0000,
];

#[rustfmt::skip]
const G_LZ: Glyph = [  // narrower zigzag (4px vs 5px wide)
    0b_1111_0000,
    0b_0001_0000,
    0b_0010_0000,
    0b_0100_0000,
    0b_0100_0000,
    0b_1000_0000,
    0b_1111_0000,
    0b_0000_0000,
];

/// Font ordered so tile index matches [`ascii_to_tile`].
/// Tiles 0 = space, 1-10 = digits, 11-36 = A-Z, 37-44 = symbols,
/// 45-49 = extra symbols (#>@[%), 50-75 = lowercase a-z.
const FONT: [Glyph; 76] = [
    G_SPACE,    // 0
    G_0,        // 1
    G_1,        // 2
    G_2,        // 3
    G_3,        // 4
    G_4,        // 5
    G_5,        // 6
    G_6,        // 7
    G_7,        // 8
    G_8,        // 9
    G_9,        // 10
    G_A,        // 11
    G_B,        // 12
    G_C,        // 13
    G_D,        // 14
    G_E,        // 15
    G_F,        // 16
    G_G,        // 17
    G_H,        // 18
    G_I,        // 19
    G_J,        // 20
    G_K,        // 21
    G_L,        // 22
    G_M,        // 23
    G_N,        // 24
    G_O,        // 25
    G_P,        // 26
    G_Q,        // 27
    G_R,        // 28
    G_S,        // 29
    G_T,        // 30
    G_U,        // 31
    G_V,        // 32
    G_W,        // 33
    G_X,        // 34
    G_Y,        // 35
    G_Z,        // 36
    G_COLON,    // 37
    G_DOT,      // 38
    G_SLASH,    // 39
    G_EXCL,     // 40
    G_PLUS,     // 41
    G_DASH,     // 42
    G_LPAREN,   // 43
    G_RPAREN,   // 44
    G_HASH,     // 45
    G_GT,       // 46
    G_AT,       // 47
    G_LBRACKET, // 48
    G_PERCENT,  // 49
    G_LA,       // 50
    G_LB,       // 51
    G_LC,       // 52
    G_LD,       // 53
    G_LE,       // 54
    G_LF,       // 55
    G_LG,       // 56
    G_LH,       // 57
    G_LI,       // 58
    G_LJ,       // 59
    G_LK,       // 60
    G_LL,       // 61
    G_LM,       // 62
    G_LN,       // 63
    G_LO,       // 64
    G_LP,       // 65
    G_LQ,       // 66
    G_LR,       // 67
    G_LS,       // 68
    G_LT,       // 69
    G_LU,       // 70
    G_LV,       // 71
    G_LW,       // 72
    G_LX,       // 73
    G_LY,       // 74
    G_LZ,       // 75
];

/// Map an ASCII byte to the tile index in [`FONT`].
/// Unknown characters fall back to the space tile.
#[inline]
pub(crate) fn ascii_to_tile(c: u8) -> u16 {
    match c {
        b' ' => 0,
        b'0'..=b'9' => 1 + (c - b'0') as u16,
        b'A'..=b'Z' => 11 + (c - b'A') as u16,
        b'a'..=b'z' => 50 + (c - b'a') as u16,
        b':' => 37,
        b'.' => 38,
        b'/' => 39,
        b'!' => 40,
        b'+' => 41,
        b'-' => 42,
        b'(' => 43,
        b')' => 44,
        b'#' => 45,
        b'>' => 46,
        b'@' => 47,
        b'[' => 48,
        b'%' => 49,
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

/// Write the full font into a VRAM character base as 4bpp tiles.
///
/// Each tile is 32 bytes (8 rows × 4 bytes) = 16 u16 words (8 rows × 2
/// words). We emit the data as `u16` writes because ARM9 u8 writes are
/// silently dropped when a VRAM bank is mapped as BG memory.
///
/// `char_base` is the VRAM address to write to — Engine B uses
/// `CHAR_BASE` (0x06200000), Engine A overlay uses 0x06000000.
pub(crate) fn upload_font_to(char_base: *mut u16) {
    for (tile_idx, glyph) in FONT.iter().enumerate() {
        // 16 u16 words per tile, sequential.
        let tile_word_base = unsafe { char_base.add(tile_idx * 16) };
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

fn upload_font() {
    upload_font_to(CHAR_BASE);
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
    write_text_pal(col, row, text, 0);
}

/// Write an ASCII byte string with a palette bank into the tilemap at `(col, row)`.
/// `pal` selects the 16-color palette bank (0-15) encoded in tilemap bits 12-15.
pub fn write_text_pal(col: u8, row: u8, text: &[u8], pal: u16) {
    if row >= 32 {
        return;
    }
    let row_offset = row as usize * 32;
    let pal_bits = (pal & 0xF) << 12;
    for (i, &byte) in text.iter().enumerate() {
        let col_idx = col as usize + i;
        if col_idx >= 32 {
            break;
        }
        let tile = ascii_to_tile(byte);
        unsafe {
            MAP_BASE
                .add(row_offset + col_idx)
                .write_volatile(tile | pal_bits);
        }
    }
}

/// Write a single tilemap entry with explicit tile index and palette bank.
///
/// Used by the automap renderer which computes tile indices and palette
/// banks from game state rather than from ASCII text.
pub(crate) fn write_tile_pal(col: u8, row: u8, tile: u16, pal: u16) {
    if row >= 32 || col >= 32 {
        return;
    }
    let offset = row as usize * 32 + col as usize;
    let entry = (tile & 0x3FF) | ((pal & 0xF) << 12);
    unsafe {
        MAP_BASE.add(offset).write_volatile(entry);
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

        // Engine B palette banks. Each bank is 16 colors; we use index 0
        // (transparent/black) and index 1 (foreground) in our 1bpp font.
        // DS BGR555: bit15 unused | B[4:0]<<10 | G[4:0]<<5 | R[4:0].
        // Bank 0: white (debug text)
        PALETTE_B.add(0).write_volatile(0x0000);
        PALETTE_B.add(1).write_volatile(0x7FFF);
        // Bank 1: green (status bar)
        PALETTE_B.add(16).write_volatile(0x0000);
        PALETTE_B.add(17).write_volatile(0x03E0);
        // Bank 2: yellow (messages)
        PALETTE_B.add(32).write_volatile(0x0000);
        PALETTE_B.add(33).write_volatile(0x03FF);
        // Bank 3: dark grey (floor tiles)
        PALETTE_B.add(48).write_volatile(0x0000);
        PALETTE_B.add(49).write_volatile(0x294A);
        // Bank 4: cyan (stairs, short sword)
        PALETTE_B.add(64).write_volatile(0x0000);
        PALETTE_B.add(65).write_volatile(0x7FE0);
        // Bank 5: red (orc, health potion)
        PALETTE_B.add(80).write_volatile(0x0000);
        PALETTE_B.add(81).write_volatile(0x001F);
        // Bank 6: dark green (troll)
        PALETTE_B.add(96).write_volatile(0x0000);
        PALETTE_B.add(97).write_volatile(0x0200);
        // Bank 7: grey (chain mail)
        PALETTE_B.add(112).write_volatile(0x0000);
        PALETTE_B.add(113).write_volatile(0x5294);
        // Bank 8: dark red (corpse, greater health potion)
        PALETTE_B.add(128).write_volatile(0x0000);
        PALETTE_B.add(129).write_volatile(0x0010);
        // Bank 9: dim explored (dark blue-grey, ~RGB(40,40,50))
        PALETTE_B.add(144).write_volatile(0x0000);
        PALETTE_B.add(145).write_volatile(0x18A5);
        // Bank 10: bright cyan (touch button bar)
        PALETTE_B.add(160).write_volatile(0x0000);
        PALETTE_B.add(161).write_volatile(0x7FE8); // ~RGB(8,31,31)
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

/// Write a u16 as 4-digit uppercase hex into `buf` starting at `pos`.
/// Returns the new position (pos + 4).
#[allow(dead_code)] // used only in hardware-3D path (fog HUD)
pub fn write_u16_hex(buf: &mut [u8], pos: usize, val: u16) -> usize {
    const HEX: [u8; 16] = *b"0123456789ABCDEF";
    let mut p = pos;
    for shift in [12, 8, 4, 0] {
        if p < buf.len() {
            buf[p] = HEX[((val >> shift) & 0xF) as usize];
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
