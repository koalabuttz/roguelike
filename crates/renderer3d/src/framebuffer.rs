/// Pack 5-bit RGB channels into u16 RGB555 format.
/// Input channels are masked to 0..31.
#[inline]
pub const fn rgb555(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0x1F) << 10) | ((g as u16 & 0x1F) << 5) | (b as u16 & 0x1F)
}

/// Unpack RGB555 to 8-bit channels (0-255).
/// Uses standard 5-to-8 bit expansion: `(val << 3) | (val >> 2)` for full range.
#[inline]
pub const fn unpack_rgb555(c: u16) -> (u8, u8, u8) {
    let r5 = ((c >> 10) & 0x1F) as u8;
    let g5 = ((c >> 5) & 0x1F) as u8;
    let b5 = (c & 0x1F) as u8;
    (
        (r5 << 3) | (r5 >> 2),
        (g5 << 3) | (g5 >> 2),
        (b5 << 3) | (b5 >> 2),
    )
}

/// Pixel buffer with RGB555 color and i16 depth (z-buffer).
///
/// 4 bytes per pixel total (2 color + 2 depth). At 1080p that's ~8 MB;
/// at 256x192 (DS) it's ~192 KB.
#[cfg(feature = "std")]
pub struct Framebuffer {
    width: u32,
    height: u32,
    color: Vec<u16>,
    depth: Vec<i16>,
}

#[cfg(feature = "std")]
impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height) as usize;
        Self {
            width,
            height,
            color: vec![0u16; size],
            depth: vec![i16::MAX; size],
        }
    }

    /// Fill both buffers. Typical: `clear(0x0000, i16::MAX)` for black + far depth.
    pub fn clear(&mut self, color: u16, depth: i16) {
        self.color.fill(color);
        self.depth.fill(depth);
    }

    /// Write a pixel if it passes the z-buffer test (closer or equal depth wins).
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u16, depth: i16) {
        debug_assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) out of bounds"
        );
        let idx = self.index(x, y);
        if depth <= self.depth[idx] {
            self.color[idx] = color;
            self.depth[idx] = depth;
        }
    }

    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> u16 {
        debug_assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) out of bounds"
        );
        self.color[self.index(x, y)]
    }

    #[inline]
    pub fn get_depth(&self, x: u32, y: u32) -> i16 {
        debug_assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) out of bounds"
        );
        self.depth[self.index(x, y)]
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb555_known_values() {
        assert_eq!(rgb555(0, 0, 0), 0x0000);
        assert_eq!(rgb555(31, 31, 31), 0x7FFF);
        assert_eq!(rgb555(31, 0, 0), 0x7C00); // red only
        assert_eq!(rgb555(0, 31, 0), 0x03E0); // green only
        assert_eq!(rgb555(0, 0, 31), 0x001F); // blue only
    }

    #[test]
    fn rgb555_masks_overflow() {
        // Inputs > 31 get masked to 5 bits
        assert_eq!(rgb555(255, 255, 255), rgb555(31, 31, 31));
    }

    #[test]
    fn unpack_rgb555_roundtrip() {
        let packed = rgb555(31, 0, 15);
        let (r, g, b) = unpack_rgb555(packed);
        assert_eq!(r, 255); // 31 → 255
        assert_eq!(g, 0); //  0 → 0
        assert_eq!(b, 123); // 15 → (15<<3)|(15>>2) = 120+3 = 123
    }

    #[test]
    fn unpack_full_white() {
        let (r, g, b) = unpack_rgb555(0x7FFF);
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn unpack_black() {
        let (r, g, b) = unpack_rgb555(0x0000);
        assert_eq!((r, g, b), (0, 0, 0));
    }

    #[test]
    fn new_dimensions() {
        let fb = Framebuffer::new(10, 8);
        assert_eq!(fb.width(), 10);
        assert_eq!(fb.height(), 8);
    }

    #[test]
    fn clear_fills_buffers() {
        let mut fb = Framebuffer::new(10, 8);
        let red = rgb555(31, 0, 0);
        fb.clear(red, 500);
        assert_eq!(fb.get_pixel(0, 0), red);
        assert_eq!(fb.get_pixel(9, 7), red);
        assert_eq!(fb.get_depth(0, 0), 500);
        assert_eq!(fb.get_depth(9, 7), 500);
    }

    #[test]
    fn set_pixel_writes() {
        let mut fb = Framebuffer::new(10, 8);
        let green = rgb555(0, 31, 0);
        fb.set_pixel(5, 3, green, 0);
        assert_eq!(fb.get_pixel(5, 3), green);
    }

    #[test]
    fn zbuffer_closer_wins() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);
        let green = rgb555(0, 31, 0);
        let blue = rgb555(0, 0, 31);

        // Write far pixel
        fb.set_pixel(5, 5, red, 100);
        assert_eq!(fb.get_pixel(5, 5), red);

        // Closer pixel overwrites
        fb.set_pixel(5, 5, green, 50);
        assert_eq!(fb.get_pixel(5, 5), green);

        // Farther pixel does NOT overwrite
        fb.set_pixel(5, 5, blue, 200);
        assert_eq!(fb.get_pixel(5, 5), green);
        assert_eq!(fb.get_depth(5, 5), 50);
    }

    #[test]
    fn zbuffer_equal_depth_overwrites() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);
        let green = rgb555(0, 31, 0);

        fb.set_pixel(5, 5, red, 50);
        fb.set_pixel(5, 5, green, 50);
        assert_eq!(fb.get_pixel(5, 5), green);
    }
}
