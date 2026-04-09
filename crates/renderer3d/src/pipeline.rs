use crate::math::{Fixed16, Vec4};

/// Screen-space vertex after projection, viewport transform, and integer snap.
/// This is the handoff point from the Fixed16 math pipeline to the i32 rasterizer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenVertex {
    pub x: i32,
    pub y: i32,
    pub z: i16,
}

impl ScreenVertex {
    #[inline]
    pub const fn new(x: i32, y: i32, z: i16) -> Self {
        Self { x, y, z }
    }
}

/// Project a clip-space Vec4 to screen-space coordinates.
///
/// Performs perspective divide, viewport transform (Y-flipped for screen-down),
/// and vertex snap to integer pixel coordinates.
///
/// `clip`: post-MVP vertex in clip space (before perspective divide).
/// `width`, `height`: framebuffer dimensions in pixels.
pub fn project_vertex(clip: Vec4, width: i32, height: i32) -> ScreenVertex {
    let ndc = clip.perspective_divide();

    // Viewport transform: NDC [-1,1] → screen pixels
    // half(v) avoids Fixed16 division — just shift right by 1
    let half_ndc_x = Fixed16::from_raw((ndc.x + Fixed16::ONE).to_raw() >> 1);
    let half_ndc_y = Fixed16::from_raw((Fixed16::ONE - ndc.y).to_raw() >> 1);

    let pixel_x = (half_ndc_x * Fixed16::from_int(width)).to_int();
    let pixel_y = (half_ndc_y * Fixed16::from_int(height)).to_int();

    // Depth: NDC z [-1,1] → i16 [-32767, 32767]
    let z_screen = ((ndc.z.to_raw() as i64 * i16::MAX as i64) >> 16) as i16;

    ScreenVertex {
        x: pixel_x,
        y: pixel_y,
        z: z_screen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_projects_to_center() {
        // NDC (0, 0, 0) with w=1 → screen center
        let clip = Vec4::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE);
        let sv = project_vertex(clip, 320, 240);
        assert_eq!(sv.x, 160);
        assert_eq!(sv.y, 120);
    }

    #[test]
    fn top_left_corner() {
        // NDC (-1, 1, 0) → screen (0, 0) (top-left, Y flipped)
        let clip = Vec4::new(Fixed16::NEG_ONE, Fixed16::ONE, Fixed16::ZERO, Fixed16::ONE);
        let sv = project_vertex(clip, 320, 240);
        assert_eq!(sv.x, 0);
        assert_eq!(sv.y, 0);
    }

    #[test]
    fn bottom_right_corner() {
        // NDC (1, -1, 0) → screen (320, 240) — one past last pixel (clipped by rasterizer)
        let clip = Vec4::new(Fixed16::ONE, Fixed16::NEG_ONE, Fixed16::ZERO, Fixed16::ONE);
        let sv = project_vertex(clip, 320, 240);
        assert_eq!(sv.x, 320);
        assert_eq!(sv.y, 240);
    }

    #[test]
    fn depth_near_maps_to_min() {
        // NDC z = -1.0 → z_screen ≈ -32767
        let clip = Vec4::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::NEG_ONE, Fixed16::ONE);
        let sv = project_vertex(clip, 320, 240);
        assert!(
            sv.z <= -32700,
            "near depth = {}, expected near -32767",
            sv.z
        );
    }

    #[test]
    fn depth_far_maps_to_max() {
        // NDC z = +1.0 → z_screen ≈ +32767
        let clip = Vec4::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE, Fixed16::ONE);
        let sv = project_vertex(clip, 320, 240);
        assert!(sv.z >= 32700, "far depth = {}, expected near 32767", sv.z);
    }

    #[test]
    fn depth_ordering_preserved() {
        let near = Vec4::new(
            Fixed16::ZERO,
            Fixed16::ZERO,
            Fixed16::from_f32(-0.5),
            Fixed16::ONE,
        );
        let far = Vec4::new(
            Fixed16::ZERO,
            Fixed16::ZERO,
            Fixed16::from_f32(0.5),
            Fixed16::ONE,
        );
        let sv_near = project_vertex(near, 320, 240);
        let sv_far = project_vertex(far, 320, 240);
        assert!(
            sv_near.z < sv_far.z,
            "near z={} should be < far z={}",
            sv_near.z,
            sv_far.z
        );
    }
}
