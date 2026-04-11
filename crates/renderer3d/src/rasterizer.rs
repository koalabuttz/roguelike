use crate::framebuffer::Framebuffer;
use crate::pipeline::ScreenVertex;

/// 4×4 Bayer ordered dither matrix, pre-scaled to 0..240 (matching the 8-bit
/// fractional precision of our fog blending). This breaks fog banding into a
/// fine stipple pattern that the eye reads as a smooth gradient.
#[rustfmt::skip]
const BAYER_4X4: [u16; 16] = [
      0, 128,  32, 160,
    192,  64, 224,  96,
     48, 176,  16, 144,
    240, 112, 208,  80,
];

/// Fractional bits for the reciprocal of twice_area.
///
/// We precompute `(1 << RECIP_SHIFT) / twice_area` once per triangle, then
/// multiply per pixel and shift right. This replaces expensive i64 software
/// division (~100+ cycles on ARM9) with a single multiply (~4 cycles).
///
/// 16 bits gives 1/65536 precision — sufficient for our i16 depth/fog range.
const RECIP_SHIFT: u32 = 16;

/// Compute `(1 << RECIP_SHIFT) / twice_area` as a fixed-point reciprocal.
///
/// The caller guarantees `twice_area > 0`. The result is used as:
/// `(numerator * recip) >> RECIP_SHIFT` ≈ `numerator / twice_area`.
#[inline]
fn compute_reciprocal(twice_area: i64) -> i64 {
    ((1i64 << RECIP_SHIFT) + twice_area / 2) / twice_area
}

/// Apply a precomputed reciprocal: `(numerator * recip) >> RECIP_SHIFT`.
#[inline]
fn apply_recip(numerator: i64, recip: i64) -> i64 {
    (numerator * recip) >> RECIP_SHIFT
}

/// Blend an RGB555 color toward black by a fog factor, with ordered dithering
/// and per-channel light color tinting.
///
/// `fog`: 0 = clear, 256 = full black. `dither`: Bayer matrix value (0..240).
/// `light_color`: per-channel brightness multiplier `[r, g, b]` in 0..256
/// (256 = full white, lower = tinted). A warm torch might be `[256, 200, 100]`.
///
/// Each 5-bit channel is computed as: `surface × (256-fog) × light / 256²`,
/// with a dither offset added before the final truncation to 5 bits.
#[inline]
fn apply_fog(color: u16, fog: i16, dither: u16, light_color: [u16; 3]) -> u16 {
    if fog <= 0 && light_color[0] >= 256 && light_color[1] >= 256 && light_color[2] >= 256 {
        return color;
    }
    let inv = (256 - fog.min(256)) as u16;
    // Per-channel: brightness = inv × light_channel / 256
    let r_bright = (inv as u32 * light_color[0] as u32) >> 8;
    let g_bright = (inv as u32 * light_color[1] as u32) >> 8;
    let b_bright = (inv as u32 * light_color[2] as u32) >> 8;
    let r = (((((color >> 10) & 0x1F) as u32) * r_bright + dither as u32) >> 8).min(31) as u16;
    let g = (((((color >> 5) & 0x1F) as u32) * g_bright + dither as u32) >> 8).min(31) as u16;
    let b = ((((color & 0x1F) as u32) * b_bright + dither as u32) >> 8).min(31) as u16;
    (r << 10) | (g << 5) | b
}

/// Is this edge a "top" or "left" edge for the top-left fill rule?
/// Top: horizontal, going right (a == 0, b > 0).
/// Left: going up in screen space (a > 0).
#[inline]
fn is_top_left(a: i32, b: i32) -> bool {
    a > 0 || (a == 0 && b > 0)
}

/// Rasterize a flat-colored triangle with z-buffer depth testing.
///
/// Uses incremental edge function evaluation — 3 additions per pixel in the
/// inner loop. Depth is interpolated via barycentric coordinates (i64 to
/// avoid overflow at high resolutions).
///
/// Triangles with CCW winding are front-facing. CW or degenerate triangles
/// are culled.
pub fn rasterize_triangle(
    fb: &mut Framebuffer,
    v0: ScreenVertex,
    v1: ScreenVertex,
    v2: ScreenVertex,
    color: u16,
    light_color: [u16; 3],
) {
    // Back-face cull: twice-area via cross product. Positive = CCW = front-facing.
    let twice_area =
        (v1.x - v0.x) as i64 * (v2.y - v0.y) as i64 - (v2.x - v0.x) as i64 * (v1.y - v0.y) as i64;
    if twice_area <= 0 {
        return;
    }

    // Edge function coefficients: E_i(x,y) = A_i*x + B_i*y + C_i
    // Edge 0 (v1→v2, opposite v0): weight for v0
    let a0 = v1.y - v2.y;
    let b0 = v2.x - v1.x;
    let c0 = v1.x * v2.y - v2.x * v1.y;

    // Edge 1 (v2→v0, opposite v1): weight for v1
    let a1 = v2.y - v0.y;
    let b1 = v0.x - v2.x;
    let c1 = v2.x * v0.y - v0.x * v2.y;

    // Edge 2 (v0→v1, opposite v2): weight for v2
    let a2 = v0.y - v1.y;
    let b2 = v1.x - v0.x;
    let c2 = v0.x * v1.y - v1.x * v0.y;

    // Top-left fill rule bias: non-top-left edges use > 0 instead of >= 0
    let bias0 = if is_top_left(a0, b0) { 0 } else { -1 };
    let bias1 = if is_top_left(a1, b1) { 0 } else { -1 };
    let bias2 = if is_top_left(a2, b2) { 0 } else { -1 };

    // Bounding box, clamped to framebuffer
    let fb_w = fb.width() as i32;
    let fb_h = fb.height() as i32;

    let min_x = v0.x.min(v1.x).min(v2.x).max(0);
    let min_y = v0.y.min(v1.y).min(v2.y).max(0);
    let max_x = v0.x.max(v1.x).max(v2.x).min(fb_w - 1);
    let max_y = v0.y.max(v1.y).max(v2.y).min(fb_h - 1);

    if min_x > max_x || min_y > max_y {
        return;
    }

    // Precompute reciprocal once per triangle — eliminates all per-pixel i64 division.
    let recip = compute_reciprocal(twice_area);

    // Evaluate edge functions at (min_x, min_y).
    // Biased values are used for inside/outside test (top-left fill rule).
    // Unbiased values are used for depth interpolation (correct barycentric weights).
    let base0 = a0 * min_x + b0 * min_y + c0;
    let base1 = a1 * min_x + b1 * min_y + c1;
    let base2 = a2 * min_x + b2 * min_y + c2;

    let mut w0_row = base0 + bias0;
    let mut w1_row = base1 + bias1;
    let mut w2_row = base2 + bias2;

    let mut u0_row = base0;
    let mut u1_row = base1;
    let mut u2_row = base2;

    for y in min_y..=max_y {
        let mut w0 = w0_row;
        let mut w1 = w1_row;
        let mut w2 = w2_row;
        let mut u0 = u0_row;
        let mut u1 = u1_row;
        let mut u2 = u2_row;

        for x in min_x..=max_x {
            if w0 >= 0 && w1 >= 0 && w2 >= 0 {
                // Depth interpolation via reciprocal multiply (no i64 division)
                let z = apply_recip(
                    u0 as i64 * v0.z as i64 + u1 as i64 * v1.z as i64 + u2 as i64 * v2.z as i64,
                    recip,
                ) as i16;

                // Fog interpolation + light color tinting + dithered blend
                let pixel_color = if v0.fog | v1.fog | v2.fog != 0
                    || light_color[0] < 256
                    || light_color[1] < 256
                    || light_color[2] < 256
                {
                    let fog = apply_recip(
                        u0 as i64 * v0.fog as i64
                            + u1 as i64 * v1.fog as i64
                            + u2 as i64 * v2.fog as i64,
                        recip,
                    ) as i16;
                    let dither = BAYER_4X4[((y & 3) * 4 + (x & 3)) as usize];
                    apply_fog(color, fog, dither, light_color)
                } else {
                    color
                };

                fb.set_pixel(x as u32, y as u32, pixel_color, z);
            }
            w0 += a0;
            w1 += a1;
            w2 += a2;
            u0 += a0;
            u1 += a1;
            u2 += a2;
        }

        w0_row += b0;
        w1_row += b1;
        w2_row += b2;
        u0_row += b0;
        u1_row += b1;
        u2_row += b2;
    }
}

/// Rasterize a glyph-textured triangle with 1-bit texel lookup.
///
/// Same edge function rasterization as `rasterize_triangle`, but additionally
/// interpolates UV coordinates (0..255 range mapping to 0..7 in glyph space)
/// and skips transparent texels.
#[allow(clippy::too_many_arguments)]
///
/// `uv0..uv2`: per-vertex UV coordinates as (u, v) in 0..255 fixed-point.
/// `glyph`: 8-byte bitmap — one byte per row, MSB = leftmost pixel.
pub fn rasterize_glyph_triangle(
    fb: &mut Framebuffer,
    v0: ScreenVertex,
    v1: ScreenVertex,
    v2: ScreenVertex,
    color: u16,
    light_color: [u16; 3],
    uv0: (i16, i16),
    uv1: (i16, i16),
    uv2: (i16, i16),
    glyph: &[u8; 8],
) {
    let twice_area =
        (v1.x - v0.x) as i64 * (v2.y - v0.y) as i64 - (v2.x - v0.x) as i64 * (v1.y - v0.y) as i64;
    if twice_area <= 0 {
        return;
    }

    let a0 = v1.y - v2.y;
    let b0 = v2.x - v1.x;
    let c0 = v1.x * v2.y - v2.x * v1.y;
    let a1 = v2.y - v0.y;
    let b1 = v0.x - v2.x;
    let c1 = v2.x * v0.y - v0.x * v2.y;
    let a2 = v0.y - v1.y;
    let b2 = v1.x - v0.x;
    let c2 = v0.x * v1.y - v1.x * v0.y;

    let bias0 = if is_top_left(a0, b0) { 0 } else { -1 };
    let bias1 = if is_top_left(a1, b1) { 0 } else { -1 };
    let bias2 = if is_top_left(a2, b2) { 0 } else { -1 };

    let fb_w = fb.width() as i32;
    let fb_h = fb.height() as i32;
    let min_x = v0.x.min(v1.x).min(v2.x).max(0);
    let min_y = v0.y.min(v1.y).min(v2.y).max(0);
    let max_x = v0.x.max(v1.x).max(v2.x).min(fb_w - 1);
    let max_y = v0.y.max(v1.y).max(v2.y).min(fb_h - 1);

    if min_x > max_x || min_y > max_y {
        return;
    }

    // Precompute reciprocal once per triangle
    let recip = compute_reciprocal(twice_area);

    let base0 = a0 * min_x + b0 * min_y + c0;
    let base1 = a1 * min_x + b1 * min_y + c1;
    let base2 = a2 * min_x + b2 * min_y + c2;

    let mut w0_row = base0 + bias0;
    let mut w1_row = base1 + bias1;
    let mut w2_row = base2 + bias2;
    let mut u0_row = base0;
    let mut u1_row = base1;
    let mut u2_row = base2;

    for y in min_y..=max_y {
        let mut w0 = w0_row;
        let mut w1 = w1_row;
        let mut w2 = w2_row;
        let mut ub0 = u0_row;
        let mut ub1 = u1_row;
        let mut ub2 = u2_row;

        for x in min_x..=max_x {
            if w0 >= 0 && w1 >= 0 && w2 >= 0 {
                // Interpolate UV coordinates via reciprocal multiply
                let tex_u = apply_recip(
                    ub0 as i64 * uv0.0 as i64
                        + ub1 as i64 * uv1.0 as i64
                        + ub2 as i64 * uv2.0 as i64,
                    recip,
                ) as i32;
                let tex_v = apply_recip(
                    ub0 as i64 * uv0.1 as i64
                        + ub1 as i64 * uv1.1 as i64
                        + ub2 as i64 * uv2.1 as i64,
                    recip,
                ) as i32;

                // Map UV (0..255) to glyph pixel (0..7)
                let gx = ((tex_u * 8) >> 8).clamp(0, 7) as usize;
                let gy = ((tex_v * 8) >> 8).clamp(0, 7) as usize;

                // Texel lookup: skip transparent pixels
                if glyph[gy] & (0x80 >> gx) != 0 {
                    let z = apply_recip(
                        ub0 as i64 * v0.z as i64
                            + ub1 as i64 * v1.z as i64
                            + ub2 as i64 * v2.z as i64,
                        recip,
                    ) as i16;

                    let pixel_color = if v0.fog | v1.fog | v2.fog != 0
                        || light_color[0] < 256
                        || light_color[1] < 256
                        || light_color[2] < 256
                    {
                        let fog = apply_recip(
                            ub0 as i64 * v0.fog as i64
                                + ub1 as i64 * v1.fog as i64
                                + ub2 as i64 * v2.fog as i64,
                            recip,
                        ) as i16;
                        let dither = BAYER_4X4[((y & 3) * 4 + (x & 3)) as usize];
                        apply_fog(color, fog, dither, light_color)
                    } else {
                        color
                    };

                    fb.set_pixel(x as u32, y as u32, pixel_color, z);
                }
            }
            w0 += a0;
            w1 += a1;
            w2 += a2;
            ub0 += a0;
            ub1 += a1;
            ub2 += a2;
        }

        w0_row += b0;
        w1_row += b1;
        w2_row += b2;
        u0_row += b0;
        u1_row += b1;
        u2_row += b2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::rgb555;

    /// White (neutral) light for tests that don't care about color tinting.
    const WHITE: [u16; 3] = [256, 256, 256];

    fn count_colored_pixels(fb: &Framebuffer, color: u16) -> u32 {
        let mut count = 0;
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get_pixel(x, y) == color {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn small_triangle_renders_pixels() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);

        // CCW triangle covering a few pixels
        let v0 = ScreenVertex::new(2, 1, 0);
        let v1 = ScreenVertex::new(6, 1, 0);
        let v2 = ScreenVertex::new(4, 5, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, red, WHITE);

        // Center pixel should be filled
        assert_eq!(fb.get_pixel(4, 3), red, "center pixel should be red");
        // Corner outside triangle should not
        assert_eq!(fb.get_pixel(0, 0), 0, "outside pixel should be black");
        // Should have rendered some pixels
        let count = count_colored_pixels(&fb, red);
        assert!(count > 0, "triangle should render at least some pixels");
        assert!(count < 100, "triangle shouldn't fill the whole buffer");
    }

    #[test]
    fn backface_cull_cw() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);

        // CW winding — should be culled
        let v0 = ScreenVertex::new(0, 0, 0);
        let v1 = ScreenVertex::new(0, 5, 0);
        let v2 = ScreenVertex::new(5, 0, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, red, WHITE);
        assert_eq!(
            count_colored_pixels(&fb, red),
            0,
            "CW triangle should be culled"
        );
    }

    #[test]
    fn backface_cull_ccw_renders() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);

        // Swap v1/v2 to get CCW — should render
        let v0 = ScreenVertex::new(0, 0, 0);
        let v1 = ScreenVertex::new(5, 0, 0);
        let v2 = ScreenVertex::new(0, 5, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, red, WHITE);
        assert!(
            count_colored_pixels(&fb, red) > 0,
            "CCW triangle should render"
        );
    }

    #[test]
    fn degenerate_collinear() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);

        // Collinear points — zero area
        let v0 = ScreenVertex::new(0, 0, 0);
        let v1 = ScreenVertex::new(5, 5, 0);
        let v2 = ScreenVertex::new(9, 9, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, red, WHITE);
        assert_eq!(
            count_colored_pixels(&fb, red),
            0,
            "degenerate triangle should render nothing"
        );
    }

    #[test]
    fn clipping_to_bounds() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);

        // Triangle extends well beyond the framebuffer
        let v0 = ScreenVertex::new(-10, 5, 0);
        let v1 = ScreenVertex::new(5, -10, 0);
        let v2 = ScreenVertex::new(20, 20, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, red, WHITE);

        // Should have rendered some pixels inside the buffer
        let count = count_colored_pixels(&fb, red);
        assert!(
            count > 0,
            "partially visible triangle should render some pixels"
        );
        // No crash = no out-of-bounds writes
    }

    #[test]
    fn zbuffer_near_occludes_far() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);
        let green = rgb555(0, 31, 0);

        // Far triangle (depth 1000)
        let far = 1000i16;
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, far),
            ScreenVertex::new(8, 1, far),
            ScreenVertex::new(4, 8, far),
            red,
            WHITE,
        );

        // Near triangle overlapping (depth -1000)
        let near = -1000i16;
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, near),
            ScreenVertex::new(8, 1, near),
            ScreenVertex::new(4, 8, near),
            green,
            WHITE,
        );

        // Overlapping pixels should be green (near wins)
        assert_eq!(
            fb.get_pixel(4, 4),
            green,
            "near triangle should occlude far"
        );
    }

    #[test]
    fn zbuffer_reverse_draw_order() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);
        let green = rgb555(0, 31, 0);

        // Near triangle drawn first (depth -1000)
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, -1000),
            ScreenVertex::new(8, 1, -1000),
            ScreenVertex::new(4, 8, -1000),
            red,
            WHITE,
        );

        // Far triangle drawn second (depth 1000) — should NOT overwrite
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, 1000),
            ScreenVertex::new(8, 1, 1000),
            ScreenVertex::new(4, 8, 1000),
            green,
            WHITE,
        );

        // Should still show red (near triangle preserved by z-buffer)
        assert_eq!(
            fb.get_pixel(4, 4),
            red,
            "z-buffer should preserve near triangle"
        );
    }

    #[test]
    fn flat_depth_interpolation() {
        let mut fb = Framebuffer::new(10, 10);
        let red = rgb555(31, 0, 0);
        let depth = 500i16;

        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, depth),
            ScreenVertex::new(8, 1, depth),
            ScreenVertex::new(4, 8, depth),
            red,
            WHITE,
        );

        // All rasterized pixels should have approximately the same depth.
        // The reciprocal multiply optimization introduces ±1 LSB rounding vs
        // exact division — invisible to the z-buffer (relative order preserved).
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get_pixel(x, y) == red {
                    let d = fb.get_depth(x, y);
                    assert!(
                        (d - depth).abs() <= 1,
                        "pixel ({x}, {y}) has depth {d} but expected {depth} ± 1",
                    );
                }
            }
        }
    }

    #[test]
    fn full_screen_triangle() {
        let mut fb = Framebuffer::new(4, 4);
        let red = rgb555(31, 0, 0);

        // Triangle that covers entire 4x4 buffer
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(-10, -10, 0),
            ScreenVertex::new(20, -10, 0),
            ScreenVertex::new(5, 20, 0),
            red,
            WHITE,
        );

        assert_eq!(
            count_colored_pixels(&fb, red),
            16,
            "should fill all 16 pixels"
        );
    }

    #[test]
    fn warm_light_tints_output() {
        let mut fb = Framebuffer::new(10, 10);
        let white = rgb555(31, 31, 31);
        // Warm light: full red, half green, no blue
        let warm = [256, 128, 0];

        let v0 = ScreenVertex::new(0, 0, 0);
        let v1 = ScreenVertex::new(9, 0, 0);
        let v2 = ScreenVertex::new(5, 9, 0);

        rasterize_triangle(&mut fb, v0, v1, v2, white, warm);

        let pixel = fb.get_pixel(5, 3);
        let r = (pixel >> 10) & 0x1F;
        let g = (pixel >> 5) & 0x1F;
        let b = pixel & 0x1F;
        assert!(r > g, "warm light: red ({r}) should exceed green ({g})");
        assert_eq!(b, 0, "warm light: blue should be zero");
    }
}
