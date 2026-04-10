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

// --- Light map system ---
// A per-tile fog map combines FOV visibility with distance-based falloff.
// Visible tiles: fog = distance_ramp (0 near player, 256 at FOV_RADIUS).
// Non-visible tiles: fog = 256 (full dark, rendered as black geometry).
// This eliminates hard edges: non-visible tiles ARE rendered but fully dark.

/// Distance² (in tiles) where fog begins.
const FOG_START_SQ: i32 = 4; // 2 tiles
/// Distance² (in tiles) where fog reaches full black (= FOV_RADIUS²).
const FOG_END_SQ: i32 = 64; // 8 tiles
/// Range for the linear ramp.
const FOG_RANGE_SQ: i32 = FOG_END_SQ - FOG_START_SQ;

// --- Billboard constants ---

/// Billboard width in world units (fraction of a tile).
const BILLBOARD_WIDTH: Fixed16 = Fixed16::from_raw(0xB333); // ~0.7
/// Billboard height in world units.
const BILLBOARD_HEIGHT: Fixed16 = Fixed16::from_raw(0xE666); // ~0.9

/// Minimum ambient light factor (0..256). Prevents surfaces from going
/// completely black just from the Lambert term — simulates indirect light.
const AMBIENT: i32 = 50;

/// Height of the player's light source above the floor.
/// Higher = wider floor light pool (light hits floor at less oblique angle).
/// At h=2.5: the floor at 2 tiles away still gets ~80% of directly-below brightness.
const LIGHT_HEIGHT: Fixed16 = Fixed16::from_raw(0x28000); // 2.5

/// Attenuation scale: controls light reach. Formula: `256 / (1 + d²/ATTEN_SCALE)`.
/// Higher = light reaches further. At 12: brightness halves at ~3.5 tiles.
const ATTEN_SCALE: i64 = 16;

/// Rasterizing triangle sink: transforms world-space triangles through
/// the MVP matrix and rasterizes them into the framebuffer.
struct RasterSink<'a> {
    fb: &'a mut Framebuffer,
    mvp: Mat4,
    width: i32,
    height: i32,
    cam_right: Vec3,
    /// Per-tile fog map: fog value (0=bright, 256=black) for each tile.
    /// Combines FOV visibility with distance-based light falloff.
    fog_map: Vec<i16>,
    map_width: i32,
    map_height: i32,
    /// Light source position in world space.
    light_pos: Vec3,
}

impl<'a> RasterSink<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        fb: &'a mut Framebuffer,
        mvp: Mat4,
        cam_right: Vec3,
        fog_map: Vec<i16>,
        map_width: i32,
        map_height: i32,
        light_pos: Vec3,
    ) -> Self {
        let width = fb.width() as i32;
        let height = fb.height() as i32;
        Self {
            fb,
            mvp,
            width,
            height,
            cam_right,
            fog_map,
            map_width,
            map_height,
            light_pos,
        }
    }

    /// Compute per-vertex lighting: `brightness = visibility × lambert × attenuation`.
    ///
    /// - **Visibility**: fog map (smooth FOV envelope: 1 near player, 0 at boundary)
    /// - **Lambert**: `dot(N, normalize(L))` — angular shading
    /// - **Attenuation**: `1 / (1 + k·d²)` — physical inverse-square light falloff
    ///
    /// Distance is part of the light equation, not a separate fog system.
    /// This ensures a nearby floor is always brighter than a distant wall.
    #[inline]
    fn vertex_light(&self, vertex: Vec3, normal: Vec3) -> i16 {
        // FOV visibility envelope from fog map (0 = fully visible, 256 = dark)
        let tx = vertex.x.to_int().clamp(0, self.map_width - 1);
        let tz = vertex.z.to_int().clamp(0, self.map_height - 1);
        let fog_vis = self.fog_map[(tz * self.map_width + tx) as usize] as i64;
        if fog_vis >= 256 {
            return 256;
        }
        let visibility = 256 - fog_vis; // 0..256

        // Direction + distance to light
        let to_light = self.light_pos - vertex;
        let dist = to_light.length();

        if dist.to_raw() == 0 {
            return (256 - visibility) as i16; // at light — full brightness modulated by visibility
        }

        // Lambert: dot(N, normalize(to_light))
        let inv_dist = Fixed16::ONE / dist;
        let light_dir = Vec3::new(
            to_light.x * inv_dist,
            to_light.y * inv_dist,
            to_light.z * inv_dist,
        );
        let ndotl = normal.dot(light_dir);

        let lambert = if ndotl.to_raw() <= 0 {
            AMBIENT as i64
        } else {
            (ndotl.to_raw() >> 8).clamp(0, 256).max(AMBIENT) as i64
        };

        // Inverse-square attenuation: 256 / (1 + k·d²)
        // d² in tiles (integer): to_light.length_squared().to_int()
        let dist_sq_tiles = to_light.length_squared().to_int().max(1) as i64;
        let atten = (256 * ATTEN_SCALE) / (ATTEN_SCALE + dist_sq_tiles); // 0..256

        // Combine: brightness = visibility × lambert × attenuation / 256²
        let brightness = visibility * lambert * atten / (256 * 256);
        (256 - brightness.clamp(0, 256)) as i16
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

        // Billboard normal: face toward camera (perpendicular to right vector, in XZ plane)
        // For lighting, billboards always face the light → use a normal that gives good results
        let bb_normal = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO); // up-facing for uniform lighting
        let center = Vec3::new(center_x, Fixed16::HALF, center_z);
        let fog = self.vertex_light(center, bb_normal);

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
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16) {
        // Per-vertex lighting: fog map (distance + visibility) × Lambert (face angle)
        let f0 = self.vertex_light(v0, normal);
        let f1 = self.vertex_light(v1, normal);
        let f2 = self.vertex_light(v2, normal);

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
    let (mw, mh) = view.map_dims();

    // Build per-tile fog map: combines FOV visibility with distance falloff.
    // Visible tiles get distance-based fog. All other tiles get max fog (black).
    let mut fog_map = vec![256i16; (mw * mh) as usize];
    for gz in 0..mh {
        for gx in 0..mw {
            if view.is_visible(gx, gz) {
                let dx = gx - px;
                let dz = gz - py;
                let dist_sq = dx * dx + dz * dz;
                fog_map[(gz * mw + gx) as usize] = if dist_sq <= FOG_START_SQ {
                    0
                } else if dist_sq >= FOG_END_SQ {
                    256
                } else {
                    // Quadratic curve: bright near player, steep dropoff at edges.
                    // Feels like torchlight rather than a uniform gradient.
                    let linear = (dist_sq - FOG_START_SQ) * 256 / FOG_RANGE_SQ;
                    (linear * linear / 256) as i16
                };
            }
        }
    }

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

    // Light position: player's tile center, at waist height
    let light_pos = Vec3::new(
        Fixed16::from_int(px) + Fixed16::HALF,
        LIGHT_HEIGHT,
        Fixed16::from_int(py) + Fixed16::HALF,
    );

    // Render map geometry (floors, walls)
    let mut sink = RasterSink::new(fb, mvp, cam_right, fog_map, mw, mh, light_pos);
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
        let fog_map = vec![0i16; 100 * 100]; // no fog for tests
        RasterSink::new(
            fb,
            Mat4::identity(),
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            fog_map,
            100,
            100,
            Vec3::zero(), // light at origin
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

        let n = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO); // up
        // CW winding in world xy-plane → CCW in screen after viewport y-flip.
        // With identity MVP, w=1, z=0, so w+z=1 > 0 (all inside).
        sink.emit(
            Vec3::from_ints(0, 0, 0),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::ZERO),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::ZERO),
            n,
            color,
        );

        assert!(count_colored(&fb) > 0, "all-inside triangle should render");
    }

    #[test]
    fn all_outside_culled() {
        let mut fb = Framebuffer::new(100, 100);
        let mut sink = make_test_sink(&mut fb);
        let color = rgb555(31, 0, 0);
        let n = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);

        // All at z=-2: w+z = 1+(-2) = -1 < 0 → all outside.
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            n,
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
        let n = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);

        // CW winding: v0 and v1 in front (z=0, d=1), v2 behind (z=-2, d=-1).
        // Without clipping this would be culled entirely.
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::ZERO),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            n,
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
        let n = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);

        // CW winding: v0 in front (z=0, d=1), v1 and v2 behind (z=-2, d=-1).
        sink.emit(
            Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::new(Fixed16::ZERO, Fixed16::HALF, Fixed16::from_int(-2)),
            Vec3::new(Fixed16::HALF, Fixed16::ZERO, Fixed16::from_int(-2)),
            n,
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
