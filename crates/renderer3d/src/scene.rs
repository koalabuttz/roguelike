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
/// Distance² (in tiles) where fog reaches full black.
/// Set inside the FOV radius (8 tiles = 64) so the per-vertex distance fog
/// fades to black before the FOV boundary. This hides the FOV shadowcasting
/// staircase — tiles at the FOV edge are already dark from distance.
const FOG_END_SQ: i32 = 36; // 6 tiles (FOV_RADIUS is 8)

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

// --- Torch light color (warm amber, ~1800K blackbody) ---

/// Base torch color: per-channel brightness 0..256.
/// Full red, warm green, low blue → amber/orange glow.
const TORCH_R: u16 = 256;
const TORCH_G: u16 = 210;
const TORCH_B: u16 = 110;

// --- Flicker animation ---

/// Quake-inspired flame flicker table. Each entry is a brightness multiplier
/// (0..256). The flame mostly stays near full brightness with occasional dips
/// and the rare sharp drop — matching real candle/torch behavior.
///
/// 32 entries at ~15 fps effective rate (sampled every 4 frames at 60 fps)
/// gives a ~2 second loop, long enough to avoid visible repetition.
#[rustfmt::skip]
const FLICKER_TABLE: [u16; 32] = [
    240, 245, 235, 250, 230, 248, 225, 252, // gentle undulation
    245, 220, 250, 240, 255, 235, 210, 245, // slight dip at 210
    250, 248, 230, 255, 240, 195, 250, 245, // deeper dip at 195
    235, 252, 248, 230, 245, 240, 255, 250, // back to normal
];

/// Compute the per-frame light color from the flicker table.
///
/// The flicker modulates overall brightness while preserving the warm tint.
/// A secondary effect: at lower brightness, the color shifts slightly *warmer*
/// (more red-dominant) — matching real flames where a dimmer flame burns cooler.
fn torch_light_color(frame: u32) -> [u16; 3] {
    // Sample the table at 1/4 frame rate for naturalistic low-frequency wobble
    let idx = ((frame / 4) % FLICKER_TABLE.len() as u32) as usize;
    let brightness = FLICKER_TABLE[idx] as u32;

    // Interpolate between current and next entry for smoother transitions
    let next_idx = (idx + 1) % FLICKER_TABLE.len();
    let next_brightness = FLICKER_TABLE[next_idx] as u32;
    let sub_frame = frame % 4;
    let smooth = (brightness * (4 - sub_frame) + next_brightness * sub_frame) / 4;

    // Apply brightness to each channel. The warm tint comes from the base
    // color ratio (R > G > B); flicker modulates all channels linearly.
    let r = (TORCH_R as u32 * smooth / 256).min(256) as u16;
    let g = (TORCH_G as u32 * smooth / 256).min(256) as u16;
    let b = (TORCH_B as u32 * smooth / 256).min(256) as u16;

    [r, g, b]
}

/// Compute per-vertex lighting as a fog factor (0..256).
///
/// Returns an opacity-style fog factor where 0 = fully lit and 256 =
/// fully dark. The rasterizer (or a hardware equivalent) applies the
/// fog factor to tint the base surface color.
///
/// The composite formula is
/// `brightness = visibility × lambert × attenuation / 256²`, with the
/// return value being `256 - brightness`. Components:
///
/// - **Visibility (distance ramp)**: linear 256..0 as distance² crosses
///   [FOG_START_SQ, FOG_END_SQ] (in Fixed16-raw units), matching the
///   per-tile fog map used by the FOV system.
/// - **Lambert**: `dot(normal, normalize(light_pos - vertex))`, clamped
///   to `[AMBIENT, 256]` so back-facing surfaces still receive a minimum
///   indirect-light floor.
/// - **Attenuation**: `256·k / (k + d²)` inverse-square falloff using
///   the full 3D distance (including camera and light height), with
///   `k = ATTEN_SCALE << 16`.
///
/// Extracted from `RasterSink::vertex_light` so that both the software
/// rasterizer and the DS hardware-3D sink (`crates/nds/src/gpu_sink.rs`)
/// can share the same lighting math without duplication.
pub fn vertex_light(vertex: Vec3, normal: Vec3, light_pos: Vec3) -> i16 {
    // Smooth distance fog from vertex world position — full Fixed16
    // precision. Using raw Fixed16 values (1 unit = 65536) avoids
    // integer quantization that would create visible circular
    // brightness bands.
    let dx = vertex.x - light_pos.x;
    let dz = vertex.z - light_pos.z;
    let dist_sq_raw = (dx * dx + dz * dz).to_raw().max(0) as i64;
    let fog_start_raw = (FOG_START_SQ as i64) << 16;
    let fog_end_raw = (FOG_END_SQ as i64) << 16;
    let fog_range_raw = fog_end_raw - fog_start_raw;

    let visibility = if dist_sq_raw <= fog_start_raw {
        256i64
    } else if dist_sq_raw >= fog_end_raw {
        0
    } else {
        256 * (fog_end_raw - dist_sq_raw) / fog_range_raw
    };

    if visibility <= 0 {
        return 256;
    }

    // Direction + distance to light (3D, includes height)
    let to_light = light_pos - vertex;
    let dist = to_light.length();

    if dist.to_raw() == 0 {
        return (256 - visibility) as i16;
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

    // Inverse-square attenuation: 256·k / (k + d²), using Fixed16 precision.
    let dist_sq_3d_raw = to_light.length_squared().to_raw().max(1) as i64;
    let atten_k = ATTEN_SCALE << 16; // scale constant to Fixed16
    let atten = (256 * atten_k) / (atten_k + dist_sq_3d_raw);

    // Combine: brightness = visibility × lambert × attenuation / 256²
    let brightness = visibility * lambert * atten / (256 * 256);
    (256 - brightness.clamp(0, 256)) as i16
}

/// Variant of [`vertex_light`] that omits the distance-visibility term.
///
/// Returns `brightness = lambert × attenuation / 256`, then
/// `256 - brightness` as a fog factor. Used by the DS hardware sink
/// (`crates/nds/src/gpu_sink.rs`) where the DS 3D engine's hardware
/// fog unit provides the per-fragment distance darkening — running
/// the software visibility term there too would double-darken the
/// frame.
///
/// The contract is: `vertex_light_no_visibility(v, n, l)` equals
/// `vertex_light(v, n, l)` whenever the full-visibility case (`dist²
/// ≤ FOG_START_SQ`) is in effect, i.e. close enough to the light
/// source that `visibility = 256`.
pub fn vertex_light_no_visibility(vertex: Vec3, normal: Vec3, light_pos: Vec3) -> i16 {
    // Direction + distance to light (3D, includes height)
    let to_light = light_pos - vertex;
    let dist = to_light.length();

    if dist.to_raw() == 0 {
        // At the light source: fully lit.
        return 0;
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

    // Inverse-square attenuation: 256·k / (k + d²), using Fixed16 precision.
    let dist_sq_3d_raw = to_light.length_squared().to_raw().max(1) as i64;
    let atten_k = ATTEN_SCALE << 16; // scale constant to Fixed16
    let atten = (256 * atten_k) / (atten_k + dist_sq_3d_raw);

    // brightness = lambert × attenuation / 256 (visibility = 256 folds out)
    let brightness = lambert * atten / 256;
    (256 - brightness.clamp(0, 256)) as i16
}

/// Rasterizing triangle sink: transforms world-space triangles through
/// the MVP matrix and rasterizes them into the framebuffer.
struct RasterSink<'a> {
    fb: &'a mut Framebuffer,
    mvp: Mat4,
    width: i32,
    height: i32,
    cam_right: Vec3,
    /// Light source position in world space.
    light_pos: Vec3,
    /// Per-frame light color (warm tint + flicker modulation).
    light_color: [u16; 3],
}

impl<'a> RasterSink<'a> {
    fn new(
        fb: &'a mut Framebuffer,
        mvp: Mat4,
        cam_right: Vec3,
        light_pos: Vec3,
        light_color: [u16; 3],
    ) -> Self {
        let width = fb.width() as i32;
        let height = fb.height() as i32;
        Self {
            fb,
            mvp,
            width,
            height,
            cam_right,
            light_pos,
            light_color,
        }
    }

    /// Thin wrapper forwarding to the standalone `vertex_light()` at
    /// module level. Kept so `self.vertex_light(v, n)` still reads well
    /// inside RasterSink's methods.
    #[inline]
    fn vertex_light(&self, vertex: Vec3, normal: Vec3) -> i16 {
        vertex_light(vertex, normal, self.light_pos)
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
        rasterize_triangle(self.fb, s0, s1, s2, color, self.light_color);
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

        rasterize_glyph_triangle(
            self.fb,
            s0,
            s1,
            s2,
            color,
            self.light_color,
            uv0,
            uv1,
            uv2,
            glyph,
        );
    }
}

impl TriangleSink for RasterSink<'_> {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16) {
        let f0 = self.vertex_light(v0, normal);
        let f1 = self.vertex_light(v1, normal);
        let f2 = self.vertex_light(v2, normal);

        // Skip fully dark triangles — all vertices at max fog render as pure black,
        // contributing nothing visible. Saves the MVP transform + rasterization.
        if f0 >= 256 && f1 >= 256 && f2 >= 256 {
            return;
        }

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
///
/// `frame`: monotonically increasing frame counter. Drives the torch flicker
/// animation — each frame produces a slightly different light color/intensity.
/// Pass 0 for a static snapshot (e.g., PPM output).
pub fn render_scene(view: &dyn GameView, fb: &mut Framebuffer, frame: u32) {
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

    // Light position: player's tile center, at waist height
    let light_pos = Vec3::new(
        Fixed16::from_int(px) + Fixed16::HALF,
        LIGHT_HEIGHT,
        Fixed16::from_int(py) + Fixed16::HALF,
    );

    // Compute per-frame torch light color (warm tint + flicker)
    let light_color = torch_light_color(frame);

    // Render map geometry (floors, walls)
    let mut sink = RasterSink::new(fb, mvp, cam_right, light_pos, light_color);
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
    /// Player at origin, white light.
    fn make_test_sink(fb: &mut Framebuffer) -> RasterSink<'_> {
        RasterSink::new(
            fb,
            Mat4::identity(),
            Vec3::new(Fixed16::ONE, Fixed16::ZERO, Fixed16::ZERO),
            Vec3::zero(),    // light at origin
            [256, 256, 256], // white light for tests
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

        render_scene(&game, &mut fb, 0);

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

        render_scene(&game_a, &mut fb_a, 0);
        render_scene(&game_b, &mut fb_b, 0);

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

        render_scene(&game, &mut fb, 0);

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

    // --- Torch flicker tests ---

    #[test]
    fn torch_color_is_warm() {
        let color = torch_light_color(0);
        // Red should be brightest, blue dimmest
        assert!(
            color[0] > color[1],
            "red ({}) > green ({})",
            color[0],
            color[1]
        );
        assert!(
            color[1] > color[2],
            "green ({}) > blue ({})",
            color[1],
            color[2]
        );
    }

    #[test]
    fn torch_flicker_varies_over_time() {
        // Sample at different frames — brightness should vary
        let colors: Vec<_> = (0..32).map(|f| torch_light_color(f * 4)).collect();
        let min_r = colors.iter().map(|c| c[0]).min().unwrap();
        let max_r = colors.iter().map(|c| c[0]).max().unwrap();
        assert!(
            max_r > min_r,
            "flicker should produce varying brightness: min={min_r}, max={max_r}"
        );
    }

    #[test]
    fn torch_flicker_never_goes_dark() {
        // Flicker should never drop below ~60% brightness
        for frame in 0..256 {
            let color = torch_light_color(frame);
            assert!(
                color[0] >= 150,
                "red too dim at frame {frame}: {}",
                color[0]
            );
        }
    }

    #[test]
    fn different_frames_produce_different_pixels() {
        let game = MicroGameState::new_default(42);
        let mut fb_a = Framebuffer::new(80, 60);
        let mut fb_b = Framebuffer::new(80, 60);

        render_scene(&game, &mut fb_a, 0);
        render_scene(&game, &mut fb_b, 16); // different flicker phase

        let mut differs = false;
        'outer: for y in 0..fb_a.height() {
            for x in 0..fb_a.width() {
                if fb_a.get_pixel(x, y) != fb_b.get_pixel(x, y) {
                    differs = true;
                    break 'outer;
                }
            }
        }
        assert!(
            differs,
            "different frames should produce different images (flicker)"
        );
    }

    // --- vertex_light_no_visibility parity ---

    #[test]
    fn no_visibility_matches_full_visibility_case() {
        // When the vertex is close enough to the light that the visibility
        // term in vertex_light() saturates at 256 (i.e. dist² ≤ FOG_START_SQ),
        // vertex_light_no_visibility() should return exactly the same value.
        // This is the contract: hardware fog in the DS pipeline replaces
        // the software visibility term, so the two functions must agree in
        // the "fully visible" case.
        let light_pos = Vec3::new(
            Fixed16::from_int(5),
            Fixed16::from_raw(0x28000), // LIGHT_HEIGHT = 2.5
            Fixed16::from_int(5),
        );
        // Vertex at ~1 tile horizontal, on the floor — dist² ≈ 1²+2.5² ≈ 7.25,
        // which is > FOG_START_SQ=4. Move closer to make visibility saturate.
        // Vertex at 0.5 tile horizontal, y=0: dist² = 0.25 + 6.25 = 6.5. Still > 4.
        // Try vertex directly below the light at y=0: dist² = 0 + 6.25 = 6.25. Still > 4.
        //
        // The visibility term is based on horizontal distance squared
        // (dx² + dz², not dy²), per the source: it only uses x/z components.
        // So a vertex at horizontal distance 0 gives visibility=256.
        let vertex = Vec3::new(Fixed16::from_int(5), Fixed16::ZERO, Fixed16::from_int(5));
        let normal = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO); // floor up

        let with_vis = vertex_light(vertex, normal, light_pos);
        let without_vis = vertex_light_no_visibility(vertex, normal, light_pos);
        assert_eq!(
            with_vis, without_vis,
            "parity violated: vertex_light={with_vis}, no_visibility={without_vis}"
        );
    }

    #[test]
    fn no_visibility_is_brighter_than_full_at_distance() {
        // At a distance where the visibility term darkens the output
        // (dist² well above FOG_END_SQ=36), the no-visibility variant
        // should return a strictly smaller (= brighter) fog factor.
        let light_pos = Vec3::new(
            Fixed16::from_int(0),
            Fixed16::from_raw(0x28000),
            Fixed16::from_int(0),
        );
        // 8 tiles away horizontally: dist² = 64, well past FOG_END_SQ=36.
        let vertex = Vec3::new(Fixed16::from_int(8), Fixed16::ZERO, Fixed16::ZERO);
        let normal = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);

        let with_vis = vertex_light(vertex, normal, light_pos);
        let without_vis = vertex_light_no_visibility(vertex, normal, light_pos);
        assert!(
            without_vis < with_vis,
            "no-visibility fog should be smaller (brighter) at distance: \
             with_vis={with_vis}, without_vis={without_vis}"
        );
    }

    #[test]
    fn no_visibility_at_light_source_is_fully_lit() {
        // Vertex exactly at the light position — dist = 0, so we return
        // the fully-lit value (0).
        let light_pos = Vec3::new(
            Fixed16::from_int(3),
            Fixed16::from_int(1),
            Fixed16::from_int(3),
        );
        let normal = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
        let fog = vertex_light_no_visibility(light_pos, normal, light_pos);
        assert_eq!(fog, 0);
    }
}
