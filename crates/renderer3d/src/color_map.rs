use roguelike_core::rules::color::GameColor;

use crate::framebuffer::rgb555;

/// Which face of a surface this triangle belongs to.
/// Shading models a point light at the player's position (ground level):
/// vertical wall sides facing the player are brightest, floors moderate,
/// wall tops dimmest (lit from below at a steep angle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// Floor surface (y=0) — moderate brightness.
    Floor,
    /// Wall top surface (y=WALL_HEIGHT) — dimmest (lit indirectly from below).
    WallTop,
    /// Vertical wall side facing the camera — full brightness (light hits head-on).
    Side,
}

/// Map a GameColor to an RGB555 u16 value.
pub const fn game_color_to_rgb555(color: GameColor) -> u16 {
    match color {
        GameColor::Black => rgb555(0, 0, 0),
        GameColor::White => rgb555(31, 31, 31),
        GameColor::Grey => rgb555(20, 20, 20),
        GameColor::DarkGrey => rgb555(10, 10, 10),
        GameColor::Red => rgb555(31, 0, 0),
        GameColor::DarkRed => rgb555(20, 0, 0),
        GameColor::Green => rgb555(0, 31, 0),
        GameColor::DarkGreen => rgb555(0, 20, 0),
        GameColor::Yellow => rgb555(31, 31, 0),
        GameColor::DarkBlue => rgb555(0, 0, 20),
        GameColor::Cyan => rgb555(0, 31, 31),
        #[cfg(feature = "std")]
        GameColor::Rgb(r, g, b) => rgb555(r >> 3, g >> 3, b >> 3),
    }
}

/// Darken an RGB555 color by right-shifting each 5-bit channel.
///
/// `shift=1` halves brightness, `shift=2` quarters it, etc.
/// Channels clamp to 0.
pub const fn darken_rgb555(color: u16, shift: u8) -> u16 {
    let r = (color >> 10) & 0x1F;
    let g = (color >> 5) & 0x1F;
    let b = color & 0x1F;
    ((r >> shift) << 10) | ((g >> shift) << 5) | (b >> shift)
}

/// Compute the final RGB555 color for a tile triangle.
///
/// Pipeline: GameColor → RGB555 base → face shade.
/// Wall sides (facing player) are brightest. Floors are 1 shift dimmer.
/// Wall tops are 2 shifts dimmer (lit from below at a steep angle).
pub fn tile_color(color: GameColor, face: Face) -> u16 {
    let base = game_color_to_rgb555(color);
    let face_shift = match face {
        Face::Side => 0,    // full brightness — light hits head-on
        Face::Floor => 1,   // moderate — horizontal surface, glancing light
        Face::WallTop => 2, // dim — top of wall, lit indirectly from below
    };
    darken_rgb555(base, face_shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_color_mappings() {
        assert_eq!(game_color_to_rgb555(GameColor::Black), 0x0000);
        assert_eq!(game_color_to_rgb555(GameColor::White), 0x7FFF);
        // Red: r=31, g=0, b=0 → (31 << 10) = 0x7C00
        assert_eq!(game_color_to_rgb555(GameColor::Red), 0x7C00);
        // Green: r=0, g=31, b=0 → (31 << 5) = 0x03E0
        assert_eq!(game_color_to_rgb555(GameColor::Green), 0x03E0);
        // Cyan: r=0, g=31, b=31 → 0x03E0 | 0x001F = 0x03FF
        assert_eq!(game_color_to_rgb555(GameColor::Cyan), 0x03FF);
    }

    #[test]
    fn darken_shifts_channels() {
        // White (31,31,31) darkened by 1 → (15,15,15)
        let white = rgb555(31, 31, 31);
        let dimmed = darken_rgb555(white, 1);
        assert_eq!(dimmed, rgb555(15, 15, 15));
    }

    #[test]
    fn darken_by_two() {
        // White darkened by 2 → (7,7,7)
        let white = rgb555(31, 31, 31);
        assert_eq!(darken_rgb555(white, 2), rgb555(7, 7, 7));
    }

    #[test]
    fn darken_clamps_to_zero() {
        // Already dim value darkened further
        let dim = rgb555(1, 1, 1);
        assert_eq!(darken_rgb555(dim, 1), rgb555(0, 0, 0));

        // Black stays black
        assert_eq!(darken_rgb555(0, 1), 0);
    }

    #[test]
    fn tile_color_face_shading() {
        // Side: brightest (0 shifts), Floor: 1 shift, WallTop: 2 shifts
        let side = tile_color(GameColor::White, Face::Side);
        let floor = tile_color(GameColor::White, Face::Floor);
        let wall_top = tile_color(GameColor::White, Face::WallTop);
        assert_eq!(side, rgb555(31, 31, 31));
        assert_eq!(floor, rgb555(15, 15, 15));
        assert_eq!(wall_top, rgb555(7, 7, 7));
    }

    #[test]
    fn asymmetric_color_darken() {
        // Red (31,0,0) darkened by 1 → (15,0,0)
        let red = rgb555(31, 0, 0);
        let dimmed = darken_rgb555(red, 1);
        assert_eq!(dimmed, rgb555(15, 0, 0));
    }
}
