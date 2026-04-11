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

// Matrix stack commands. Phase 1 only needs MTX_MODE + MTX_IDENTITY
// (hardware matrices are left as identity; MVP runs in software). Push,
// pop, load, and multiply commands return when Phase 2+ moves lighting
// and matrix work onto the hardware.
pub const GX_MTX_MODE: *mut u32 = 0x0400_0440 as *mut u32;
pub const GX_MTX_IDENTITY: *mut u32 = 0x0400_0454 as *mut u32;

// Vertex and draw commands
pub const GX_COLOR: *mut u32 = 0x0400_0480 as *mut u32;
pub const GX_VTX_16: *mut u32 = 0x0400_048C as *mut u32;
pub const GX_POLYGON_ATTR: *mut u32 = 0x0400_04A4 as *mut u32;
pub const GX_BEGIN_VTXS: *mut u32 = 0x0400_0500 as *mut u32;
pub const GX_END_VTXS: *mut u32 = 0x0400_0504 as *mut u32;
pub const GX_SWAP_BUFFERS: *mut u32 = 0x0400_0540 as *mut u32;
pub const GX_VIEWPORT: *mut u32 = 0x0400_0580 as *mut u32;

// ---------------------------------------------------------------------------
// 3D engine configuration registers (GBATEK §DS Video Registers)
// ---------------------------------------------------------------------------

/// 3D display control — fog, edge, anti-alias, toon/highlight enables
pub const DISP3DCNT: *mut u16 = 0x0400_0060 as *mut u16;
/// Polygon clear color (includes alpha, polygon ID, fog enable)
pub const CLEAR_COLOR: *mut u32 = 0x0400_0350 as *mut u32;
/// Z-buffer clear depth (0..0x7FFF)
pub const CLEAR_DEPTH: *mut u16 = 0x0400_0354 as *mut u16;

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
//   Bits 16-20: Alpha (0..31, 0 = wireframe, 31 = opaque)
//   Bits 24-29: Polygon ID (0..63)
const POLY_ATTR_RENDER_BACK: u32 = 1 << 6;
const POLY_ATTR_RENDER_FRONT: u32 = 1 << 7;
const POLY_ATTR_ALPHA_SHIFT: u32 = 16;

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the 3D engine to a known-good default state.
///
/// Call this once at boot, after `init_display()` has set the POWCNT1
/// 3D engine bits. Leaves the engine ready to accept geometry:
///
/// - No fog, edge marking, or anti-aliasing (Phase 1)
/// - Viewport covering the full 256×192 screen
/// - Clear color = black opaque, depth = farthest
/// - Polygon attr = render both sides, opaque, no lights, no fog
/// - All matrix stacks loaded with identity (Phase 1 performs the
///   MVP transform in software and emits clip-space vertices)
pub fn init() {
    unsafe {
        // Disable fog, edge marking, anti-aliasing, toon shading.
        // Phase 2 will reconfigure this with fog enable + table.
        ptr::write_volatile(DISP3DCNT, 0);

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
        // Black + alpha 31:
        ptr::write_volatile(CLEAR_COLOR, 0x001F_0000);

        // Clear depth = max (farthest).
        ptr::write_volatile(CLEAR_DEPTH, 0x7FFF);

        // Polygon attributes: render both sides (no culling for now,
        // we'll enable culling once we're confident about winding
        // order with the hardware's y-axis convention), opaque alpha 31,
        // polygon ID 0, no lights, no fog enable.
        let poly_attr = POLY_ATTR_RENDER_FRONT
            | POLY_ATTR_RENDER_BACK
            | (31u32 << POLY_ATTR_ALPHA_SHIFT);
        ptr::write_volatile(GX_POLYGON_ATTR, poly_attr);

        // Load identity into all matrix stacks. Phase 1 does MVP in
        // software; hardware is told to just rasterize what we give it.
        mtx_mode(MTX_MODE_PROJECTION);
        mtx_identity();
        mtx_mode(MTX_MODE_POSITION);
        mtx_identity();
        mtx_mode(MTX_MODE_TEXTURE);
        mtx_identity();
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
