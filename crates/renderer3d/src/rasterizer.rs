use crate::framebuffer::Framebuffer;
use crate::pipeline::ScreenVertex;

/// Blend an RGB555 color toward black by a fog factor (0 = clear, 256 = full black).
/// Each 5-bit channel is multiplied by (256 - fog) and shifted right by 8.
#[inline]
fn apply_fog(color: u16, fog: i16) -> u16 {
    if fog <= 0 {
        return color;
    }
    let inv = (256 - fog.min(256)) as u16;
    let r = (((color >> 10) & 0x1F) * inv) >> 8;
    let g = (((color >> 5) & 0x1F) * inv) >> 8;
    let b = ((color & 0x1F) * inv) >> 8;
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
                // Depth interpolation via unbiased barycentric weights (i64 for overflow)
                let z =
                    ((u0 as i64 * v0.z as i64 + u1 as i64 * v1.z as i64 + u2 as i64 * v2.z as i64)
                        / twice_area) as i16;

                // Fog interpolation + color blend
                let pixel_color = if v0.fog | v1.fog | v2.fog != 0 {
                    let fog = ((u0 as i64 * v0.fog as i64
                        + u1 as i64 * v1.fog as i64
                        + u2 as i64 * v2.fog as i64)
                        / twice_area) as i16;
                    apply_fog(color, fog)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::rgb555;

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

        rasterize_triangle(&mut fb, v0, v1, v2, red);

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

        rasterize_triangle(&mut fb, v0, v1, v2, red);
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

        rasterize_triangle(&mut fb, v0, v1, v2, red);
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

        rasterize_triangle(&mut fb, v0, v1, v2, red);
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

        rasterize_triangle(&mut fb, v0, v1, v2, red);

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
        );

        // Near triangle overlapping (depth -1000)
        let near = -1000i16;
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, near),
            ScreenVertex::new(8, 1, near),
            ScreenVertex::new(4, 8, near),
            green,
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
        );

        // Far triangle drawn second (depth 1000) — should NOT overwrite
        rasterize_triangle(
            &mut fb,
            ScreenVertex::new(1, 1, 1000),
            ScreenVertex::new(8, 1, 1000),
            ScreenVertex::new(4, 8, 1000),
            green,
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
        );

        // All rasterized pixels should have the same depth
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get_pixel(x, y) == red {
                    assert_eq!(
                        fb.get_depth(x, y),
                        depth,
                        "pixel ({x}, {y}) has depth {} but expected {depth}",
                        fb.get_depth(x, y)
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
        );

        assert_eq!(
            count_colored_pixels(&fb, red),
            16,
            "should fill all 16 pixels"
        );
    }
}
