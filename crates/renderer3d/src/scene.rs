use roguelike_core::rules::game_view::GameView;

use crate::framebuffer::Framebuffer;
use crate::geometry::{self, TriangleSink};
use crate::math::{Fixed16, Mat4, Vec3};
use crate::pipeline::project_vertex;
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

impl TriangleSink for RasterSink<'_> {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, color: u16) {
        // Transform world-space → clip-space via MVP
        let c0 = self.mvp * v0.to_point();
        let c1 = self.mvp * v1.to_point();
        let c2 = self.mvp * v2.to_point();

        // Cull triangles with any vertex behind the near plane (w <= 0).
        // Without proper clipping, perspective divide on negative w produces garbage.
        if c0.w.to_raw() <= 0 || c1.w.to_raw() <= 0 || c2.w.to_raw() <= 0 {
            return;
        }

        // Project to screen space
        let s0 = project_vertex(c0, self.width, self.height);
        let s1 = project_vertex(c1, self.width, self.height);
        let s2 = project_vertex(c2, self.width, self.height);

        rasterize_triangle(self.fb, s0, s1, s2, color);
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
    use roguelike_core::tier_micro::game::MicroGameState;

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
