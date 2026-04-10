/// 8×8 bitmap font for glyph-textured billboards.
///
/// Each glyph is 8 bytes — one byte per row, MSB on the left.
/// Bit set = foreground (entity color), bit clear = transparent (skip pixel).
/// Glyphs are designed for readability at low resolution (~10-20 screen pixels).
pub type Glyph = [u8; 8];

/// Look up the 8×8 bitmap for a character. Unknown characters get a filled square.
pub const fn glyph(ch: char) -> Glyph {
    match ch {
        '@' => [
            0b_0011_1100,
            0b_0100_0010,
            0b_0101_1010,
            0b_0110_0010,
            0b_0101_1110,
            0b_0100_0000,
            0b_0011_1100,
            0b_0000_0000,
        ],
        'g' => [
            0b_0000_0000,
            0b_0000_0000,
            0b_0011_1110,
            0b_0100_0010,
            0b_0100_0010,
            0b_0011_1110,
            0b_0000_0010,
            0b_0011_1100,
        ],
        'o' => [
            0b_0000_0000,
            0b_0000_0000,
            0b_0011_1100,
            0b_0100_0010,
            0b_0100_0010,
            0b_0100_0010,
            0b_0011_1100,
            0b_0000_0000,
        ],
        'T' => [
            0b_0111_1110,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0000_0000,
        ],
        '!' => [
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0001_1000,
            0b_0000_0000,
            0b_0001_1000,
            0b_0000_0000,
        ],
        '/' => [
            0b_0000_0010,
            0b_0000_0100,
            0b_0000_1000,
            0b_0001_0000,
            0b_0010_0000,
            0b_0100_0000,
            0b_1000_0000,
            0b_0000_0000,
        ],
        '[' => [
            0b_0011_1100,
            0b_0011_0000,
            0b_0011_0000,
            0b_0011_0000,
            0b_0011_0000,
            0b_0011_0000,
            0b_0011_1100,
            0b_0000_0000,
        ],
        '>' => [
            0b_0000_0000,
            0b_0110_0000,
            0b_0001_1000,
            0b_0000_0110,
            0b_0001_1000,
            0b_0110_0000,
            0b_0000_0000,
            0b_0000_0000,
        ],
        '%' => [
            0b_0110_0010,
            0b_0110_0100,
            0b_0000_1000,
            0b_0001_0000,
            0b_0010_0000,
            0b_0100_0110,
            0b_1000_0110,
            0b_0000_0000,
        ],
        '.' => [
            0b_0000_0000,
            0b_0000_0000,
            0b_0000_0000,
            0b_0000_0000,
            0b_0000_0000,
            0b_0000_0000,
            0b_0001_1000,
            0b_0000_0000,
        ],
        _ => [
            // Unknown: filled square (visible placeholder)
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
            0b_1111_1111,
        ],
    }
}

/// Test whether a specific texel in a glyph is set (foreground).
/// `u`: column 0..7 (0 = left), `v`: row 0..7 (0 = top).
#[inline]
pub const fn texel(glyph: &Glyph, u: u8, v: u8) -> bool {
    glyph[v as usize] & (0x80 >> u) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_sign_has_content() {
        let g = glyph('@');
        // Should have some set bits (not blank)
        let total: u32 = g.iter().map(|row| row.count_ones()).sum();
        assert!(total > 10, "@ glyph should have substantial content");
    }

    #[test]
    fn unknown_is_filled() {
        let g = glyph('\u{FFFF}');
        assert!(
            g.iter().all(|&row| row == 0xFF),
            "unknown glyph should be filled"
        );
    }

    #[test]
    fn texel_lookup() {
        let g = glyph('!');
        // Top row of '!' has bits at positions 3,4 (0b_0001_1000)
        assert!(!texel(&g, 0, 0)); // leftmost — clear
        assert!(texel(&g, 3, 0)); // set
        assert!(texel(&g, 4, 0)); // set
        assert!(!texel(&g, 7, 0)); // rightmost — clear
    }

    #[test]
    fn exclamation_gap_row() {
        let g = glyph('!');
        // Row 5 (0-indexed) is the gap: 0b_0000_0000
        assert_eq!(g[5], 0, "! should have a gap before the dot");
    }
}
