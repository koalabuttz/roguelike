use super::fixed::Fixed16;
use super::vec4::Vec4;

/// 3D vector with Fixed16 components.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vec3 {
    pub x: Fixed16,
    pub y: Fixed16,
    pub z: Fixed16,
}

const _: () = assert!(core::mem::size_of::<Vec3>() == 12);

impl Vec3 {
    #[inline]
    pub const fn new(x: Fixed16, y: Fixed16, z: Fixed16) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub const fn zero() -> Self {
        Self {
            x: Fixed16::ZERO,
            y: Fixed16::ZERO,
            z: Fixed16::ZERO,
        }
    }

    #[inline]
    pub const fn from_ints(x: i32, y: i32, z: i32) -> Self {
        Self {
            x: Fixed16::from_int(x),
            y: Fixed16::from_int(y),
            z: Fixed16::from_int(z),
        }
    }

    #[inline]
    pub const fn splat(v: Fixed16) -> Self {
        Self { x: v, y: v, z: v }
    }

    #[inline]
    pub fn dot(self, rhs: Self) -> Fixed16 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    #[inline]
    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: self.y * rhs.z - self.z * rhs.y,
            y: self.z * rhs.x - self.x * rhs.z,
            z: self.x * rhs.y - self.y * rhs.x,
        }
    }

    #[inline]
    pub fn length_squared(self) -> Fixed16 {
        self.dot(self)
    }

    #[inline]
    pub fn scale(self, s: Fixed16) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }

    /// Convert to a homogeneous point (w = 1). Affected by translation.
    #[inline]
    pub fn to_point(self) -> Vec4 {
        Vec4::new(self.x, self.y, self.z, Fixed16::ONE)
    }

    /// Convert to a homogeneous direction (w = 0). Not affected by translation.
    #[inline]
    pub fn to_dir(self) -> Vec4 {
        Vec4::new(self.x, self.y, self.z, Fixed16::ZERO)
    }

    /// Length via fixed-point square root (Newton's method).
    #[inline]
    pub fn length(self) -> Fixed16 {
        self.length_squared().sqrt()
    }

    /// Normalize to unit length. Panics on zero-length vector.
    pub fn normalize(self) -> Self {
        let len = self.length();
        debug_assert!(len != Fixed16::ZERO, "cannot normalize zero-length vector");
        Self {
            x: self.x / len,
            y: self.y / len,
            z: self.z / len,
        }
    }
}

impl core::ops::Add for Vec3 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl core::ops::Sub for Vec3 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl core::ops::Neg for Vec3 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_zero() {
        let v = Vec3::zero();
        assert_eq!(v.x, Fixed16::ZERO);
        assert_eq!(v.y, Fixed16::ZERO);
        assert_eq!(v.z, Fixed16::ZERO);
    }

    #[test]
    fn from_ints_basic() {
        let v = Vec3::from_ints(1, 2, 3);
        assert_eq!(v.x.to_int(), 1);
        assert_eq!(v.y.to_int(), 2);
        assert_eq!(v.z.to_int(), 3);
    }

    #[test]
    fn add_vecs() {
        let a = Vec3::from_ints(1, 2, 3);
        let b = Vec3::from_ints(4, 5, 6);
        let c = a + b;
        assert_eq!(c, Vec3::from_ints(5, 7, 9));
    }

    #[test]
    fn sub_vecs() {
        let a = Vec3::from_ints(5, 7, 9);
        let b = Vec3::from_ints(4, 5, 6);
        assert_eq!(a - b, Vec3::from_ints(1, 2, 3));
    }

    #[test]
    fn neg_vec() {
        assert_eq!(-Vec3::from_ints(1, 2, 3), Vec3::from_ints(-1, -2, -3));
    }

    #[test]
    fn dot_perpendicular() {
        let x = Vec3::from_ints(1, 0, 0);
        let y = Vec3::from_ints(0, 1, 0);
        assert_eq!(x.dot(y), Fixed16::ZERO);
    }

    #[test]
    fn dot_parallel() {
        let x = Vec3::from_ints(1, 0, 0);
        let x3 = Vec3::from_ints(3, 0, 0);
        assert_eq!(x.dot(x3), Fixed16::from_int(3));
    }

    #[test]
    fn cross_basis_vectors() {
        let x = Vec3::from_ints(1, 0, 0);
        let y = Vec3::from_ints(0, 1, 0);
        let z = Vec3::from_ints(0, 0, 1);
        assert_eq!(x.cross(y), z);
        assert_eq!(y.cross(z), x);
        assert_eq!(z.cross(x), y);
    }

    #[test]
    fn cross_anticommutative() {
        let a = Vec3::from_ints(1, 2, 3);
        let b = Vec3::from_ints(4, 5, 6);
        assert_eq!(a.cross(b), -b.cross(a));
    }

    #[test]
    fn length_squared_345() {
        let v = Vec3::from_ints(3, 4, 0);
        assert_eq!(v.length_squared(), Fixed16::from_int(25));
    }

    #[test]
    fn scale_vec() {
        let v = Vec3::from_ints(1, 2, 3);
        let s = Fixed16::from_int(2);
        assert_eq!(v.scale(s), Vec3::from_ints(2, 4, 6));
    }

    #[test]
    fn to_point_sets_w_one() {
        let v = Vec3::from_ints(1, 2, 3).to_point();
        assert_eq!(v.w, Fixed16::ONE);
    }

    #[test]
    fn to_dir_sets_w_zero() {
        let v = Vec3::from_ints(1, 2, 3).to_dir();
        assert_eq!(v.w, Fixed16::ZERO);
    }

    #[test]
    fn normalize_unit_x() {
        let v = Vec3::from_ints(1, 0, 0).normalize();
        assert!((v.x.to_raw() - Fixed16::ONE.to_raw()).abs() <= 2);
        assert_eq!(v.y, Fixed16::ZERO);
        assert_eq!(v.z, Fixed16::ZERO);
    }

    #[test]
    fn normalize_345() {
        let v = Vec3::from_ints(3, 4, 0).normalize();
        let len = v.length();
        assert!(
            (len.to_raw() - Fixed16::ONE.to_raw()).abs() <= 200,
            "normalized length = {} (expected 1.0)",
            len.to_f32()
        );
    }
}
