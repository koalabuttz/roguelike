//! vita2d FFI bindings and safe Rust wrappers.
//!
//! vita2d (https://github.com/xerpi/libvita2d) is a GPU-accelerated 2D
//! library bundled with vitasdk. It manages SceGxm initialization,
//! double-buffering, and a temporary memory pool for per-frame draw calls.
//!
//! # Color format
//! vita2d uses RGBA8: `color = r | (g << 8) | (b << 16) | (a << 24)`.
//! Use the [`rgba`] helper to construct colors.
//!
//! # Usage (Phase 1)
//! ```no_run
//! let _vita2d = Vita2d::init();          // Initialize — must stay alive
//! loop {
//!     vita2d_start_drawing();
//!     vita2d_clear_screen();
//!     // ... draw calls ...
//!     vita2d_end_drawing();
//!     vita2d_swap_buffers();
//! }
//! ```

use core::ffi::c_void;

// vita2d is part of vitasdk. Link against the static library in the sysroot.
// Transitive stub dependencies (SceGxm, SceDisplay, SceSysmem, etc.) are
// satisfied by vitasdk-sys features in Cargo.toml.
#[link(name = "vita2d")]
extern "C" {
    // --- Lifecycle ---
    fn vita2d_init() -> i32;
    fn vita2d_fini() -> i32;
    fn vita2d_wait_rendering_done();

    // --- Per-frame draw cycle ---
    fn vita2d_start_drawing();
    fn vita2d_end_drawing();
    fn vita2d_swap_buffers();
    fn vita2d_clear_screen();

    // --- Clear color ---
    fn vita2d_set_clear_color(color: u32);

    // --- Primitive drawing ---
    /// Draw a filled rectangle. vita2d_draw_rectangle renders as a filled
    /// triangle strip (confirmed from source — not an outline).
    fn vita2d_draw_rectangle(x: f32, y: f32, w: f32, h: f32, color: u32);

    // --- Texture management ---
    /// Create an empty RGBA8888 texture. Returns null on failure.
    fn vita2d_create_empty_texture(w: u32, h: u32) -> *mut c_void;
    fn vita2d_free_texture(texture: *mut c_void);
    /// Pointer to raw pixel data (RGBA8, row-major).
    fn vita2d_texture_get_datap(texture: *const c_void) -> *mut u32;
    /// Row stride in bytes.
    fn vita2d_texture_get_stride(texture: *const c_void) -> u32;

    // --- Texture drawing (Phase 4: glyph atlas blits) ---
    /// Blit a sub-rectangle of a texture at (x, y), tinted by `color`.
    /// Pass rgba(255,255,255,255) for no tinting.
    fn vita2d_draw_texture_tint_part(
        texture: *const c_void,
        x: f32,
        y: f32,
        tex_x: f32,
        tex_y: f32,
        tex_w: f32,
        tex_h: f32,
        color: u32,
    );
}

// ── Color helper ─────────────────────────────────────────────────────────────

/// Pack RGBA components into a vita2d color word.
/// Format: `r | (g << 8) | (b << 16) | (a << 24)`.
#[inline(always)]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | ((a as u32) << 24)
}

pub const BLACK: u32 = rgba(0, 0, 0, 255);
pub const WHITE: u32 = rgba(255, 255, 255, 255);

// ── RAII guard ───────────────────────────────────────────────────────────────

/// RAII guard for vita2d lifecycle.
/// Call `Vita2d::init()` once at program start; keep the value alive.
/// vita2d_fini() is called on drop.
pub struct Vita2d {
    _private: (),
}

impl Vita2d {
    /// Initialize vita2d. Panics if vita2d_init() returns a non-zero error.
    pub fn init() -> Self {
        let rc = unsafe { vita2d_init() };
        assert_eq!(rc, 0, "vita2d_init() failed with code {rc}");
        Self { _private: () }
    }

    /// Begin a frame. Must be paired with [`end_frame`].
    pub fn start_frame(&self) {
        unsafe { vita2d_start_drawing() };
    }

    /// Finish the frame and submit it to the GPU.
    pub fn end_frame(&self) {
        unsafe {
            vita2d_end_drawing();
            vita2d_swap_buffers();
        }
    }

    /// Clear the back buffer to the current clear color.
    pub fn clear(&self) {
        unsafe { vita2d_clear_screen() };
    }

    /// Set the color used by [`clear`].
    pub fn set_clear_color(&self, color: u32) {
        unsafe { vita2d_set_clear_color(color) };
    }

    /// Draw a filled rectangle.
    pub fn draw_rect(&self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        unsafe { vita2d_draw_rectangle(x, y, w, h, color) };
    }

    /// Wait for all GPU rendering to complete (e.g. before reading back pixels).
    pub fn wait_done(&self) {
        unsafe { vita2d_wait_rendering_done() };
    }
}

impl Drop for Vita2d {
    fn drop(&mut self) {
        unsafe { vita2d_fini() };
    }
}

// ── Texture ───────────────────────────────────────────────────────────────────

/// RAII wrapper for a vita2d texture.
pub struct Texture {
    ptr: *mut c_void,
    width: u32,
    height: u32,
}

impl Texture {
    /// Create an empty RGBA8888 texture of the given dimensions.
    /// Returns `None` if allocation fails.
    pub fn new_empty(width: u32, height: u32) -> Option<Self> {
        let ptr = unsafe { vita2d_create_empty_texture(width, height) };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr, width, height })
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row stride in bytes (may be larger than `width * 4` due to alignment).
    pub fn stride_bytes(&self) -> u32 {
        unsafe { vita2d_texture_get_stride(self.ptr) }
    }

    /// Stride in u32 pixels.
    pub fn stride_pixels(&self) -> u32 {
        self.stride_bytes() / 4
    }

    /// Mutable slice view of the texture pixel data.
    /// Length is `stride_pixels * height` u32 values.
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        let datap = unsafe { vita2d_texture_get_datap(self.ptr) };
        let len = (self.stride_pixels() * self.height) as usize;
        unsafe { core::slice::from_raw_parts_mut(datap, len) }
    }

    /// Blit a glyph sub-rectangle from this texture at screen position (x, y),
    /// tinted by `color`. Use `WHITE` for no tint.
    pub fn draw_part(
        &self,
        screen_x: f32,
        screen_y: f32,
        tex_x: f32,
        tex_y: f32,
        tex_w: f32,
        tex_h: f32,
        color: u32,
    ) {
        unsafe {
            vita2d_draw_texture_tint_part(
                self.ptr, screen_x, screen_y, tex_x, tex_y, tex_w, tex_h, color,
            )
        };
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { vita2d_free_texture(self.ptr) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_black() {
        assert_eq!(rgba(0, 0, 0, 255), 0xFF000000);
    }

    #[test]
    fn rgba_white() {
        assert_eq!(rgba(255, 255, 255, 255), 0xFFFFFFFF);
    }

    #[test]
    fn rgba_red() {
        // Red: r=255, g=0, b=0, a=255 → 0xFF0000FF
        assert_eq!(rgba(255, 0, 0, 255), 0xFF0000FF);
    }

    #[test]
    fn rgba_green() {
        // Green: r=0, g=255, b=0, a=255 → 0xFF00FF00
        assert_eq!(rgba(0, 255, 0, 255), 0xFF00FF00);
    }

    #[test]
    fn rgba_blue() {
        // Blue: r=0, g=0, b=255, a=255 → 0xFFFF0000
        assert_eq!(rgba(0, 0, 255, 255), 0xFFFF0000);
    }
}
