use roguelike_core::rules::game_view::GameView;

use crate::framebuffer::Framebuffer;
use crate::geometry::{self, TriangleSink};
use crate::math::{Fixed16, Mat4, Vec3, Vec4};
use crate::pipeline::{ScreenVertex, project_vertex};
use crate::rasterizer::rasterize_triangle;

/// Camera height above the floor plane (in world units).
const CAMERA_HEIGHT: Fixed16 = Fixed16::from_int(12);

/// Forward offset from the eye toward the target — controls tilt angle.
/// At height=12, offset=3 gives ~75° from horizontal (arctan(12/3) ≈ 76°).
const CAMERA_TILT_OFFSET: Fixed16 = Fixed16::from_int(3);

/// Vertical FOV as a fraction of full circle. ~60° ≈ 1/6 turn.
/// Fixed16::ONE = 360°, so 60° = ONE/6 ≈ 0x2AAA.
const FOV: Fixed16 = Fixed16::from_raw(0x2AAA);

/// Near clipping plane distance.
const NEAR: Fixed16 = Fixed16::ONE;

/// Far clipping plane distance — enough to see ~40 tiles.
const FAR: Fixed16 = Fixed16::from_int(60);

// --- PS1 aesthetic effects ---

/// Vertex snap grid size in pixels. Vertices are snapped to multiples of this
/// value after projection, causing the signature PS1 polygon wobble/jitter.
/// Set to 1 to disable. 2 gives a subtle shimmer, 4+ is very noticeable.
const SNAP_GRID: i32 = 2;

/// Rasterizing triangle sink: transforms world-space triangles through
/// the MVP matrix and rasterizes them into the framebuffer.
struct RasterSink<'a> {
    fb: &'a mut Framebuffer,
    mvp: Mat4,
    width: i32,
    height: i32,
}

impl<'a> RasterSink<'a> {
    fn new(fb: &'a mut Framebuffer, mvp: Mat4) -> Self {
        let width = fb.width() as i32;
        let height = fb.height() as i32;
        Self {
            fb,
            mvp,
            width,
            height,
        }
    }
}

/// Lerp between two clip-space vertices at parameter t.
#[inline]
fn clip_lerp(a: Vec4, b: Vec4, t: Fixed16) -> Vec4 {
    Vec4::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
        a.w + (b.w - a.w) * t,
    )
}

/// Snap a screen-space vertex to the PS1-style grid.
/// Uses Euclidean division for consistent behavior with negative coordinates.
#[inline]
fn snap_vertex(sv: ScreenVertex) -> ScreenVertex {
    if SNAP_GRID <= 1 {
        return sv;
    }
    ScreenVertex::new(
        sv.x.div_euclid(SNAP_GRID) * SNAP_GRID,
        sv.y.div_euclid(SNAP_GRID) * SNAP_GRID,
        sv.z,
    )
}

impl RasterSink<'_> {
    /// Project, snap, and rasterize a clip-space triangle.
    #[inline]
    fn project_and_rasterize(&mut self, c0: Vec4, c1: Vec4, c2: Vec4, color: u16) {
        let s0 = snap_vertex(project_vertex(c0, self.width, self.height));
        let s1 = snap_vertex(project_vertex(c1, self.width, self.height));
        let s2 = snap_vertex(project_vertex(c2, self.width, self.height));
        rasterize_triangle(self.fb, s0, s1, s2, color);
    }

    /// Clip a triangle against the near plane (w + z = 0) and rasterize the result.
    ///
    /// `v`: clip-space vertices, `d`: signed distance to near plane (w + z),
    /// `inside`: which vertices are in front, `count`: number inside (1 or 2).
    fn clip_and_rasterize(
        &mut self,
        v: [Vec4; 3],
        d: [Fixed16; 3],
        inside: [bool; 3],
        count: u8,
        color: u16,
    ) {
        if count == 2 {
            // 1 vertex outside — clip produces a quad (2 triangles)
            let out = if !inside[0] {
                0
            } else if !inside[1] {
                1
            } else {
                2
            };
            let next = (out + 1) % 3;
            let prev = (out + 2) % 3;

            // Interpolation parameter along each edge from inside→outside
            let t_next = d[next] / (d[next] - d[out]);
            let t_prev = d[prev] / (d[prev] - d[out]);

            let p_next = clip_lerp(v[next], v[out], t_next);
            let p_prev = clip_lerp(v[prev], v[out], t_prev);

            // Quad (v[next], v[prev], p_prev, p_next) preserves winding
            self.project_and_rasterize(v[next], v[prev], p_prev, color);
            self.project_and_rasterize(v[next], p_prev, p_next, color);
        } else {
            // 2 vertices outside — clip produces 1 smaller triangle
            let in_idx = if inside[0] {
                0
            } else if inside[1] {
                1
            } else {
                2
            };
            let next = (in_idx + 1) % 3;
            let prev = (in_idx + 2) % 3;

            let t_next = d[in_idx] / (d[in_idx] - d[next]);
            let t_prev = d[in_idx] / (d[in_idx] - d[prev]);

            let p_next = clip_lerp(v[in_idx], v[next], t_next);
            let p_prev = clip_lerp(v[in_idx], v[prev], t_prev);

            self.project_and_rasterize(v[in_idx], p_next, p_prev, color);
        }
    }
}

impl TriangleSink for RasterSink<'_> {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, color: u16) {
        // Transform world-space → clip-space via MVP
        let c0 = self.mvp * v0.to_point();
        let c1 = self.mvp * v1.to_point();
        let c2 = self.mvp * v2.to_point();

        // Classify vertices against near plane (w + z = 0).
        // Vertices with d >= 0 are in front of (or on) the near plane.
        let d0 = c0.w + c0.z;
        let d1 = c1.w + c1.z;
        let d2 = c2.w + c2.z;

        let in0 = d0.to_raw() >= 0;
        let in1 = d1.to_raw() >= 0;
        let in2 = d2.to_raw() >= 0;

        let count = in0 as u8 + in1 as u8 + in2 as u8;

        match count {
            3 => {
                // Fast path: all inside, no clipping needed
                self.project_and_rasterize(c0, c1, c2, color);
            }
            0 => {
                // All behind near plane — cull
            }
            _ => {
                // 1 or 2 vertices inside — clip against near plane
                self.clip_and_rasterize([c0, c1, c2], [d0, d1, d2], [in0, in1, in2], count, color);
            }
        }
    }
}

/// Render the game world into a framebuffer.
///
/// Sets up a nearly top-down camera centered on the player, builds the
/// MVP matrix, and streams all visible geometry through the rasterizer.
pub fn render_scene(view: &dyn GameView, fb: &mut Framebuffer) {
    fb.clear(0, i16::MAX);

    let (px, py) = view.player_xy();

    // Camera looks at the player's floor position.
    // Eye is above and slightly behind (negative z = north in grid space).
    let target = Vec3::new(
        Fixed16::from_int(px) + Fixed16::HALF,
        Fixed16::ZERO,
        Fixed16::from_int(py) + Fixed16::HALF,
    );
    let eye = Vec3::new(target.x, CAMERA_HEIGHT, target.z - CAMERA_TILT_OFFSET);
    let up = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);

    let view_mat = Mat4::look_at(eye, target, up);

    let aspect = Fixed16::from_raw((((fb.width() as i64) << 16) / fb.height() as i64) as i32);
    let proj_mat = Mat4::perspective(FOV, aspect, NEAR, FAR);

    let mvp = proj_mat.mul_mat(&view_mat);

    let mut sink = RasterSink::new(fb, mvp);
    geometry::generate_map_geometry(view, &mut sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::rgb555;
    use crate::math::Vec4;
    use roguelike_core::tier_micro::game::MicroGameState;

    /// Helper: count non-black pixels in a framebuffer.
    fn count_colored(fb: &Framebuffer) -> u32 {
        let mut n = 0u32;
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get_pixel(x, y) != 0 {
                    n += 1;
                }
            }
        }
        n
    }

    /// Create a RasterSink with identity MVP (clip space = world space).
    /// Useful for testing clipping in isolation.
    fn make_test_sink(fb: &mut Framebuffer) -> RasterSink<'_> {
        RasterSink::new(fb, Mat4::identity())
    }

    // --- Clipping unit tests ---

    #[test]
    fn clip_lerp_midpoint() {
        let a = Vec4::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE);
        let b = Vec4::new(Fixed16::ONE, Fixed16::ONE, Fixed16::ONE, Fixed16::ONE);
        let mid = clip_lerp(a, b, Fixed16::HALF);
        assert_eq!(mid.x, Fixed16::HALF);
        assert_eq!(mid.y, Fixed16::HALF);
        assert_eq!(mid.z, Fixed16::HALF);
        assert_eq!(mid.w, Fixed16::ONE);
    }

    #[test]
    fn clip_lerp_endpoints() {
        let a = Vec4::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE);
        let b = Vec4::new(
            Fixed16::ONE,
            Fixed16::ONE,
            Fixed16::ONE,
            Fixed16::from_int(2),
        );
        assert_eq!(clip_lerp(a, b, Fixed16::ZERO), a);
        assert_eq!(clip_lerp(a, b, Fixed16::ONE), b);
    }

    #[test]
    fn all_inside_renders() {
        let mut fb = Framebuffer::new(100, 100);
        let mut sink = make_test_sink(&mut fb);
        let color = rgb555(31, 0, 0);

        // CW winding in world xy-plane → CCW in screen after viewport y-flip.
        // With identity MVP, w=1, z=0, so w+z=1 > 0 (all inside).
        sink.emit(
            Vec3::from_ints(0, 0, 0),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::ZERO),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::ZERO),
            color,
        );

        assert!(count_colored(&fb) > 0, "all-inside triangle should render");
    }

    #[test]
    fn all_outside_culled() {
        let mut fb = Framebuffer::new(100, 100);
        let mut sink = make_test_sink(&mut fb);
        let color = rgb555(31, 0, 0);

        // All at z=-2: w+z = 1+(-2) = -1 < 0 → all outside.
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            color,
        );

        assert_eq!(
            count_colored(&fb),
            0,
            "all-outside triangle should be culled"
        );
    }

    #[test]
    fn one_vertex_behind_clips() {
        let mut fb = Framebuffer::new(100, 100);
        let mut sink = make_test_sink(&mut fb);
        let color = rgb555(31, 0, 0);

        // CW winding: v0 and v1 in front (z=0, d=1), v2 behind (z=-2, d=-1).
        // Without clipping this would be culled entirely.
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::ZERO),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            color,
        );

        assert!(
            count_colored(&fb) > 0,
            "triangle with 1 vertex behind should be clipped, not culled"
        );
    }

    #[test]
    fn two_vertices_behind_clips() {
        let mut fb = Framebuffer::new(100, 100);
        let mut sink = make_test_sink(&mut fb);
        let color = rgb555(31, 0, 0);

        // CW winding: v0 in front (z=0, d=1), v1 and v2 behind (z=-2, d=-1).
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            color,
        );

        assert!(
            count_colored(&fb) > 0,
            "triangle with 2 vertices behind should be clipped, not culled"
        );
    }

    // --- Integration tests ---

    #[test]
    fn render_produces_pixels() {
        let game = MicroGameState::new_default(42);
        let mut fb = Framebuffer::new(160, 120);

        render_scene(&game, &mut fb);

        // Should have rendered at least some non-black pixels
        let mut colored = 0u32;
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                if fb.get_pixel(x, y) != 0 {
                    colored += 1;
                }
            }
        }
        assert!(
            colored > 0,
            "render_scene should produce at least some colored pixels"
        );
    }

    #[test]
    fn different_seeds_differ() {
        let game_a = MicroGameState::new_default(100);
        let game_b = MicroGameState::new_default(200);

        let mut fb_a = Framebuffer::new(80, 60);
        let mut fb_b = Framebuffer::new(80, 60);

        render_scene(&game_a, &mut fb_a);
        render_scene(&game_b, &mut fb_b);

        // At least one pixel should differ between different seeds
        let mut differs = false;
        'outer: for y in 0..fb_a.height() {
            for x in 0..fb_a.width() {
                if fb_a.get_pixel(x, y) != fb_b.get_pixel(x, y) {
                    differs = true;
                    break 'outer;
                }
            }
        }
        assert!(differs, "different seeds should produce different images");
    }

    #[test]
    fn center_half_has_content() {
        let game = MicroGameState::new_default(42);
        let mut fb = Framebuffer::new(160, 120);

        render_scene(&game, &mut fb);

        // The central 50% of the image should have some content.
        // With a top-down camera centered on the player, the player's
        // surroundings should project somewhere in this region.
        let x0 = fb.width() / 4;
        let x1 = 3 * fb.width() / 4;
        let y0 = fb.height() / 4;
        let y1 = 3 * fb.height() / 4;
        let mut center_colored = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                if fb.get_pixel(x, y) != 0 {
                    center_colored += 1;
                }
            }
        }
        assert!(
            center_colored > 0,
            "central region should have rendered pixels"
        );
    }
}
