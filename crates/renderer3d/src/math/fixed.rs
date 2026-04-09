/// 16.16 fixed-point number backed by i32.
///
/// The top 16 bits are the integer part (signed, range -32768..32767).
/// The bottom 16 bits are the fractional part (precision 1/65536).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Fixed16(i32);

const _: () = assert!(core::mem::size_of::<Fixed16>() == 4);

impl Fixed16 {
    pub const FRAC_BITS: u32 = 16;
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1 << 16);
    pub const NEG_ONE: Self = Self(-(1 << 16));
    pub const HALF: Self = Self(1 << 15);

    #[inline]
    pub const fn from_int(n: i32) -> Self {
        Self(n << 16)
    }

    #[inline]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn to_raw(self) -> i32 {
        self.0
    }

    #[inline]
    pub const fn to_int(self) -> i32 {
        self.0 >> 16
    }

    #[inline]
    pub const fn floor(self) -> Self {
        Self(self.0 & !0xFFFF)
    }

    #[inline]
    pub const fn ceil(self) -> Self {
        if self.0 & 0xFFFF == 0 {
            self
        } else {
            Self((self.0 & !0xFFFF) + (1 << 16))
        }
    }

    #[inline]
    pub const fn frac(self) -> Self {
        Self(self.0 & 0xFFFF)
    }

    #[inline]
    pub const fn abs(self) -> Self {
        if self.0 < 0 { Self(-self.0) } else { self }
    }

    /// Fixed-point square root via Newton's method (4 iterations).
    /// Returns zero for zero or negative inputs.
    pub fn sqrt(self) -> Self {
        if self.0 <= 0 {
            return Self::ZERO;
        }
        // Compute sqrt in the i64 domain scaled by FRAC_BITS for correct
        // fixed-point result: sqrt(x_fixed) = sqrt(x_raw * 2^16) in raw units,
        // which equals sqrt(x_raw) * 2^8. We shift x_raw left by 16 first so
        // that the integer sqrt directly gives us the 16.16 result.
        let val = (self.0 as i64) << 16;
        // Initial guess via bit-level estimate: half the bit-width of val
        let bits = 64 - val.leading_zeros();
        let mut guess = 1i64 << (bits.div_ceil(2));
        // Newton's method: guess = (guess + val / guess) / 2
        for _ in 0..4 {
            if guess == 0 {
                break;
            }
            guess = (guess + val / guess) >> 1;
        }
        Self(guess as i32)
    }

    #[cfg(any(test, feature = "std"))]
    #[inline]
    pub fn to_f32(self) -> f32 {
        self.0 as f32 / 65536.0
    }

    #[cfg(any(test, feature = "std"))]
    #[inline]
    pub fn from_f32(v: f32) -> Self {
        Self((v * 65536.0) as i32)
    }
}

// --- Arithmetic operators ---

impl core::ops::Add for Fixed16 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl core::ops::AddAssign for Fixed16 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl core::ops::Sub for Fixed16 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl core::ops::SubAssign for Fixed16 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl core::ops::Mul for Fixed16 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self(((self.0 as i64 * rhs.0 as i64) >> 16) as i32)
    }
}

impl core::ops::MulAssign for Fixed16 {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl core::ops::Div for Fixed16 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        Self((((self.0 as i64) << 16) / (rhs.0 as i64)) as i32)
    }
}

impl core::ops::DivAssign for Fixed16 {
    #[inline]
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

impl core::ops::Neg for Fixed16 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

// --- Display ---

impl core::fmt::Debug for Fixed16 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(any(test, feature = "std"))]
        {
            write!(f, "Fixed16({:.4})", self.to_f32())
        }
        #[cfg(not(any(test, feature = "std")))]
        {
            write!(f, "Fixed16(raw=0x{:08X})", self.0)
        }
    }
}

impl core::fmt::Display for Fixed16 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        #[cfg(any(test, feature = "std"))]
        {
            write!(f, "{:.4}", self.to_f32())
        }
        #[cfg(not(any(test, feature = "std")))]
        {
            write!(f, "0x{:08X}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_int_roundtrip() {
        assert_eq!(Fixed16::from_int(5).to_int(), 5);
        assert_eq!(Fixed16::from_int(-3).to_int(), -3);
        assert_eq!(Fixed16::from_int(0).to_int(), 0);
    }

    #[test]
    fn from_raw_roundtrip() {
        assert_eq!(Fixed16::from_raw(12345).to_raw(), 12345);
        assert_eq!(Fixed16::from_raw(-99999).to_raw(), -99999);
    }

    #[test]
    fn constants() {
        assert_eq!(Fixed16::ZERO.to_raw(), 0);
        assert_eq!(Fixed16::ONE.to_raw(), 65536);
        assert_eq!(Fixed16::NEG_ONE.to_raw(), -65536);
        assert_eq!(Fixed16::HALF.to_raw(), 32768);
    }

    #[test]
    fn add_basic() {
        assert_eq!(
            Fixed16::from_int(3) + Fixed16::from_int(4),
            Fixed16::from_int(7)
        );
    }

    #[test]
    fn sub_basic() {
        assert_eq!(
            Fixed16::from_int(7) - Fixed16::from_int(3),
            Fixed16::from_int(4)
        );
    }

    #[test]
    fn mul_integers() {
        assert_eq!(
            Fixed16::from_int(3) * Fixed16::from_int(4),
            Fixed16::from_int(12)
        );
    }

    #[test]
    fn mul_fractions() {
        let a = Fixed16::from_f32(1.5);
        let b = Fixed16::from_f32(2.0);
        let result = a * b;
        let expected = Fixed16::from_f32(3.0);
        assert!((result.to_raw() - expected.to_raw()).abs() <= 1);
    }

    #[test]
    fn div_basic() {
        assert_eq!(
            Fixed16::from_int(12) / Fixed16::from_int(4),
            Fixed16::from_int(3)
        );
    }

    #[test]
    fn div_fractions() {
        let result = Fixed16::from_int(1) / Fixed16::from_int(3);
        let expected = Fixed16::from_f32(1.0 / 3.0);
        assert!(
            (result.to_raw() - expected.to_raw()).abs() <= 2,
            "1/3 = {} (raw {}), expected {} (raw {})",
            result.to_f32(),
            result.to_raw(),
            expected.to_f32(),
            expected.to_raw()
        );
    }

    #[test]
    fn neg_basic() {
        assert_eq!(-Fixed16::from_int(5), Fixed16::from_int(-5));
        assert_eq!(-Fixed16::ZERO, Fixed16::ZERO);
    }

    #[test]
    fn ordering() {
        assert!(Fixed16::from_int(1) > Fixed16::ZERO);
        assert!(Fixed16::from_int(-1) < Fixed16::ZERO);
        assert!(Fixed16::from_int(-5) < Fixed16::from_int(-3));
    }

    #[test]
    fn floor_positive() {
        assert_eq!(Fixed16::from_f32(3.7).floor(), Fixed16::from_int(3));
    }

    #[test]
    fn floor_negative() {
        assert_eq!(Fixed16::from_f32(-3.7).floor(), Fixed16::from_int(-4));
    }

    #[test]
    fn ceil_positive() {
        assert_eq!(Fixed16::from_f32(3.2).ceil(), Fixed16::from_int(4));
    }

    #[test]
    fn ceil_exact() {
        assert_eq!(Fixed16::from_int(3).ceil(), Fixed16::from_int(3));
    }

    #[test]
    fn abs_positive() {
        assert_eq!(Fixed16::from_int(5).abs(), Fixed16::from_int(5));
    }

    #[test]
    fn abs_negative() {
        assert_eq!(Fixed16::from_int(-5).abs(), Fixed16::from_int(5));
    }

    #[test]
    fn frac_extracts_fractional() {
        let v = Fixed16::from_f32(3.75);
        let frac = v.frac();
        assert!((frac.to_f32() - 0.75).abs() < 0.001);
    }

    #[test]
    fn mul_world_scale() {
        // Verify 80*80 doesn't overflow (world-scale coordinates)
        let a = Fixed16::from_int(80);
        let b = Fixed16::from_int(80);
        assert_eq!(a * b, Fixed16::from_int(6400));
    }
}
