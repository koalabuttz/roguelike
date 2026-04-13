//! DS hardware-3D sink: writes GXFIFO commands instead of rasterizing.
//!
//! Phase 2 restructuring (#306): the DS hardware matrix stack now owns
//! projection + perspective divide. `render_scene_ds` uploads view and
//! projection matrices via [`gx::mtx_load_4x4`] each frame, and
//! `GpuSink::emit` emits **player-relative, half-scaled world-space**
//! vertices via `VTX_16` — letting the hardware transform them.
//!
//! Per-triangle pipeline:
//!
//! 1. CPU-side centroid cull (rejects ~60% of geometry past fog-end)
//! 2. Per-vertex lighting via [`vertex_light_no_visibility`] (Lambert
//!    + attenuation only — hardware fog provides distance darkening)
//! 3. Dark-triangle skip
//! 4. Color tint via [`apply_fog`] with the computed lighting factor
//! 5. RGB555 swizzle from renderer3d to DS hardware convention
//! 6. Vertex emission via `VTX_16` after player-relative translation
//!    and a 1/2 scale so coords fit in s.3.12's [-8, +8) range
//!
//! The hardware then runs MVP × vertex, perspective divide, frustum
//! clip, viewport transform, and per-fragment fog lookup.
//!
//! ## Why player-relative + 1/2 scale?
//!
//! The DS `VTX_16` command takes signed 16-bit coordinates in s.3.12
//! format — range `[-8, +8)` world units. Our map is 80×40 tiles so
//! world coordinates can reach ±80, and render-radius geometry
//! extends ±11 tiles from the player. Pre-translating vertices by
//! the player position gives `≤ ±11` magnitude; dividing by 2 gives
//! `≤ ±5.5`, safely inside the format range.
//!
//! The matching POSITION matrix is `look_at(eye_rel, target_rel, up) ×
//! scale(2)` — the scale(2) factor cancels the /2 on the vertex side,
//! and the player-relative camera produces the same rotation as the
//! world-space camera (look_at's basis is translation-invariant).
//!
//! Phase 1's software MVP, near-plane reject, Cohen-Sutherland outcode,
//! and reciprocal perspective divide are all deleted — the hardware
//! pipeline handles them. Centroid cull and dark-triangle skip remain
//! as CPU-side savings (they reject triangles before we pay GXFIFO
//! bandwidth for them).
//!
//! Billboards are not emitted in Phase 1/2 — `render_scene_ds` skips
//! the entity and item walks. They return in Phase 3 with A3I5 textures.
//!
//! ## Why pre-tinted vertex colors?
//!
//! The software path interpolates the *fog factor* per-pixel, then
//! tints the base color via `apply_fog` at pixel time. DS hardware
//! Gouraud-interpolates *vertex colors* instead — it has no concept of
//! per-vertex fog factor interpolation. So we apply the Lambert /
//! attenuation tint at vertex time on all three vertices; hardware
//! then interpolates and layers its own per-fragment distance fog
//! on top.

use core::cell::UnsafeCell;

use roguelike_core::rules::color::GameColor;
use roguelike_core::rules::game_view::{GameView, TileVisibility};
use roguelike_renderer3d::color_map::game_color_to_rgb555;
use roguelike_renderer3d::font;
use roguelike_renderer3d::geometry::{self, TriangleSink};
use roguelike_renderer3d::math::{Fixed16, Mat4, Vec3};
use roguelike_renderer3d::rasterizer::apply_fog;
use roguelike_renderer3d::scene::vertex_light_no_visibility;

use crate::{gx, read_timer32};

// ---------------------------------------------------------------------------
// Compile-time A3I5 glyph atlas
// ---------------------------------------------------------------------------

/// A3I5 texel: alpha=7 (opaque, bits 5-7) + palette index 1 (white, bits 0-4).
const A3I5_FOREGROUND: u8 = (7 << 5) | 1;
/// A3I5 texel: alpha=0 (transparent).
const A3I5_BACKGROUND: u8 = 0;

/// Glyph characters in atlas order. The atlas is a 4×4 grid of 8×8 cells
/// (32×32 texture). Index = row * 4 + col.
const ATLAS_CHARS: [char; 12] = ['@', 'g', 'o', 'T', '!', '/', '[', '>', '%', '.', '\0', '\0'];

/// Map a character to its (col, row) position in the atlas. Unknown
/// characters map to the filled-square fallback at slot (2, 2).
const fn char_to_atlas(ch: char) -> (u8, u8) {
    // Linear search over the 12-entry table. The filled-square fallback
    // for unknown chars lives at slot 10 (col=2, row=2), which we also
    // use for all unknown characters — matching font.rs's behavior of
    // returning a filled square for unrecognized chars.
    let mut i = 0;
    while i < 10 {
        // Can't match on char in const fn, so compare as u32.
        if ATLAS_CHARS[i] as u32 == ch as u32 {
            return ((i % 4) as u8, (i / 4) as u8);
        }
        i += 1;
    }
    // Fallback: filled square at slot 10 (col=2, row=2)
    (2, 2)
}

/// Build the 32×32 A3I5 texture atlas at compile time from the renderer3d
/// 1bpp font glyphs. Each 8×8 glyph occupies one cell in the 4×4 grid.
/// Foreground pixels → A3I5_FOREGROUND (opaque white), background → 0x00
/// (transparent).
const fn build_atlas() -> [u8; 1024] {
    let mut atlas = [0u8; 1024]; // 32 × 32

    let chars: [char; 11] = ['@', 'g', 'o', 'T', '!', '/', '[', '>', '%', '.', '\x00'];
    let mut slot = 0;
    while slot < 11 {
        let col = slot % 4;
        let row = slot / 4;

        // Get the 1bpp glyph. Slot 10 ('\x00') gets the fallback filled square.
        let glyph = if slot < 10 {
            font::glyph(chars[slot])
        } else {
            // Filled square fallback (unknown char)
            font::glyph('\x01') // triggers the _ => filled square branch
        };

        // Convert each texel
        let mut ty = 0;
        while ty < 8 {
            let mut tx = 0;
            while tx < 8 {
                let atlas_x = col * 8 + tx;
                let atlas_y = row * 8 + ty;
                let idx = atlas_y * 32 + atlas_x;

                atlas[idx] = if font::texel(&glyph, tx as u8, ty as u8) {
                    A3I5_FOREGROUND
                } else {
                    A3I5_BACKGROUND
                };
                tx += 1;
            }
            ty += 1;
        }
        slot += 1;
    }
    atlas
}

/// The 32×32 A3I5 texture atlas, computed at compile time and stored in
/// `.rodata`. Uploaded to VRAM Bank B once at init.
pub const GLYPH_ATLAS: [u8; 1024] = build_atlas();

/// 2-entry texture palette for glyph rendering. Entry 0 = black (masked
/// by alpha=0, never visible). Entry 1 = white (modulated by vertex
/// color to produce the entity's GameColor).
pub const GLYPH_PALETTE: [u16; 2] = [0x0000, 0x7FFF];

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
// Runtime-tunable fog parameters
// ---------------------------------------------------------------------------
//
// Exposed via d-pad HUD controls (Select+Up/Down = offset, Select+Left/Right
// = shift) so the fog curve can be tuned on real hardware without rebuilding.
// `render_scene_ds` reads these each frame and re-programs the fog registers.

static FOG_OFFSET: GenTicks = GenTicks(UnsafeCell::new(0x3000));
static FOG_SHIFT: GenTicks = GenTicks(UnsafeCell::new(0));

pub fn fog_offset() -> u32 {
    unsafe { *FOG_OFFSET.0.get() }
}

pub fn fog_shift() -> u32 {
    unsafe { *FOG_SHIFT.0.get() }
}

pub fn set_fog_offset(val: u32) {
    unsafe { *FOG_OFFSET.0.get() = val }
}

pub fn set_fog_shift(val: u32) {
    unsafe { *FOG_SHIFT.0.get() = val }
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
///
/// Holds the CPU-side state needed by per-triangle emission — the
/// world-space light position (for `vertex_light_no_visibility` and
/// the centroid cull), pre-scaled light coords, light color, and
/// the player's world X/Z used to translate vertices into the
/// player-relative frame consumed by the hardware matrix stack.
pub struct GpuSink {
    /// Light source position in world space (used by lighting math).
    light_pos: Vec3,
    /// Pre-scaled `light_pos.x * 3` and `light_pos.z * 3`, used by the
    /// centroid cull so we don't multiply `light_pos` by 3 per triangle.
    light_x_3: Fixed16,
    light_z_3: Fixed16,
    /// Per-frame torch color (with flicker modulation).
    light_color: [u16; 3],
    /// Player world X — subtracted from vertex X before VTX_16 emit.
    player_x: Fixed16,
    /// Player world Z — subtracted from vertex Z before VTX_16 emit.
    player_z: Fixed16,
}

impl GpuSink {
    pub fn new(
        light_pos: Vec3,
        light_color: [u16; 3],
        player_x: Fixed16,
        player_z: Fixed16,
    ) -> Self {
        let three = Fixed16::from_int(3);
        Self {
            light_pos,
            light_x_3: light_pos.x * three,
            light_z_3: light_pos.z * three,
            light_color,
            player_x,
            player_z,
        }
    }

    /// Convert a world-space Fixed16 coordinate into DS `VTX_16` s.3.12
    /// format, after subtracting the player reference and scaling by 1/2.
    ///
    /// The player reference makes geometry camera-relative (vertices
    /// near the player land near zero). The /2 scale packs a ≤11-tile
    /// render radius into the s.3.12 `[-8, +8)` range with margin. The
    /// matching POSITION matrix compensates via `scale(2)`.
    ///
    /// Shift breakdown: `Fixed16` is s.15.16 (16 frac bits), s.3.12 has
    /// 12 frac bits, and `/2` drops one more bit — total `>> 5` on the
    /// raw i32. Still preserves 11 fractional bits of subpixel precision,
    /// plenty for tile-aligned geometry.
    #[inline]
    fn world_to_s3_12_rel(value: Fixed16, player_ref: Fixed16) -> i16 {
        let rel_raw = (value - player_ref).to_raw();
        let raw = rel_raw >> 5;
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

    /// Emit one vertex at world-space coordinates. Subtracts the player
    /// reference position and scales by 1/2 so the submitted value fits
    /// in `VTX_16`'s s.3.12 range.
    #[inline]
    fn emit_vertex(&self, world: Vec3) {
        let x = Self::world_to_s3_12_rel(world.x, self.player_x);
        // Y is height above the floor (0..~10 units), always positive
        // and well below the range limit even without scaling. We still
        // apply the 1/2 scale so the POSITION matrix's scale(2) factor
        // applies uniformly and world geometry stays in proportion.
        let y = Self::world_to_s3_12_rel(world.y, Fixed16::ZERO);
        let z = Self::world_to_s3_12_rel(world.z, self.player_z);
        unsafe {
            gx::vtx_16(x, y, z);
        }
    }
}

impl TriangleSink for GpuSink {
    fn emit(&mut self, v0: Vec3, v1: Vec3, v2: Vec3, normal: Vec3, color: u16) {
        // ---- Cheap centroid cull ----
        //
        // Before doing any expensive per-vertex work, check whether the
        // triangle's centroid is even close enough to the light source to
        // be visible. Rejects the ~60% of map-radius triangles that are
        // past the fog-end distance and would only produce fully-dark
        // pixels. Avoids 3× vertex_light calls plus the GXFIFO writes
        // for those triangles — pure CPU-side win even after the
        // software MVP moved to hardware.
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

        // ---- Per-vertex lighting (Lambert + attenuation, no visibility) ----
        //
        // Hardware fog (DISP3DCNT bit 7 + POLYGON_ATTR bit 15) provides
        // the per-fragment distance darkening that the software path's
        // visibility term did. Running that term here too would
        // double-darken. `vertex_light_no_visibility` drops it, leaving
        // only the Lambert normal term and inverse-square attenuation
        // that create the close-range torch pool.
        let f0 = vertex_light_no_visibility(v0, normal, self.light_pos);
        let f1 = vertex_light_no_visibility(v1, normal, self.light_pos);
        let f2 = vertex_light_no_visibility(v2, normal, self.light_pos);

        // Dark-triangle skip: if Lambert+attenuation alone produces a
        // fully dark result at every vertex, the hardware interpolation
        // would just draw a black triangle. Skip it.
        if f0 >= 256 && f1 >= 256 && f2 >= 256 {
            return;
        }

        // ---- Emit triangle ----
        //
        // No software MVP, near-plane reject, frustum outcode, or
        // perspective divide — the hardware matrix stack and rasterizer
        // handle all of that on the GPU side. Our job is to submit
        // player-relative, 1/2-scaled world-space vertices via VTX_16
        // and let the hardware do the rest.
        unsafe {
            gx::begin(gx::PRIM_TRIANGLES);
        }
        self.emit_color(color, f0);
        self.emit_vertex(v0);
        self.emit_color(color, f1);
        self.emit_vertex(v1);
        self.emit_color(color, f2);
        self.emit_vertex(v2);
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
/// uploads view and projection matrices to the DS hardware matrix stack,
/// constructs a [`GpuSink`], and walks the visible map geometry through
/// `generate_map_geometry`. Phase 1/2 does not emit entity or item
/// billboards (deferred to Phase 3 with A3I5 textures).
///
/// ## Matrix stack setup (Phase 2)
///
/// The hardware matrix stack owns projection + perspective divide. We
/// upload two matrices per frame:
///
/// - `MTX_MODE_PROJECTION` ← `perspective(FOV, aspect, NEAR, FAR)`
/// - `MTX_MODE_POSITION`   ← `look_at(eye_rel, target_rel, up) × scale(2)`
///
/// The view matrix is built in **player-relative space** (both eye and
/// target have the player position subtracted) so that its rotation
/// portion matches the world-space equivalent while its translation
/// term collapses. `GpuSink::emit_vertex` then subtracts the player
/// position from each emitted vertex and scales by 1/2 — together
/// those match the `scale(2)` in POSITION, giving
/// `POSITION × (v_world - player)/2 = view × v_world`.
///
/// ## Why not build the view matrix in world space?
///
/// Because DS `VTX_16` submits coordinates in s.3.12 (range `[-8, +8)`).
/// A world-space vertex at `(60, 0, 30)` wraps silently into i16. The
/// player-relative emission keeps `|value - player_ref|` bounded by
/// the render radius (~11 tiles), and the /2 scale packs that into
/// `±5.5`, well inside the format range.
///
/// The DS 3D engine hardware clears and depth-tests internally; no
/// framebuffer clear is necessary. The caller is responsible for
/// calling [`gx::swap_buffers`] after this function returns to commit
/// the frame for display.
pub fn render_scene_ds(view: &dyn GameView, frame: u32) {
    let (px, py) = view.player_xy();

    // Player world position (tile center) — reference point for the
    // view matrix translation collapse and for the vertex emission
    // subtraction in GpuSink::emit_vertex.
    let player_x = Fixed16::from_int(px) + Fixed16::HALF;
    let player_z = Fixed16::from_int(py) + Fixed16::HALF;

    // Build the camera in player-relative space. Both target and eye
    // are expressed relative to the player — the rotation portion of
    // the view matrix comes out identical to the world-space version
    // (look_at's basis is translation-invariant), and the translation
    // portion collapses to near-zero.
    let target_rel = Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO);
    let eye_rel = Vec3::new(
        Fixed16::ZERO,         // same x as target
        CAMERA_HEIGHT,         // 10 units up
        CAMERA_TILT_OFFSET,    // 5 units south of target (camera looks north, -Z)
    );
    let up = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
    let view_rel = Mat4::look_at(eye_rel, target_rel, up);

    // Pre-multiply by scale(2) so the hardware matrix undoes the /2
    // applied per vertex in emit_vertex. Recomputed per frame (~64
    // Fixed16 multiplies) because the camera rotation is constant but
    // the code path is simpler this way than caching.
    let two = Fixed16::from_int(2);
    let scale2 = Mat4::scale(Vec3::new(two, two, two));
    let position_mat = view_rel * scale2;

    // DS top screen is 256×192, aspect 4:3 in Fixed16.
    let aspect = Fixed16::from_raw((((256i64) << 16) / 192) as i32);
    let projection_mat = Mat4::perspective(FOV, aspect, NEAR, FAR);

    // Convert to DS 1.19.12 column-major submission order and upload.
    let ds_projection = projection_mat.to_ds_matrix();
    let ds_position = position_mat.to_ds_matrix();
    unsafe {
        // MTX_MODE_POSITION (1), not POSITION_AND_VECTOR (2): Phase 2
        // keeps software Lambert + attenuation lighting. When hardware
        // lighting lands, switch to mode 2 so the vector stack tracks
        // the position stack for normal transformation.
        gx::mtx_mode(gx::MTX_MODE_PROJECTION);
        gx::mtx_load_4x4(&ds_projection);
        gx::mtx_mode(gx::MTX_MODE_POSITION);
        gx::mtx_load_4x4(&ds_position);

        // Re-program fog offset and depth shift from the runtime-tunable
        // cells. Only touches two registers (FOG_OFFSET + DISP3DCNT) —
        // the density table is static and was programmed once in init().
        gx::update_fog_params(fog_offset() as u16, fog_shift() as u16);
    }

    // Torch light at the player's waist. World-space — the lighting
    // math in `vertex_light_no_visibility` consumes world coords, as
    // does the centroid cull.
    let light_pos = Vec3::new(player_x, LIGHT_HEIGHT, player_z);
    let light_color = torch_light_color(frame);

    let mut sink = GpuSink::new(light_pos, light_color, player_x, player_z);

    // Profiling: measure time spent walking map geometry + emitting
    // through GpuSink. Exposed via `last_gen_ticks()` for the debug HUD.
    let gen_t0 = read_timer32();
    geometry::generate_map_geometry(view, &mut sink);
    let gen_t1 = read_timer32();
    unsafe {
        *LAST_GEN_TICKS.0.get() = gen_t1.wrapping_sub(gen_t0);
    }

    // Billboard pass: render entities and items as textured quads.
    render_billboards(view, light_pos, light_color, player_x, player_z);
}

// ---------------------------------------------------------------------------
// Billboard rendering
// ---------------------------------------------------------------------------

/// Billboard width in world units (~0.7 tile), matching the software
/// path's `BILLBOARD_WIDTH` in renderer3d::scene.
const BILLBOARD_HALF_W: Fixed16 = Fixed16::from_raw(0xB333 / 2); // ~0.35

/// Billboard height in world units (~0.9 tile), matching the software
/// path's `BILLBOARD_HEIGHT`.
const BILLBOARD_HEIGHT: Fixed16 = Fixed16::from_raw(0xE666); // ~0.9

/// Render visible entities and items as A3I5-textured billboard quads.
///
/// Each entity/item becomes a camera-facing quad (2 triangles) textured
/// with the corresponding glyph from the compile-time atlas. The vertex
/// COLOR carries the entity's `GameColor` tinted by Lambert + attenuation
/// (via `vertex_light_no_visibility`); the white texture modulates it
/// to produce the final color. A3I5 alpha=0 texels are rejected by the
/// hardware alpha test, giving clean glyph transparency.
///
/// The camera right vector is a compile-time constant `(+1, 0, 0)`
/// because the camera parameters (eye_rel, target_rel) are fixed. The
/// quad construction matches `renderer3d::scene::render_billboard`
/// exactly: `center ± cam_right × half_width`, with height along +Y.
fn render_billboards(
    view: &dyn GameView,
    light_pos: Vec3,
    light_color: [u16; 3],
    player_x: Fixed16,
    player_z: Fixed16,
) {
    // Enable the glyph atlas texture for the billboard pass.
    unsafe {
        core::ptr::write_volatile(gx::GX_TEXIMAGE_PARAM, gx::TEXIMAGE_GLYPH_ATLAS);
        core::ptr::write_volatile(gx::GX_PLTT_BASE, 0); // palette slot 0
    }

    let up_normal = Vec3::new(Fixed16::ZERO, Fixed16::ONE, Fixed16::ZERO);
    let half_h = Fixed16::from_raw(BILLBOARD_HEIGHT.to_raw() / 2);

    // Entities
    let entity_count = view.entity_count();
    let mut i = 0;
    while i < entity_count {
        if view.entity_alive(i) {
            let (ex, ey) = view.entity_xy(i);
            if view.tile_visibility(ex, ey) == TileVisibility::Visible {
                let (ch, gc) = view.render_entity(i);
                emit_billboard(
                    ex, ey, ch, gc, &up_normal, light_pos, light_color,
                    player_x, player_z, half_h,
                );
            }
        }
        i += 1;
    }

    // Items
    let item_count = view.item_count();
    let mut i = 0;
    while i < item_count {
        if view.item_alive(i) {
            let (ix, iy) = view.item_xy(i);
            if view.tile_visibility(ix, iy) == TileVisibility::Visible {
                let (ch, gc) = view.render_item(i);
                emit_billboard(
                    ix, iy, ch, gc, &up_normal, light_pos, light_color,
                    player_x, player_z, half_h,
                );
            }
        }
        i += 1;
    }

    // Disable texturing for any subsequent geometry (defensive reset).
    unsafe {
        core::ptr::write_volatile(gx::GX_TEXIMAGE_PARAM, 0);
    }
}

/// Emit one textured billboard quad (2 triangles) for an entity or item.
#[inline(never)] // keep out of the hot loop to reduce register pressure
#[allow(clippy::too_many_arguments)] // per-call values, no natural struct
fn emit_billboard(
    tile_x: i32,
    tile_z: i32,
    ch: char,
    gc: GameColor,
    up_normal: &Vec3,
    light_pos: Vec3,
    light_color: [u16; 3],
    player_x: Fixed16,
    player_z: Fixed16,
    half_h: Fixed16,
) {
    let (atlas_col, atlas_row) = char_to_atlas(ch);

    // TEXCOORD values in s.11.4 format (1 texel = 16 raw units).
    // Each glyph is 8×8 texels in the 32×32 atlas.
    let s_min = atlas_col as i16 * 128; // col * 8 texels * 16
    let s_max = s_min + 128;
    let t_min = atlas_row as i16 * 128;
    let t_max = t_min + 128;

    // World-space quad center at the tile center, y=0 (floor).
    let cx = Fixed16::from_int(tile_x) + Fixed16::HALF;
    let cz = Fixed16::from_int(tile_z) + Fixed16::HALF;

    // Camera right vector = (+1, 0, 0). The quad extends along ±X:
    //   left  = center - half_w  (negative X — screen left)
    //   right = center + half_w  (positive X — screen right)
    let lx = cx - BILLBOARD_HALF_W;
    let rx = cx + BILLBOARD_HALF_W;

    // 4 vertices: bl (bottom-left), tl (top-left), tr (top-right), br (bottom-right)
    // Y axis: 0 = floor, BILLBOARD_HEIGHT = top of glyph.
    let y_bot = Fixed16::ZERO;
    let y_top = BILLBOARD_HEIGHT;

    // Per-quad lighting: compute once at the quad center (midpoint height).
    let center_mid = Vec3::new(cx, half_h, cz);
    let fog = vertex_light_no_visibility(center_mid, *up_normal, light_pos);

    // Tint entity color by lighting, swizzle to DS format.
    let base_rgb = game_color_to_rgb555(gc);
    let tinted = apply_fog(base_rgb, fog, 0, light_color);
    let ds_color = gx::swizzle_gl_to_ds(tinted);

    // Player-relative half-scaled vertex conversion (reuse Phase 2 helper).
    let xl = GpuSink::world_to_s3_12_rel(lx, player_x);
    let xr = GpuSink::world_to_s3_12_rel(rx, player_x);
    let yb = GpuSink::world_to_s3_12_rel(y_bot, Fixed16::ZERO);
    let yt = GpuSink::world_to_s3_12_rel(y_top, Fixed16::ZERO);
    let z = GpuSink::world_to_s3_12_rel(cz, player_z);

    unsafe {
        gx::color(ds_color);
        gx::begin(gx::PRIM_TRIANGLES);

        // Triangle 1: bl - tl - tr
        gx::texcoord(s_min, t_max);
        gx::vtx_16(xl, yb, z);
        gx::texcoord(s_min, t_min);
        gx::vtx_16(xl, yt, z);
        gx::texcoord(s_max, t_min);
        gx::vtx_16(xr, yt, z);

        // Triangle 2: bl - tr - br
        gx::texcoord(s_min, t_max);
        gx::vtx_16(xl, yb, z);
        gx::texcoord(s_max, t_min);
        gx::vtx_16(xr, yt, z);
        gx::texcoord(s_max, t_max);
        gx::vtx_16(xr, yb, z);

        gx::end();
    }
}
