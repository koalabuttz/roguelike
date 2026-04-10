use roguelike_core::rules::game_view::{GameView, TileVisibility};

use crate::color_map::game_color_to_rgb555;
use crate::font;
use crate::framebuffer::Framebuffer;
use crate::geometry::{self, TriangleSink};
use crate::math::{Fixed16, Mat4, Vec3, Vec4};
use crate::pipeline::{ScreenVertex, project_vertex};
use crate::rasterizer::{rasterize_glyph_triangle, rasterize_triangle};

/// Camera height above the floor plane (in world units).
const CAMERA_HEIGHT: Fixed16 = Fixed16::from_int(10);

/// Forward offset from the eye toward the target — controls tilt angle.
/// At height=10, offset=5 gives ~63° from horizontal (arctan(10/5) ≈ 63°).
/// Lower than the original 76° to make entity billboards more readable.
const CAMERA_TILT_OFFSET: Fixed16 = Fixed16::from_int(5);

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

// --- Distance fog ---

/// Distance² (in tiles) where fog begins.
const FOG_START_SQ: i32 = 36; // 6 tiles
/// Distance² (in tiles) where fog reaches full black.
const FOG_END_SQ: i32 = 196; // 14 tiles
/// Range for the linear ramp (precomputed to avoid division in the hot path).
const FOG_RANGE_SQ: i32 = FOG_END_SQ - FOG_START_SQ;

// --- Billboard constants ---

/// Billboard width in world units (fraction of a tile).
const BILLBOARD_WIDTH: Fixed16 = Fixed16::from_raw(0xB333); // ~0.7
/// Billboard height in world units.
const BILLBOARD_HEIGHT: Fixed16 = Fixed16::from_raw(0xE666); // ~0.9

/// Rasterizing triangle sink: transforms world-space triangles through
/// the MVP matrix and rasterizes them into the framebuffer.
struct RasterSink<'a> {
    fb: &'a mut Framebuffer,
    mvp: Mat4,
    width: i32,
    height: i32,
    /// Player position in world-space tile coordinates for distance fog.
    player_x: i32,
    player_z: i32,
    /// Camera right vector for billboard orientation.
    cam_right: Vec3,
}

impl<'a> RasterSink<'a> {
    fn new(
        fb: &'a mut Framebuffer,
        mvp: Mat4,
        player_x: i32,
        player_z: i32,
        cam_right: Vec3,
    ) -> Self {
        let width = fb.width() as i32;
        let height = fb.height() as i32;
        Self {
            fb,
            mvp,
            width,
            height,
            player_x,
            player_z,
            cam_right,
        }
    }

    /// Compute fog factor (0-256) for a world-space vertex based on XZ distance to player.
    #[inline]
    fn vertex_fog(&self, world: Vec3) -> i16 {
        let dx = world.x.to_int() - self.player_x;
        let dz = world.z.to_int() - self.player_z;
        let dist_sq = dx * dx + dz * dz;
        if dist_sq <= FOG_START_SQ {
            0
        } else if dist_sq >= FOG_END_SQ {
            256
        } else {
            ((dist_sq - FOG_START_SQ) * 256 / FOG_RANGE_SQ) as i16
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
fn snap_vertex(mut sv: ScreenVertex) -> ScreenVertex {
    if SNAP_GRID > 1 {
        sv.x = sv.x.div_euclid(SNAP_GRID) * SNAP_GRID;
        sv.y = sv.y.div_euclid(SNAP_GRID) * SNAP_GRID;
    }
    sv
}

/// Lerp a fog value at parameter t (matching clip_lerp for positions).
#[inline]
fn fog_lerp(a: i16, b: i16, t: Fixed16) -> i16 {
    (a as i32 + ((((b as i32 - a as i32) as i64) * t.to_raw() as i64) >> 16) as i32) as i16
}

impl RasterSink<'_> {
    /// Project a clip-space vertex to screen space with fog, then snap.
    #[inline]
    fn project_with_fog(&self, clip: Vec4, fog: i16) -> ScreenVertex {
        let mut sv = project_vertex(clip, self.width, self.height);
        sv.fog = fog;
        snap_vertex(sv)
    }

    /// Project, snap, and rasterize a clip-space triangle with per-vertex fog.
    #[inline]
    fn project_and_rasterize(&mut self, c0: Vec4, c1: Vec4, c2: Vec4, f: [i16; 3], color: u16) {
        let s0 = self.project_with_fog(c0, f[0]);
        let s1 = self.project_with_fog(c1, f[1]);
        let s2 = self.project_with_fog(c2, f[2]);
        rasterize_triangle(self.fb, s0, s1, s2, color);
    }

    /// Clip a triangle against the near plane (w + z = 0) and rasterize the result.
    /// Fog factors are interpolated at clip points alongside positions.
    fn clip_and_rasterize(
        &mut self,
        v: [Vec4; 3],
        d: [Fixed16; 3],
        inside: [bool; 3],
        count: u8,
        f: [i16; 3],
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

            let t_next = d[next] / (d[next] - d[out]);
            let t_prev = d[prev] / (d[prev] - d[out]);

            let p_next = clip_lerp(v[next], v[out], t_next);
            let p_prev = clip_lerp(v[prev], v[out], t_prev);
            let f_next = fog_lerp(f[next], f[out], t_next);
            let f_prev = fog_lerp(f[prev], f[out], t_prev);

            self.project_and_rasterize(v[next], v[prev], p_prev, [f[next], f[prev], f_prev], color);
            self.project_and_rasterize(v[next], p_prev, p_next, [f[next], f_prev, f_next], color);
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
            let f_next = fog_lerp(f[in_idx], f[next], t_next);
            let f_prev = fog_lerp(f[in_idx], f[prev], t_prev);

            self.project_and_rasterize(
                v[in_idx],
                p_next,
                p_prev,
                [f[in_idx], f_next, f_prev],
                color,
            );
        }
    }

    /// Render a glyph billboard at a world-space tile position.
    ///
    /// Constructs a camera-facing vertical quad, transforms it through
    /// the MVP pipeline, and rasterizes with 1-bit glyph texel lookup.
    fn render_billboard(&mut self, tile_x: i32, tile_z: i32, glyph: &font::Glyph, color: u16) {
        let center_x = Fixed16::from_int(tile_x) + Fixed16::HALF;
        let center_z = Fixed16::from_int(tile_z) + Fixed16::HALF;

        let half_w = Fixed16::from_raw(BILLBOARD_WIDTH.to_raw() >> 1);

        let right = self.cam_right;

        // Billboard quad corners (CW from front for y-flip compensation)
        // Bottom-left, top-left, top-right, bottom-right
        let bl = Vec3::new(
            center_x - right.x * half_w,
            Fixed16::ZERO,
            center_z - right.z * half_w,
        );
        let tl = Vec3::new(
            center_x - right.x * half_w,
            BILLBOARD_HEIGHT,
            center_z - right.z * half_w,
        );
        let tr = Vec3::new(
            center_x + right.x * half_w,
            BILLBOARD_HEIGHT,
            center_z + right.z * half_w,
        );
        let br = Vec3::new(
            center_x + right.x * half_w,
            Fixed16::ZERO,
            center_z + right.z * half_w,
        );

        // UV coords: 0..255 maps to glyph 0..7
        // CW winding (y-flip): bl, tl, tr and bl, tr, br
        let uv_bl = (0i16, 255); // bottom-left of glyph
        let uv_tl = (0, 0); // top-left
        let uv_tr = (255, 0); // top-right
        let uv_br = (255, 255); // bottom-right

        // Compute fog for the center position
        let fog = self.vertex_fog(Vec3::new(center_x, Fixed16::ZERO, center_z));

        // Transform and rasterize each triangle of the quad
        // Using CW winding to match emit_quad convention (y-flip compensation)
        self.rasterize_billboard_tri(bl, tl, tr, color, fog, uv_bl, uv_tl, uv_tr, glyph);
        self.rasterize_billboard_tri(bl, tr, br, color, fog, uv_bl, uv_tr, uv_br, glyph);
    }

    /// Transform, clip, project, and rasterize a single billboard triangle.
    #[allow(clippy::too_many_arguments)]
    fn rasterize_billboard_tri(
        &mut self,
        v0: Vec3,
        v1: Vec3,
        v2: Vec3,
        color: u16,
        fog: i16,
        uv0: (i16, i16),
        uv1: (i16, i16),
        uv2: (i16, i16),
        glyph: &font::Glyph,
    ) {
        let c0 = self.mvp * v0.to_point();
        let c1 = self.mvp * v1.to_point();
        let c2 = self.mvp * v2.to_point();

        // Near-plane classification
        let d0 = c0.w + c0.z;
        let d1 = c1.w + c1.z;
        let d2 = c2.w + c2.z;

        let in0 = d0.to_raw() >= 0;
        let in1 = d1.to_raw() >= 0;
        let in2 = d2.to_raw() >= 0;

        let count = in0 as u8 + in1 as u8 + in2 as u8;

        if count == 0 {
            return;
        }

        // For simplicity, only render billboard triangles that are fully inside.
        // Billboards are small — clipping is rarely needed.
        if count < 3 {
            return;
        }

        let mut s0 = snap_vertex(project_vertex(c0, self.width, self.height));
        let mut s1 = snap_vertex(project_vertex(c1, self.width, self.height));
        let mut s2 = snap_vertex(project_vertex(c2, self.width, self.height));

        s0.fog = fog;
        s1.fog = fog;
        s2.fog = fog;

        rasterize_glyph_triangle(self.fb, s0, s1, s2, color, uv0, uv1, uv2, glyph);
    }
}

impl TriangleSink for RasterSink<'_> {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, color: u16) {
        // Compute per-vertex fog from world-space XZ distance to player
        let f0 = self.vertex_fog(v0);
        let f1 = self.vertex_fog(v1);
        let f2 = self.vertex_fog(v2);

        // Transform world-space → clip-space via MVP
        let c0 = self.mvp * v0.to_point();
        let c1 = self.mvp * v1.to_point();
        let c2 = self.mvp * v2.to_point();

        // Classify vertices against near plane (w + z = 0).
        let d0 = c0.w + c0.z;
        let d1 = c1.w + c1.z;
        let d2 = c2.w + c2.z;

        let in0 = d0.to_raw() >= 0;
        let in1 = d1.to_raw() >= 0;
        let in2 = d2.to_raw() >= 0;

        let count = in0 as u8 + in1 as u8 + in2 as u8;
        let f = [f0, f1, f2];

        match count {
            3 => {
                // Fast path: all inside, no clipping needed
                self.project_and_rasterize(c0, c1, c2, f, color);
            }
            0 => {
                // All behind near plane — cull
            }
            _ => {
                // 1 or 2 vertices inside — clip against near plane
                self.clip_and_rasterize(
                    [c0, c1, c2],
                    [d0, d1, d2],
                    [in0, in1, in2],
                    count,
                    f,
                    color,
                );
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

    // Camera basis vectors for billboard orientation
    let forward = (target - eye).normalize();
    let cam_right = forward.cross(up).normalize();

    let view_mat = Mat4::look_at(eye, target, up);

    let aspect = Fixed16::from_raw((((fb.width() as i64) << 16) / fb.height() as i64) as i32);
    let proj_mat = Mat4::perspective(FOV, aspect, NEAR, FAR);

    let mvp = proj_mat.mul_mat(&view_mat);

    // Render map geometry (floors, walls)
    let mut sink = RasterSink::new(fb, mvp, px, py, cam_right);
    geometry::generate_map_geometry(view, &mut sink);

    // Render entity billboards
    for i in 0..view.entity_count() {
        if !view.entity_alive(i) {
            continue;
        }
        let (ex, ey) = view.entity_xy(i);
        if view.tile_visibility(ex, ey) != TileVisibility::Visible {
            continue;
        }
        let (ch, gc) = view.render_entity(i);
        let color = game_color_to_rgb555(gc);
        let glyph = font::glyph(ch);
        sink.render_billboard(ex, ey, &glyph, color);
    }

    // Render item billboards
    for i in 0..view.item_count() {
        if !view.item_alive(i) {
            continue;
        }
        let (ix, iy) = view.item_xy(i);
        if view.tile_visibility(ix, iy) != TileVisibility::Visible {
            continue;
        }
        let (ch, gc) = view.render_item(i);
        let color = game_color_to_rgb555(gc);
        let glyph = font::glyph(ch);
        sink.render_billboard(ix, iy, &glyph, color);
    }
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
    /// Player at origin, no fog for nearby test vertices.
    fn make_test_sink(fb: &mut Framebuffer) -> RasterSink<'_> {
        RasterSink::new(
            fb,
            Mat4::identity(),
            0,
            0,
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
        )
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
