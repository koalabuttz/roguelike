use super::fixed::Fixed16;
use super::trig;
use super::vec3::Vec3;
use super::vec4::Vec4;

/// 4x4 transformation matrix, row-major layout.
///
/// `m[row][col]` — each row is contiguous in memory, so the dot product
/// of a row with a Vec4 (the hot path for matrix-vector multiply) reads
/// a single contiguous slice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mat4 {
    pub m: [[Fixed16; 4]; 4],
}

const _: () = assert!(core::mem::size_of::<Mat4>() == 64);

impl Mat4 {
    pub const fn zero() -> Self {
        Self {
            m: [[Fixed16::ZERO; 4]; 4],
        }
    }

    pub const fn identity() -> Self {
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [o, z, z, z],
                [z, o, z, z],
                [z, z, o, z],
                [z, z, z, o],
            ],
        }
    }

    pub const fn from_rows(
        r0: [Fixed16; 4],
        r1: [Fixed16; 4],
        r2: [Fixed16; 4],
        r3: [Fixed16; 4],
    ) -> Self {
        Self {
            m: [r0, r1, r2, r3],
        }
    }

    /// Dot product of matrix row `row` with a Vec4.
    #[inline]
    fn row_dot(&self, row: usize, v: Vec4) -> Fixed16 {
        self.m[row][0] * v.x + self.m[row][1] * v.y + self.m[row][2] * v.z + self.m[row][3] * v.w
    }

    /// Transform a Vec4 by this matrix (M * v).
    pub fn mul_vec(&self, v: Vec4) -> Vec4 {
        Vec4::new(
            self.row_dot(0, v),
            self.row_dot(1, v),
            self.row_dot(2, v),
            self.row_dot(3, v),
        )
    }

    /// Multiply two matrices (self * rhs).
    pub fn mul_mat(&self, rhs: &Mat4) -> Mat4 {
        let mut result = Mat4::zero();
        for i in 0..4 {
            for j in 0..4 {
                result.m[i][j] = self.m[i][0] * rhs.m[0][j]
                    + self.m[i][1] * rhs.m[1][j]
                    + self.m[i][2] * rhs.m[2][j]
                    + self.m[i][3] * rhs.m[3][j];
            }
        }
        result
    }

    // --- Transform constructors ---

    pub fn translate(v: Vec3) -> Self {
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [o, z, z, v.x],
                [z, o, z, v.y],
                [z, z, o, v.z],
                [z, z, z, o],
            ],
        }
    }

    pub fn scale(v: Vec3) -> Self {
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [v.x, z, z, z],
                [z, v.y, z, z],
                [z, z, v.z, z],
                [z, z, z, o],
            ],
        }
    }

    pub fn rotate_x(angle: Fixed16) -> Self {
        let s = trig::sin(angle);
        let c = trig::cos(angle);
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [o, z, z, z],
                [z, c, -s, z],
                [z, s, c, z],
                [z, z, z, o],
            ],
        }
    }

    pub fn rotate_y(angle: Fixed16) -> Self {
        let s = trig::sin(angle);
        let c = trig::cos(angle);
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [c, z, s, z],
                [z, o, z, z],
                [-s, z, c, z],
                [z, z, z, o],
            ],
        }
    }

    pub fn rotate_z(angle: Fixed16) -> Self {
        let s = trig::sin(angle);
        let c = trig::cos(angle);
        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;
        Self {
            m: [
                [c, -s, z, z],
                [s, c, z, z],
                [z, z, o, z],
                [z, z, z, o],
            ],
        }
    }

    /// Build a view matrix from camera parameters.
    ///
    /// `eye`: camera position, `target`: what the camera looks at,
    /// `up`: world up direction (typically +Y).
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let forward = (target - eye).normalize();
        let right = forward.cross(up).normalize();
        let true_up = right.cross(forward);

        let z = Fixed16::ZERO;
        let o = Fixed16::ONE;

        // Rotation: place right/true_up/-forward into rows (transpose of basis)
        // Translation: dot each basis vector with -eye
        Self {
            m: [
                [right.x, right.y, right.z, -(right.dot(eye))],
                [true_up.x, true_up.y, true_up.z, -(true_up.dot(eye))],
                [-forward.x, -forward.y, -forward.z, forward.dot(eye)],
                [z, z, z, o],
            ],
        }
    }

    /// Build a perspective projection matrix.
    ///
    /// `fov`: vertical field of view (Fixed16 full-circle convention),
    /// `aspect`: width/height ratio, `near`/`far`: clipping planes.
    pub fn perspective(fov: Fixed16, aspect: Fixed16, near: Fixed16, far: Fixed16) -> Self {
        // f = 1 / tan(fov / 2)
        // tan = sin / cos
        let half_fov = Fixed16::from_raw(fov.to_raw() >> 1);
        let s = trig::sin(half_fov);
        let c = trig::cos(half_fov);
        // Guard against zero sin (fov=0 is degenerate)
        let f = if s.to_raw() != 0 { c / s } else { Fixed16::ZERO };

        let z = Fixed16::ZERO;
        let range = near - far;

        Self {
            m: [
                [f / aspect, z, z, z],
                [z, f, z, z],
                [z, z, (far + near) / range, (Fixed16::from_int(2) * far * near) / range],
                [z, z, Fixed16::NEG_ONE, z],
            ],
        }
    }
}

// --- Operator overloads ---

impl core::ops::Mul<Vec4> for Mat4 {
    type Output = Vec4;
    #[inline]
    fn mul(self, rhs: Vec4) -> Vec4 {
        self.mul_vec(rhs)
    }
}

impl core::ops::Mul<Mat4> for Mat4 {
    type Output = Mat4;
    #[inline]
    fn mul(self, rhs: Mat4) -> Mat4 {
        self.mul_mat(&rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Fixed16, b: Fixed16, tolerance: i32) -> bool {
        (a.to_raw() - b.to_raw()).abs() <= tolerance
    }

    fn approx_vec3(a: Vec3, b: Vec3, tolerance: i32) -> bool {
        approx(a.x, b.x, tolerance) && approx(a.y, b.y, tolerance) && approx(a.z, b.z, tolerance)
    }

    #[allow(dead_code)]
    fn approx_vec4(a: Vec4, b: Vec4, tolerance: i32) -> bool {
        approx(a.x, b.x, tolerance)
            && approx(a.y, b.y, tolerance)
            && approx(a.z, b.z, tolerance)
            && approx(a.w, b.w, tolerance)
    }

    // --- Identity ---

    #[test]
    fn identity_times_vec() {
        let v = Vec4::new(
            Fixed16::from_int(1),
            Fixed16::from_int(2),
            Fixed16::from_int(3),
            Fixed16::ONE,
        );
        assert_eq!(Mat4::identity() * v, v);
    }

    #[test]
    fn identity_times_identity() {
        assert_eq!(Mat4::identity() * Mat4::identity(), Mat4::identity());
    }

    // --- Translation ---

    #[test]
    fn translate_moves_point() {
        let t = Mat4::translate(Vec3::from_ints(5, 0, 0));
        let p = Vec3::from_ints(1, 2, 3).to_point();
        let result = t * p;
        assert_eq!(result.to_vec3(), Vec3::from_ints(6, 2, 3));
        assert_eq!(result.w, Fixed16::ONE);
    }

    #[test]
    fn translate_ignores_direction() {
        let t = Mat4::translate(Vec3::from_ints(5, 10, 15));
        let d = Vec3::from_ints(1, 2, 3).to_dir();
        let result = t * d;
        assert_eq!(result.to_vec3(), Vec3::from_ints(1, 2, 3));
        assert_eq!(result.w, Fixed16::ZERO);
    }

    // --- Scale ---

    #[test]
    fn scale_multiplies_components() {
        let s = Mat4::scale(Vec3::from_ints(2, 3, 4));
        let p = Vec3::from_ints(1, 1, 1).to_point();
        let result = (s * p).to_vec3();
        assert_eq!(result, Vec3::from_ints(2, 3, 4));
    }

    // --- Rotation ---

    // Quarter turn in our angle convention: 65536/4 = 16384
    const QUARTER: Fixed16 = Fixed16::from_raw(16384);

    #[test]
    fn rotate_x_90_moves_y_to_z() {
        let r = Mat4::rotate_x(QUARTER);
        let p = Vec3::from_ints(0, 1, 0).to_point();
        let result = (r * p).to_vec3();
        assert!(
            approx_vec3(result, Vec3::from_ints(0, 0, 1), 100),
            "rotate_x(90) * (0,1,0) = ({}, {}, {}), expected (0,0,1)",
            result.x.to_f32(),
            result.y.to_f32(),
            result.z.to_f32()
        );
    }

    #[test]
    fn rotate_y_90_moves_z_to_x() {
        let r = Mat4::rotate_y(QUARTER);
        let p = Vec3::from_ints(0, 0, 1).to_point();
        let result = (r * p).to_vec3();
        assert!(
            approx_vec3(result, Vec3::from_ints(1, 0, 0), 100),
            "rotate_y(90) * (0,0,1) = ({}, {}, {}), expected (1,0,0)",
            result.x.to_f32(),
            result.y.to_f32(),
            result.z.to_f32()
        );
    }

    #[test]
    fn rotate_z_90_moves_x_to_y() {
        let r = Mat4::rotate_z(QUARTER);
        let p = Vec3::from_ints(1, 0, 0).to_point();
        let result = (r * p).to_vec3();
        assert!(
            approx_vec3(result, Vec3::from_ints(0, 1, 0), 100),
            "rotate_z(90) * (1,0,0) = ({}, {}, {}), expected (0,1,0)",
            result.x.to_f32(),
            result.y.to_f32(),
            result.z.to_f32()
        );
    }

    #[test]
    fn rotation_preserves_length() {
        let r = Mat4::rotate_y(Fixed16::from_raw(10000)); // arbitrary angle
        let p = Vec3::from_ints(3, 4, 5);
        let original_len = p.length_squared();
        let rotated = (r * p.to_point()).to_vec3();
        let rotated_len = rotated.length_squared();
        assert!(
            approx(original_len, rotated_len, 500),
            "length changed: {} -> {}",
            original_len.to_f32(),
            rotated_len.to_f32()
        );
    }

    // --- Composition ---

    #[test]
    fn compose_translate_scale() {
        let t = Mat4::translate(Vec3::from_ints(10, 0, 0));
        let s = Mat4::scale(Vec3::from_ints(2, 2, 2));
        let composed = t * s;
        let p = Vec3::from_ints(1, 1, 1).to_point();
        // Scale first (2,2,2), then translate (10,0,0) → (12, 2, 2)
        let result = (composed * p).to_vec3();
        assert_eq!(result, Vec3::from_ints(12, 2, 2));
    }

    #[test]
    fn matrix_mul_associative() {
        let a = Mat4::translate(Vec3::from_ints(1, 2, 3));
        let b = Mat4::scale(Vec3::from_ints(2, 2, 2));
        let c = Mat4::rotate_y(QUARTER);

        let ab_c = (a * b) * c;
        let a_bc = a * (b * c);

        for i in 0..4 {
            for j in 0..4 {
                assert!(
                    approx(ab_c.m[i][j], a_bc.m[i][j], 200),
                    "(A*B)*C != A*(B*C) at [{i}][{j}]: {} vs {}",
                    ab_c.m[i][j].to_f32(),
                    a_bc.m[i][j].to_f32()
                );
            }
        }
    }

    // --- look_at ---

    #[test]
    fn look_at_identity_camera() {
        // Camera at origin, looking down -Z, up is +Y (right-handed)
        let view = Mat4::look_at(
            Vec3::zero(),
            Vec3::from_ints(0, 0, -1),
            Vec3::from_ints(0, 1, 0),
        );
        // A point at (0, 0, -5) maps to view-space z = -5
        // (forward is -Z in both world and view space in this convention)
        let p = Vec3::from_ints(0, 0, -5).to_point();
        let result = (view * p).to_vec3();
        assert!(
            approx(result.z, Fixed16::from_int(-5), 200),
            "look_at: z = {}, expected -5",
            result.z.to_f32()
        );
        // x and y should remain zero (point is on the view axis)
        assert!(approx(result.x, Fixed16::ZERO, 200));
        assert!(approx(result.y, Fixed16::ZERO, 200));
    }

    // --- Perspective ---

    #[test]
    fn perspective_center_stays_center() {
        // FOV = 1/8 turn (45 degrees), aspect = 1, near = 1, far = 100
        let eighth = Fixed16::from_raw(65536 / 8);
        let proj = Mat4::perspective(
            eighth,
            Fixed16::ONE,
            Fixed16::ONE,
            Fixed16::from_int(100),
        );
        // Point on the center axis at z = -10 (in front of camera)
        let p = Vec4::new(
            Fixed16::ZERO,
            Fixed16::ZERO,
            Fixed16::from_int(-10),
            Fixed16::ONE,
        );
        let result = proj * p;
        // x and y should be zero (center of screen)
        assert!(
            approx(result.x, Fixed16::ZERO, 10),
            "center x = {}, expected 0",
            result.x.to_f32()
        );
        assert!(
            approx(result.y, Fixed16::ZERO, 10),
            "center y = {}, expected 0",
            result.y.to_f32()
        );
    }

    // --- sqrt (tested here since it's used by look_at via normalize) ---

    #[test]
    fn sqrt_of_4() {
        let result = Fixed16::from_int(4).sqrt();
        assert!(
            approx(result, Fixed16::from_int(2), 10),
            "sqrt(4) = {}, expected 2",
            result.to_f32()
        );
    }

    #[test]
    fn sqrt_of_1() {
        let result = Fixed16::ONE.sqrt();
        assert!(
            approx(result, Fixed16::ONE, 10),
            "sqrt(1) = {}, expected 1",
            result.to_f32()
        );
    }

    #[test]
    fn sqrt_of_quarter() {
        let result = Fixed16::from_f32(0.25).sqrt();
        assert!(
            approx(result, Fixed16::HALF, 100),
            "sqrt(0.25) = {}, expected 0.5",
            result.to_f32()
        );
    }

    #[test]
    fn sqrt_of_zero() {
        assert_eq!(Fixed16::ZERO.sqrt(), Fixed16::ZERO);
    }
}
