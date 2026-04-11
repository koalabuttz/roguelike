//! DS hardware-3D sink: writes GXFIFO commands instead of rasterizing.
//!
//! Phase 1 replacement for `roguelike_renderer3d::scene::RasterSink`.
//! Implements [`TriangleSink`] and, for each triangle emitted by
//! `geometry::generate_map_geometry`, performs:
//!
//! 1. Per-vertex fog via `renderer3d::scene::vertex_light` (shared with
//!    the software path — same formula, identical fog output)
//! 2. Dark-triangle skip (all three vertices at full fog)
//! 3. Software MVP transform to clip space
//! 4. Near-plane rejection (Phase 1 culls any triangle crossing the near
//!    plane rather than clipping — acceptable because walls near the
//!    camera are rare and Phase 2 can add proper clipping if needed)
//! 5. Software perspective divide to NDC via `Vec4::perspective_divide`
//! 6. Per-vertex color tinting via `renderer3d::rasterizer::apply_fog`
//!    (no dither — hardware Gouraud interpolates across the triangle)
//! 7. RGB555 swizzle from renderer3d (R<<10|G<<5|B) to DS hardware
//!    convention (B<<10|G<<5|R)
//! 8. Emission via GXFIFO (`BEGIN`, `COLOR`+`VTX_16` ×3, `END`)
//!
//! Billboards are not emitted in Phase 1 — `render_scene_ds` skips the
//! entity and item walks. They return in Phase 3 with A3I5 textures.
//!
//! ## Why pre-tinted vertex colors?
//!
//! The software path interpolates the *fog factor* per-pixel, then
//! tints the base color via `apply_fog` at pixel time. DS hardware
//! Gouraud-interpolates *vertex colors* instead — it has no concept of
//! per-vertex fog factor interpolation. So we apply the fog tint at
//! vertex time on all three vertices, and hardware interpolation of
//! those tinted colors closely approximates the software per-pixel
//! tint. Not bit-identical but visually very close.

use core::cell::UnsafeCell;

use roguelike_core::rules::game_view::GameView;
use roguelike_renderer3d::geometry::{self, TriangleSink};
use roguelike_renderer3d::math::{Fixed16, Mat4, Vec3};
use roguelike_renderer3d::rasterizer::apply_fog;
use roguelike_renderer3d::scene::vertex_light;

use crate::{gx, read_timer32};

// ---------------------------------------------------------------------------
// Timing instrumentation
// ---------------------------------------------------------------------------

/// How many timer ticks the last call to `generate_map_geometry` took,
/// stored in a Sync-wrapped UnsafeCell so the main-loop HUD can read it
/// without needing atomics (armv5te has no hardware CAS).
struct GenTicks(UnsafeCell<u32>);
// SAFETY: DS is single-threaded; there is no concurrent access.
unsafe impl Sync for GenTicks {}

static LAST_GEN_TICKS: GenTicks = GenTicks(UnsafeCell::new(0));

/// Returns the time (in bus-clock ticks) that the most recent
/// `render_scene_ds` spent inside `generate_map_geometry`.
pub fn last_gen_ticks() -> u32 {
    unsafe { *LAST_GEN_TICKS.0.get() }
}

// ---------------------------------------------------------------------------
// Camera + light constants (duplicated from renderer3d::scene so the
// hardware path matches the software path's visual setup exactly).
// A SceneCamera extraction in renderer3d is deferred to a later phase.
// ---------------------------------------------------------------------------

const CAMERA_HEIGHT: Fixed16 = Fixed16::from_int(10);
const CAMERA_TILT_OFFSET: Fixed16 = Fixed16::from_int(5);

/// 60° vertical FOV as a fraction of a full turn (Fixed16::ONE = 360°).
const FOV: Fixed16 = Fixed16::from_raw(0x2AAA);
const NEAR: Fixed16 = Fixed16::ONE;
const FAR: Fixed16 = Fixed16::from_int(60);

/// Light source above the floor (torch at waist height).
const LIGHT_HEIGHT: Fixed16 = Fixed16::from_raw(0x28000);

// Warm torch base color — amber, ~1800K blackbody
const TORCH_R: u16 = 256;
const TORCH_G: u16 = 210;
const TORCH_B: u16 = 110;

/// Quake-style torch flicker table. 32 brightness samples (0..256),
/// sampled at 1/4 frame rate for a ~2 second loop.
#[rustfmt::skip]
const FLICKER_TABLE: [u16; 32] = [
    240, 245, 235, 250, 230, 248, 225, 252,
    245, 220, 250, 240, 255, 235, 210, 245,
    250, 248, 230, 255, 240, 195, 250, 245,
    235, 252, 248, 230, 245, 240, 255, 250,
];

/// Compute the per-frame light color from the flicker table with
/// sub-frame interpolation. Matches `scene::torch_light_color`.
fn torch_light_color(frame: u32) -> [u16; 3] {
    let idx = ((frame / 4) % FLICKER_TABLE.len() as u32) as usize;
    let brightness = FLICKER_TABLE[idx] as u32;
    let next_idx = (idx + 1) % FLICKER_TABLE.len();
    let next_brightness = FLICKER_TABLE[next_idx] as u32;
    let sub_frame = frame % 4;
    let smooth = (brightness * (4 - sub_frame) + next_brightness * sub_frame) / 4;
    let r = (TORCH_R as u32 * smooth / 256).min(256) as u16;
    let g = (TORCH_G as u32 * smooth / 256).min(256) as u16;
    let b = (TORCH_B as u32 * smooth / 256).min(256) as u16;
    [r, g, b]
}

// ---------------------------------------------------------------------------
// GpuSink
// ---------------------------------------------------------------------------

/// Cull any triangle whose centroid is more than `sqrt(64) = 8` tiles
/// from the light source in the XZ plane. `FOG_END_SQ` in scene.rs is
/// `36` (≈ 6 tiles), so 8 tiles gives a ~2-tile safety margin to keep
/// triangles that straddle the fog boundary.
///
/// Math: `dx = 3·(centroid.x − light.x)`, so `dx² + dz² = 9 · dist²`.
/// We compare against `9 × 64 × Fixed16::ONE_RAW = 9 × 64 × 65536` to
/// avoid having to actually divide the sum by 3.
const CENTROID_CULL_TILES_SQ: i64 = 64;
const CENTROID_CULL_THRESHOLD_RAW: i64 = 9 * CENTROID_CULL_TILES_SQ * 65536;

/// Triangle sink that emits DS GX commands instead of rasterizing.
pub struct GpuSink {
    mvp: Mat4,
    light_pos: Vec3,
    /// Pre-scaled `light_pos.x * 3` and `light_pos.z * 3`, used by the
    /// centroid cull so we don't multiply `light_pos` by 3 per triangle.
    light_x_3: Fixed16,
    light_z_3: Fixed16,
    light_color: [u16; 3],
}

impl GpuSink {
    pub fn new(mvp: Mat4, light_pos: Vec3, light_color: [u16; 3]) -> Self {
        let three = Fixed16::from_int(3);
        Self {
            mvp,
            light_pos,
            light_x_3: light_pos.x * three,
            light_z_3: light_pos.z * three,
            light_color,
        }
    }

    /// Convert a Fixed16 value expected to be in NDC range `[-1, +1]`
    /// into DS VTX_16 s.3.12 fixed-point format.
    ///
    /// Fixed16 has 16 fractional bits, s.3.12 has 12 — the conversion
    /// is a right shift by 4. Values outside the s.3.12 representable
    /// range ([-8, +8)) are clamped; the hardware will then clip them
    /// against the frustum during its own pipeline. Clamping here is
    /// necessary to avoid silent integer truncation when NDC values
    /// exceed ±8 for vertices that are well outside the view frustum.
    #[inline]
    fn ndc_to_s3_12(value: Fixed16) -> i16 {
        let raw = value.to_raw() >> 4;
        raw.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// Tint the base color by fog and light, swizzle to DS format,
    /// and write it to the GX COLOR register.
    ///
    /// `dither = 0` because the DS 3D engine Gouraud-interpolates
    /// vertex colors across the triangle — the per-pixel Bayer dither
    /// used by the software rasterizer isn't needed (or representable)
    /// in the hardware pipeline.
    #[inline]
    fn emit_color(&self, base_color: u16, fog: i16) {
        let tinted = apply_fog(base_color, fog, 0, self.light_color);
        let ds_color = gx::swizzle_gl_to_ds(tinted);
        unsafe {
            gx::color(ds_color);
        }
    }

    /// Emit one vertex at NDC coordinates (x, y, z).
    #[inline]
    fn emit_vertex(&self, ndc: Vec3) {
        let x = Self::ndc_to_s3_12(ndc.x);
        let y = Self::ndc_to_s3_12(ndc.y);
        let z = Self::ndc_to_s3_12(ndc.z);
        unsafe {
            gx::vtx_16(x, y, z);
        }
    }
}

impl TriangleSink for GpuSink {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16) {
        // ---- B.2: Cheap centroid cull ----
        //
        // Before doing any expensive per-vertex work, check whether the
        // triangle's centroid is even close enough to the light source to
        // be visible. Rejects the ~60 % of map-radius triangles that are
        // past the fog-end distance and would only produce fully-dark
        // pixels. Skips 3 × vertex_light (including sqrt + divides) plus
        // the MVP transform for those triangles.
        //
        // We compare `9 · dist²` to the pre-scaled threshold to avoid a
        // division by 3 — `dx = 3·(centroid − light)` because we use
        // the sum of the three vertex coordinates directly.
        let sum_x = v0.x + v1.x + v2.x;
        let sum_z = v0.z + v1.z + v2.z;
        let dx = sum_x - self.light_x_3;
        let dz = sum_z - self.light_z_3;
        let dist_sq_9x = (dx * dx + dz * dz).to_raw() as i64;
        if dist_sq_9x > CENTROID_CULL_THRESHOLD_RAW {
            return;
        }

        // ---- Per-vertex lighting (expensive: sqrt + divides) ----
        let f0 = vertex_light(v0, normal, self.light_pos);
        let f1 = vertex_light(v1, normal, self.light_pos);
        let f2 = vertex_light(v2, normal, self.light_pos);

        // Dark-triangle skip — matches RasterSink's optimization.
        if f0 >= 256 && f1 >= 256 && f2 >= 256 {
            return;
        }

        // ---- Software MVP transform into clip space ----
        let c0 = self.mvp * v0.to_point();
        let c1 = self.mvp * v1.to_point();
        let c2 = self.mvp * v2.to_point();

        // Near-plane classification. `w + z < 0` means behind the near
        // plane (standard reverse-Z-ish convention used by the renderer3d
        // perspective matrix).
        let d0 = c0.w + c0.z;
        let d1 = c1.w + c1.z;
        let d2 = c2.w + c2.z;
        let in0 = d0.to_raw() >= 0;
        let in1 = d1.to_raw() >= 0;
        let in2 = d2.to_raw() >= 0;
        let count = in0 as u8 + in1 as u8 + in2 as u8;

        // Phase 1: any triangle that crosses the near plane is culled
        // entirely rather than clipped. RasterSink has proper clipping
        // logic; porting that to emit interpolated sub-triangles is
        // deferred. In practice walls rarely straddle the near plane
        // because the camera is 10 units above the floor.
        if count < 3 {
            return;
        }

        // ---- B.1: Reciprocal-multiply perspective divide ----
        //
        // Each `Vec4::perspective_divide` would do 3 Fixed16 divides.
        // Precomputing `1/w` once and multiplying by it drops the cost
        // to 1 divide + 3 multiplies per vertex — a ~500-cycle saving
        // per triangle on ARM9 (which has no hardware i64 divide).
        let inv_w0 = Fixed16::ONE / c0.w;
        let inv_w1 = Fixed16::ONE / c1.w;
        let inv_w2 = Fixed16::ONE / c2.w;
        let ndc0 = Vec3::new(c0.x * inv_w0, c0.y * inv_w0, c0.z * inv_w0);
        let ndc1 = Vec3::new(c1.x * inv_w1, c1.y * inv_w1, c1.z * inv_w1);
        let ndc2 = Vec3::new(c2.x * inv_w2, c2.y * inv_w2, c2.z * inv_w2);

        // Emit the triangle: COLOR + VTX_16 per vertex, bracketed by
        // BEGIN_VTXS / END_VTXS.
        unsafe {
            gx::begin(gx::PRIM_TRIANGLES);
        }
        self.emit_color(color, f0);
        self.emit_vertex(ndc0);
        self.emit_color(color, f1);
        self.emit_vertex(ndc1);
        self.emit_color(color, f2);
        self.emit_vertex(ndc2);
        unsafe {
            gx::end();
        }
    }
}

// ---------------------------------------------------------------------------
// render_scene_ds
// ---------------------------------------------------------------------------

/// DS hardware 3D entry point — mirrors `renderer3d::scene::render_scene`.
///
/// Sets up the same camera / light / torch flicker as the software path,
/// constructs a [`GpuSink`], and walks the visible map geometry through
/// `generate_map_geometry`. Phase 1 does not emit entity or item
/// billboards.
///
/// The DS 3D engine hardware clears and depth-tests internally; no
/// framebuffer clear is necessary. The caller is responsible for
/// calling [`gx::swap_buffers`] after this function returns to commit
/// the frame for display.
pub fn render_scene_ds(view: &dyn GameView, frame: u32) {
    let (px, py) = view.player_xy();

    // Camera looks down at the player from above and slightly behind.
    let target = Vec3::new(
        Fixed16::from_int(px) + Fixed16::HALF,
        Fixed16::ZERO,
        Fixed16::from_int(py) + Fixed16::HALF,
    );
    let eye = Vec3::new(target.x, CAMERA_HEIGHT, target.z - CAMERA_TILT_OFFSET);
    let up = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
    let view_mat = Mat4::look_at(eye, target, up);

    // DS top screen is 256×192, aspect 4:3 in Fixed16.
    let aspect = Fixed16::from_raw((((256i64) << 16) / 192) as i32);
    let proj_mat = Mat4::perspective(FOV, aspect, NEAR, FAR);
    let mvp = proj_mat * view_mat;

    // Torch light at the player's waist.
    let light_pos = Vec3::new(
        Fixed16::from_int(px) + Fixed16::HALF,
        LIGHT_HEIGHT,
        Fixed16::from_int(py) + Fixed16::HALF,
    );
    let light_color = torch_light_color(frame);

    let mut sink = GpuSink::new(mvp, light_pos, light_color);

    // Profiling: measure time spent walking map geometry + emitting
    // through GpuSink. Exposed via `last_gen_ticks()` for the debug HUD.
    let gen_t0 = read_timer32();
    geometry::generate_map_geometry(view, &mut sink);
    let gen_t1 = read_timer32();
    unsafe {
        *LAST_GEN_TICKS.0.get() = gen_t1.wrapping_sub(gen_t0);
    }

    // Phase 1 does not render entity / item billboards. Phase 3 adds
    // A3I5 textured billboards via TEXIMAGE_PARAM + TEXCOORD + a glyph
    // font uploaded to a VRAM texture slot.
}
