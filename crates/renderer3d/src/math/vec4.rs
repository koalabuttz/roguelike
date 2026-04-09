use super::fixed::Fixed16;
use super::vec3::Vec3;

/// 4D homogeneous vector with Fixed16 components.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vec4 {
    pub x: Fixed16,
    pub y: Fixed16,
    pub z: Fixed16,
    pub w: Fixed16,
}

const _: () = assert!(core::mem::size_of::<Vec4>() == 16);

impl Vec4 {
    #[inline]
    pub const fn new(x: Fixed16, y: Fixed16, z: Fixed16, w: Fixed16) -> Self {
        Self { x, y, z, w }
    }

    #[inline]
    pub const fn zero() -> Self {
        Self {
            x: Fixed16::ZERO,
            y: Fixed16::ZERO,
            z: Fixed16::ZERO,
            w: Fixed16::ZERO,
        }
    }

    #[inline]
    pub fn dot(self, rhs: Self) -> Fixed16 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z + self.w * rhs.w
    }

    #[inline]
    pub fn scale(self, s: Fixed16) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
            w: self.w * s,
        }
    }

    /// Perspective divide: project from homogeneous to 3D by dividing xyz by w.
    /// This is the core operation that makes distant objects smaller.
    ///
    /// Panics (debug) if w is zero — direction vectors cannot be projected.
    #[inline]
    pub fn perspective_divide(self) -> Vec3 {
        debug_assert!(self.w != Fixed16::ZERO, "perspective_divide on w=0 vector");
        Vec3::new(self.x / self.w, self.y / self.w, self.z / self.w)
    }

    /// Drop w without dividing. Use when you know w is 1 or don't need projection.
    #[inline]
    pub const fn to_vec3(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

impl core::ops::Add for Vec4 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
            w: self.w + rhs.w,
        }
    }
}

impl core::ops::Sub for Vec4 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
            w: self.w - rhs.w,
        }
    }
}

impl core::ops::Neg for Vec4 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: -self.w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_zero() {
        let v = Vec4::zero();
        assert_eq!(v.x, Fixed16::ZERO);
        assert_eq!(v.y, Fixed16::ZERO);
        assert_eq!(v.z, Fixed16::ZERO);
        assert_eq!(v.w, Fixed16::ZERO);
    }

    #[test]
    fn add_vecs() {
        let a = Vec4::new(
            Fixed16::from_int(1),
            Fixed16::from_int(2),
            Fixed16::from_int(3),
            Fixed16::from_int(4),
        );
        let b = Vec4::new(
            Fixed16::from_int(10),
            Fixed16::from_int(20),
            Fixed16::from_int(30),
            Fixed16::from_int(40),
        );
        let c = a + b;
        assert_eq!(c.x, Fixed16::from_int(11));
        assert_eq!(c.w, Fixed16::from_int(44));
    }

    #[test]
    fn dot_four_components() {
        let a = Vec4::new(
            Fixed16::from_int(1),
            Fixed16::from_int(2),
            Fixed16::from_int(3),
            Fixed16::from_int(4),
        );
        let b = Vec4::new(
            Fixed16::from_int(2),
            Fixed16::from_int(3),
            Fixed16::from_int(4),
            Fixed16::from_int(5),
        );
        // 1*2 + 2*3 + 3*4 + 4*5 = 2 + 6 + 12 + 20 = 40
        assert_eq!(a.dot(b), Fixed16::from_int(40));
    }

    #[test]
    fn perspective_divide_basic() {
        let v = Vec4::new(
            Fixed16::from_int(2),
            Fixed16::from_int(4),
            Fixed16::from_int(6),
            Fixed16::from_int(2),
        );
        let p = v.perspective_divide();
        assert_eq!(p, Vec3::from_ints(1, 2, 3));
    }

    #[test]
    fn perspective_divide_identity() {
        let v = Vec3::from_ints(5, 10, 15).to_point();
        let p = v.perspective_divide();
        assert_eq!(p, Vec3::from_ints(5, 10, 15));
    }

    #[test]
    fn to_vec3_drops_w() {
        let v = Vec4::new(
            Fixed16::from_int(1),
            Fixed16::from_int(2),
            Fixed16::from_int(3),
            Fixed16::from_int(99),
        );
        assert_eq!(v.to_vec3(), Vec3::from_ints(1, 2, 3));
    }

    #[test]
    fn roundtrip_point() {
        let v = Vec3::from_ints(1, 2, 3);
        assert_eq!(v.to_point().to_vec3(), v);
    }

    #[test]
    fn roundtrip_point_perspective() {
        let v = Vec3::from_ints(7, 11, 13);
        assert_eq!(v.to_point().perspective_divide(), v);
    }
}
