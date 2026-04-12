//! DS 3D engine (GX) hardware access layer.
//!
//! Clean-room implementation from GBATEK §DS Video / 3D Engine /
//! GXFIFO Commands. No libnds, BlocksDS, or nds-sys source was
//! consulted during development.
//!
//! ## The DS 3D engine in one paragraph
//!
//! The DS has a fixed-function, PS1-era 3D GPU: 2048 polygon /
//! 6144 vertex limit per frame, fixed-point vertex format (s.3.12
//! for positions), hardware matrix stack (projection, position,
//! position+vector, and texture modes), 4 hardware directional
//! lights, hardware fog via a 32-entry density table, 24-bit
//! Z-buffer, nearest-neighbor textures, and hardware perspective
//! divide. Output goes through BG0 on Engine A (top screen) when
//! DISPCNT bit 3 is set.
//!
//! ## How GX commands work
//!
//! Each 3D command has a dedicated memory-mapped port in the
//! `0x0400_0440..0x0400_05FC` range. Writing to a port pushes that
//! command into the GX FIFO; when enough parameters have been
//! written for the command, the hardware executes it. Commands that
//! take multiple 32-bit parameters (e.g. `MTX_LOAD_4x4` takes 16,
//! `VTX_16` takes 2) are fed by writing to the same port repeatedly.
//!
//! ## Phase 1 scope
//!
//! This file provides only what Phase 1 needs: init, matrix mode
//! switching + identity loads, BEGIN/END/COLOR/VTX_16, and
//! SWAP_BUFFERS. Lighting, textures, and hardware fog come in
//! later phases.

use core::ptr;

// ---------------------------------------------------------------------------
// 3D engine command registers (GBATEK §DS Video 3D Engine Commands)
// ---------------------------------------------------------------------------

// Matrix stack commands. Phase 1 used only MTX_MODE + MTX_IDENTITY
// (MVP ran in software, hardware matrices left as identity). Phase 2
// moves MVP onto the hardware matrix stack and adds MTX_LOAD_4x4 so
// projection and position matrices can be uploaded per frame.
pub const GX_MTX_MODE: *mut u32 = 0x0400_0440 as *mut u32;
pub const GX_MTX_IDENTITY: *mut u32 = 0x0400_0454 as *mut u32;
pub const GX_MTX_LOAD_4X4: *mut u32 = 0x0400_0458 as *mut u32;

// Vertex and draw commands
pub const GX_COLOR: *mut u32 = 0x0400_0480 as *mut u32;
pub const GX_TEXCOORD: *mut u32 = 0x0400_0488 as *mut u32;
pub const GX_VTX_16: *mut u32 = 0x0400_048C as *mut u32;
pub const GX_POLYGON_ATTR: *mut u32 = 0x0400_04A4 as *mut u32;
pub const GX_TEXIMAGE_PARAM: *mut u32 = 0x0400_04A8 as *mut u32;
pub const GX_PLTT_BASE: *mut u32 = 0x0400_04AC as *mut u32;
pub const GX_BEGIN_VTXS: *mut u32 = 0x0400_0500 as *mut u32;
pub const GX_END_VTXS: *mut u32 = 0x0400_0504 as *mut u32;
pub const GX_SWAP_BUFFERS: *mut u32 = 0x0400_0540 as *mut u32;
pub const GX_VIEWPORT: *mut u32 = 0x0400_0580 as *mut u32;

// ---------------------------------------------------------------------------
// 3D engine configuration registers (GBATEK §DS Video Registers)
// ---------------------------------------------------------------------------

/// 3D display control — fog, edge, anti-alias, toon/highlight enables
pub const DISP3DCNT: *mut u16 = 0x0400_0060 as *mut u16;
/// Alpha-test comparison value (bits 0-4: 0..31). Pixels with alpha
/// GREATER than this value are rendered; others are rejected.
pub const ALPHA_TEST_REF: *mut u16 = 0x0400_0340 as *mut u16;
/// Polygon clear color (includes alpha, polygon ID, fog enable)
pub const CLEAR_COLOR: *mut u32 = 0x0400_0350 as *mut u32;
/// Z-buffer clear depth (0..0x7FFF)
pub const CLEAR_DEPTH: *mut u16 = 0x0400_0354 as *mut u16;

// --- Fog registers (GBATEK §4000358h-4000360h) ---
/// Fog color + alpha, u32 layout: bits 0-14 RGB555 (B<<10|G<<5|R DS order),
/// bit 15 unused, bits 16-20 alpha 0..31, bits 21-31 unused.
pub const FOG_COLOR: *mut u32 = 0x0400_0358 as *mut u32;
/// Fog depth offset: unsigned 0..0x7FFF in the top 15 bits of the 24-bit
/// depth range. The first density boundary is at FOG_OFFSET + FOG_STEP.
pub const FOG_OFFSET: *mut u16 = 0x0400_035C as *mut u16;
/// Fog density table base: 32 bytes, written one u8 at a time (see
/// [`setup_fog`]). Each byte's bits 0-6 are density 0..0x7F
/// (0 = no fog, 0x7F = full fog); bit 7 is ignored.
pub const FOG_TABLE_BASE: *mut u8 = 0x0400_0360 as *mut u8;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Matrix mode values (GBATEK §MTX_MODE). Phase 1 loads identity into
// projection, position, and texture. Position+vector mode (value 2) is
// used by hardware lighting and comes back in Phase 2.
pub const MTX_MODE_PROJECTION: u32 = 0;
pub const MTX_MODE_POSITION: u32 = 1;
pub const MTX_MODE_TEXTURE: u32 = 3;

/// BEGIN_VTXS primitive type for individual triangles (GBATEK §BEGIN_VTXS).
/// Phase 1 emits everything as independent triangles. Quad and strip
/// variants can be reintroduced later if they measurably help.
pub const PRIM_TRIANGLES: u32 = 0;

// POLYGON_ATTR bit layout (GBATEK §POLYGON_ATTR)
//   Bits 0-3:   Enable flags for lights 0..3
//   Bits 4-5:   Polygon mode (0 = modulation)
//   Bit 6:      Render back-face
//   Bit 7:      Render front-face
//   Bit 11:     Depth test mode (0 = less)
//   Bit 15:     Per-polygon fog enable
//   Bits 16-20: Alpha (0..31, 0 = wireframe, 31 = opaque)
//   Bits 24-29: Polygon ID (0..63)
const POLY_ATTR_RENDER_BACK: u32 = 1 << 6;
const POLY_ATTR_RENDER_FRONT: u32 = 1 << 7;
/// Enable hardware fog for polygons drawn with this POLYGON_ATTR value.
/// Set alongside `DISP3DCNT_FOG_MASTER` for fog to actually apply.
const POLY_ATTR_FOG_ENABLE: u32 = 1 << 15;
const POLY_ATTR_ALPHA_SHIFT: u32 = 16;

// DISP3DCNT fog-related bits (GBATEK §DISP3DCNT)
//   Bit 6     : fog color/alpha mode (0 = RGB and alpha, 1 = alpha only)
//   Bit 7     : fog master enable
//   Bits 8-11 : fog depth shift (FOG_STEP = 0x400 >> FOG_SHIFT, 0..10 usable)
/// Master fog enable bit in `DISP3DCNT`. Required alongside
/// `POLY_ATTR_FOG_ENABLE` for fog to actually apply.
const DISP3DCNT_FOG_MASTER: u16 = 1 << 7;

/// Texture mapping master enable in `DISP3DCNT`. Without this bit,
/// the 3D engine ignores all TEXIMAGE_PARAM settings and renders
/// untextured (vertex color only). Required for A3I5 glyph billboards.
const DISP3DCNT_TEXTURE_MAP: u16 = 1 << 0;

/// Alpha-test enable bit in `DISP3DCNT`. When set, texels with alpha
/// ≤ `ALPHA_TEST_REF` are rejected. Used by the billboard pass to
/// discard transparent A3I5 texels (alpha=0 background pixels).
const DISP3DCNT_ALPHA_TEST: u16 = 1 << 2;

/// Packed `DISP3DCNT` bits that must always be set: texture mapping +
/// alpha test + fog master. `update_fog_params` combines these with the
/// runtime fog shift each frame. Keeps all three features from
/// clobbering each other.
const DISP3DCNT_ALWAYS: u16 =
    DISP3DCNT_TEXTURE_MAP | DISP3DCNT_ALPHA_TEST | DISP3DCNT_FOG_MASTER;

/// `CLEAR_COLOR` bit 15: fog enable for the rear-plane pixels that
/// haven't been covered by any polygon. Without this bit set, the
/// horizon behind distant geometry is flat clear-color instead of
/// fading to fog color — which reads as a dark void rather than a
/// receding horizon.
const CLEAR_COLOR_FOG_ENABLE: u32 = 1 << 15;

/// Default fog depth shift (DISP3DCNT bits 8-11). Produces
/// `FOG_STEP = 0x400 >> 0 = 0x400`, which means the 32-entry fog table
/// spans 32 × 0x400 = 0x8000 of the 15-bit-compared depth range —
/// covering the entire depth buffer from offset onward. Tuned on real
/// DS hardware (SH 0 gave the smoothest gradient given the z-buffer's
/// nonlinear depth distribution). Tunable at runtime via Select+dpad.
const DEFAULT_FOG_SHIFT: u16 = 0;

/// Default fog offset: where in the depth buffer the fog curve starts.
/// Tuned on real DS hardware to 0x3000, which begins the fog ramp at
/// roughly 1.5 tiles from the camera — near geometry stays lit, distant
/// geometry fades to black. Tunable at runtime via Select+dpad.
const DEFAULT_FOG_OFFSET: u16 = 0x3000;

/// Default fog color: black + alpha 31. Matches the clear color so the
/// rear plane fades smoothly into the fog. Low-15 bits are RGB555 (DS
/// packing: `B<<10 | G<<5 | R`), bits 16-20 are alpha.
const DEFAULT_FOG_COLOR: u32 = 0x001F_0000;

/// Default fog density table: linear `i * 4` ramp across 32 entries
/// (max value 124 = 0x7C, just shy of the 0x7F ceiling). Approximates
/// the software path's visibility curve — starts at 0 (no fog near
/// camera), ramps to near-full density at the far end. Tunable on
/// real hardware via the d-pad HUD controls.
const DEFAULT_FOG_TABLE: [u8; 32] = {
    let mut table = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        table[i] = (i * 4) as u8;
        i += 1;
    }
    table
};


// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the 3D engine to a known-good default state.
///
/// Call this once at boot, after `init_display()` has set the POWCNT1
/// 3D engine bits. Leaves the engine ready to accept geometry:
///
/// - Hardware fog enabled (master + per-poly + rear-plane) with a
///   default linear density ramp, tunable at runtime via the d-pad
///   HUD controls
/// - Viewport covering the full 256×192 screen
/// - Clear color = black opaque + fog-enabled for the rear plane
/// - Polygon attr = render both sides, opaque, no lights, fog on
/// - All matrix stacks loaded with identity (render_scene_ds uploads
///   projection and position matrices per frame)
pub fn init() {
    unsafe {
        // Enable hardware fog + alpha test. The fog master bit (7) is
        // required for the per-polygon fog enable in POLYGON_ATTR to
        // have effect. Alpha test (bit 2) rejects A3I5 texels with
        // alpha=0, used by the billboard pass for glyph transparency.
        // Bits 8-11 are the fog depth shift.
        ptr::write_volatile(
            DISP3DCNT,
            DISP3DCNT_ALWAYS | (DEFAULT_FOG_SHIFT << 8),
        );

        // Alpha test threshold = 0: reject texels with alpha == 0 only.
        // A3I5 foreground texels have alpha=7 (> 0, rendered), background
        // texels have alpha=0 (== 0, not > 0, rejected).
        ptr::write_volatile(ALPHA_TEST_REF, 0);

        // Viewport is packed into a single 32-bit word:
        //   byte 0 = x1 (= 0), byte 1 = y1 (= 0),
        //   byte 2 = x2 (= 255), byte 3 = y2 (= 191)
        // Full top-screen coverage is (0, 0, 255, 191), and since x1
        // and y1 are both zero we only need the high bytes.
        let viewport = (255u32 << 16) | (191u32 << 24);
        ptr::write_volatile(GX_VIEWPORT, viewport);

        // Clear color register layout (per GBATEK §CLEAR_COLOR):
        //   bits 0-14  : RGB555 color (B<<10 | G<<5 | R — DS convention)
        //   bit 15     : fog enable for cleared pixels
        //   bits 16-20 : alpha (0..31)
        //   bits 24-29 : polygon ID
        // Black + alpha 31 + fog enable. The fog bit is critical: without
        // it the rear plane (visible behind all geometry) stays at the
        // flat clear color instead of fading to fog color, producing a
        // jarring dark void at the horizon.
        ptr::write_volatile(CLEAR_COLOR, 0x001F_0000 | CLEAR_COLOR_FOG_ENABLE);

        // Clear depth = max (farthest).
        ptr::write_volatile(CLEAR_DEPTH, 0x7FFF);

        // Polygon attributes: render both sides (no culling — avoids a
        // winding-order debug cycle since the hardware y-axis convention
        // may differ from our software CCW front-face; defer to Phase 3
        // as a fill-rate optimization lever), opaque alpha 31, polygon
        // ID 0, no lights, fog enabled per polygon.
        let poly_attr = POLY_ATTR_RENDER_FRONT
            | POLY_ATTR_RENDER_BACK
            | POLY_ATTR_FOG_ENABLE
            | (31u32 << POLY_ATTR_ALPHA_SHIFT);
        ptr::write_volatile(GX_POLYGON_ATTR, poly_attr);

        // Load identity into all matrix stacks. render_scene_ds uploads
        // the projection and position matrices per frame via
        // mtx_load_4x4; identity is just the boot state before the
        // first frame.
        mtx_mode(MTX_MODE_PROJECTION);
        mtx_identity();
        mtx_mode(MTX_MODE_POSITION);
        mtx_identity();
        mtx_mode(MTX_MODE_TEXTURE);
        mtx_identity();

        // Program the fog registers with defaults. The density table
        // and offset/shift are runtime-tunable via d-pad HUD controls
        // (avoids a rebuild/flash cycle per tuning iteration on real
        // hardware).
        setup_fog(DEFAULT_FOG_COLOR, DEFAULT_FOG_OFFSET, &DEFAULT_FOG_TABLE);
    }
}

// ---------------------------------------------------------------------------
// Texture VRAM setup
// ---------------------------------------------------------------------------

/// VRAM bank control registers.
const VRAMCNT_B: *mut u8 = 0x0400_0241 as *mut u8;
const VRAMCNT_F: *mut u8 = 0x0400_0245 as *mut u8;

/// Bank B LCDC-mode address for CPU writes.
const VRAM_B_LCDC: *mut u8 = 0x0682_0000 as *mut u8;

/// Bank F LCDC-mode address for CPU writes. When mapped as texture
/// palette (MST=4), the 3D engine sees this data at slot 0.
const VRAM_F_LCDC: *mut u16 = 0x0689_0000 as *mut u16;

/// Upload the glyph texture atlas and palette to VRAM.
///
/// Call once at boot, after [`init`]. Maps VRAM Bank B as Texture Slot 0
/// (for the 32×32 A3I5 atlas) and Bank F as Texture Palette Slot 0 (for
/// the 2-entry white palette). Both banks are temporarily put in LCDC
/// mode for the CPU write, then remapped for 3D engine access.
///
/// # Safety
/// Must be called before any `render_billboards` call. Bank B must not
/// already be mapped for another purpose (it should be unmapped at boot).
pub fn init_textures(atlas: &[u8; 1024], palette: &[u16; 2]) {
    unsafe {
        // --- Bank B → Texture Slot 0 (atlas data) ---
        //
        // Step 1: LCDC mode (MST=0) — CPU can write to 0x06820000.
        ptr::write_volatile(VRAMCNT_B, 0x80); // enable + MST=0
        // Step 2: Copy atlas. Bank B is 128K but we only use 1024 bytes.
        // Must use u16 writes (u8 writes to non-LCDC VRAM are dropped,
        // and we're in LCDC mode here so u8 is fine, but u16 is safer
        // and matches the Phase 1.5 gotcha pattern).
        let dst = VRAM_B_LCDC as *mut u16;
        let mut i = 0;
        while i < 1024 {
            let word = (atlas[i] as u16) | ((atlas[i + 1] as u16) << 8);
            ptr::write_volatile(dst.add(i / 2), word);
            i += 2;
        }
        // Step 3: Remap as Texture Slot 0 (MST=3, OFS=0). The 3D engine
        // reads texture data from slot 0 at an internal address; the CPU
        // can no longer write to this bank.
        ptr::write_volatile(VRAMCNT_B, 0x83); // enable + MST=3 + OFS=0

        // --- Bank F → Texture Palette Slot 0 ---
        //
        // Step 1: LCDC mode.
        ptr::write_volatile(VRAMCNT_F, 0x80); // enable + MST=0
        // Step 2: Write the 2-entry palette (4 bytes = 2 u16 words).
        let pal_dst = VRAM_F_LCDC;
        ptr::write_volatile(pal_dst, palette[0]);
        ptr::write_volatile(pal_dst.add(1), palette[1]);
        // Step 3: Remap as Texture Palette Slot 0 (MST=3, OFS=0).
        // VRAMCNT_F bits: 7=enable, 0-2=MST, 3-4=OFS.
        // MST=3 → bits 0-2 = 0b011 = Texture Palette. OFS=0 → Slot 0.
        // (MST=4 would be Engine A BG ext palette — wrong destination!)
        // GBATEK: "F,G 16K MST=3 OFS=0..3 → Texture Palette Slot"
        ptr::write_volatile(VRAMCNT_F, 0x83);
    }
}

/// Program the hardware fog registers.
///
/// `color` is a 32-bit FOG_COLOR value in DS native packing: bits 0-14
/// are RGB555 (B<<10|G<<5|R), bit 15 unused, bits 16-20 are alpha 0..31.
///
/// `offset` is the 15-bit starting depth (0..0x7FFF) below which all
/// pixels use `table[0]`.
///
/// `table` is a 32-byte density ramp; each byte's low 7 bits are the
/// density 0..0x7F (0 = no fog, 0x7F = full fog). Bit 7 of each byte is
/// unused by the hardware.
///
/// Called from [`init`] with default values, and may be called again at
/// runtime when the d-pad HUD controls adjust the offset or shift.
/// The fog master enable and per-polygon enable bits are set separately
/// in `init()` via `DISP3DCNT` and `POLYGON_ATTR`.
pub fn setup_fog(color: u32, offset: u16, table: &[u8; 32]) {
    unsafe {
        ptr::write_volatile(FOG_COLOR, color);
        ptr::write_volatile(FOG_OFFSET, offset);

        // FOG_TABLE_BASE is a *mut u8; each write lands at the
        // corresponding FogDensity entry in 0x0400_0360..0x0400_037F.
        let mut i = 0;
        while i < 32 {
            ptr::write_volatile(FOG_TABLE_BASE.add(i), table[i]);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix stack helpers
// ---------------------------------------------------------------------------

/// Switch the current matrix mode. Must precede any MTX_* command.
#[inline]
pub unsafe fn mtx_mode(mode: u32) {
    ptr::write_volatile(GX_MTX_MODE, mode);
}

/// Load identity into the current matrix.
#[inline]
pub unsafe fn mtx_identity() {
    ptr::write_volatile(GX_MTX_IDENTITY, 0);
}

/// Load a 4x4 matrix into the current matrix stack.
///
/// The matrix must already be in DS 1.19.12 submission order — row
/// pairs written sequentially, with translation in `m[12..14]`. Use
/// [`roguelike_renderer3d::math::Mat4::to_ds_matrix`] to convert
/// from our column-vector row-major `Mat4` representation.
///
/// Per GBATEK §MTX_LOAD_4x4 (Cmd 16h at 0x4000458h), the command
/// takes 16 consecutive u32 parameters. Each element is a signed
/// 1.19.12 fixed-point value.
///
/// # Safety
/// Caller must ensure the 3D engine is powered on and the correct
/// matrix mode has been selected via `mtx_mode()` before calling.
#[inline]
pub unsafe fn mtx_load_4x4(m: &[u32; 16]) {
    let mut i = 0;
    while i < 16 {
        ptr::write_volatile(GX_MTX_LOAD_4X4, m[i]);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Vertex submission
// ---------------------------------------------------------------------------

/// Begin a primitive group. `primitive` is one of the `PRIM_*` constants.
///
/// Must be paired with `end()`. Between the two, call `color()` and
/// `vtx_16()` for each vertex. Primitive type cannot change within a
/// group (use `end()` + `begin()` to switch types).
#[inline]
pub unsafe fn begin(primitive: u32) {
    ptr::write_volatile(GX_BEGIN_VTXS, primitive);
}

/// End the current primitive group. Per GBATEK §END_VTXS the parameter
/// is ignored by hardware but a write must occur to advance the FIFO.
#[inline]
pub unsafe fn end() {
    ptr::write_volatile(GX_END_VTXS, 0);
}

/// Set the current vertex color.
///
/// `ds_rgb555` is packed in **DS hardware convention**: B<<10 | G<<5 | R
/// (each channel 5 bits, bit 15 unused by the 3D engine). This is the
/// opposite of the renderer3d / OpenGL convention used by the software
/// rasterizer. Use [`swizzle_gl_to_ds`] to convert if you have a
/// renderer3d-format color.
#[inline]
pub unsafe fn color(ds_rgb555: u16) {
    ptr::write_volatile(GX_COLOR, ds_rgb555 as u32);
}

/// Emit one vertex in s.3.12 fixed-point format.
///
/// Per GBATEK §VTX_16, the command takes two 32-bit parameters:
///   Param 1: bits 0-15 = X, bits 16-31 = Y
///   Param 2: bits 0-15 = Z, bits 16-31 = unused
///
/// Each coordinate is a signed 16-bit fixed-point value with 12
/// fractional bits — range approximately [-8, +8) world units.
/// Values that exceed this range wrap silently; the caller is
/// responsible for keeping pre-transform or clip-space vertices in
/// range (typically by scaling via the matrix stack or a software
/// MVP transform).
#[inline]
pub unsafe fn vtx_16(x: i16, y: i16, z: i16) {
    let xy = (x as u16 as u32) | ((y as u16 as u32) << 16);
    let z_pad = z as u16 as u32;
    ptr::write_volatile(GX_VTX_16, xy);
    ptr::write_volatile(GX_VTX_16, z_pad);
}

/// Update the hardware fog offset and depth shift registers.
///
/// Called once per frame from `render_scene_ds` with the runtime-tunable
/// fog parameters. Only writes `FOG_OFFSET` and `DISP3DCNT` — the
/// density table and per-polygon fog enable bits are unchanged.
#[inline]
pub unsafe fn update_fog_params(offset: u16, shift: u16) {
    ptr::write_volatile(FOG_OFFSET, offset);
    ptr::write_volatile(DISP3DCNT, DISP3DCNT_ALWAYS | (shift << 8));
}

/// Set texture coordinates for the next vertex.
///
/// Per GBATEK §TEXCOORD: s.11.4 fixed-point format in each half-word.
/// One texel = 16 raw units. Must be called BEFORE the `VTX_16` command
/// for each vertex of a textured polygon.
#[inline]
pub unsafe fn texcoord(s: i16, t: i16) {
    let packed = (s as u16 as u32) | ((t as u16 as u32) << 16);
    ptr::write_volatile(GX_TEXCOORD, packed);
}

/// Pre-packed `TEXIMAGE_PARAM` value for the 32×32 A3I5 glyph atlas
/// at Texture Slot 0 offset 0. Per GBATEK §TEXIMAGE_PARAM:
///   bits 0-15:  VRAM offset / 8 = 0
///   bits 20-22: S-Size W=2 (Width = 8 << 2 = 32)
///   bits 23-25: T-Size H=2 (Height = 8 << 2 = 32)
///   bits 26-28: Format = 1 (A3I5: 3-bit alpha + 5-bit palette index)
///   bit 29:     Color 0 transparency = 0 (alpha handles it)
pub const TEXIMAGE_GLYPH_ATLAS: u32 = (2 << 20) | (2 << 23) | (1 << 26);

/// Commit the current frame and swap render buffers.
///
/// Per GBATEK §SWAP_BUFFERS, the parameter bits control:
///   bit 0 : manual-sort translucent polygons
///   bit 1 : Y-sorted / W-buffering mode
/// For Phase 1 we use the defaults (parameter = 0).
#[inline]
pub unsafe fn swap_buffers() {
    ptr::write_volatile(GX_SWAP_BUFFERS, 0);
}

// ---------------------------------------------------------------------------
// Color format helpers
// ---------------------------------------------------------------------------

/// Convert a renderer3d-format RGB555 color (`R<<10 | G<<5 | B`) to
/// the DS hardware format (`B<<10 | G<<5 | R`).
///
/// This is the same swizzle used by [`crate::swizzle_rgb555`] in
/// `main.rs`, but without the bit-15 "opaque" flag — the 3D engine
/// carries opacity via `POLYGON_ATTR` alpha, not a per-pixel bit.
#[inline]
pub fn swizzle_gl_to_ds(c: u16) -> u16 {
    let r = (c >> 10) & 0x1F;
    let g = (c >> 5) & 0x1F;
    let b = c & 0x1F;
    (b << 10) | (g << 5) | r
}
