use super::fixed::Fixed16;

/// 256-entry sine lookup table. Each value is a raw Fixed16 representing
/// sin(i * 2*pi / 256) for i in 0..256.
///
/// Angle convention: Fixed16::ONE (65536 raw) = one full circle (360 degrees).
/// The top 8 bits of the raw angle value index into this table.
/// No interpolation — PS1-authentic coarseness (~1.4 degree resolution).
#[rustfmt::skip]
const SIN_TABLE: [i32; 256] = [
         0,   1608,   3216,   4821,   6424,   8022,   9616,  11204,
     12785,  14359,  15924,  17479,  19024,  20557,  22078,  23586,
     25080,  26558,  28020,  29466,  30893,  32303,  33692,  35062,
     36410,  37736,  39040,  40320,  41576,  42806,  44011,  45190,
     46341,  47464,  48559,  49624,  50660,  51665,  52639,  53581,
     54491,  55368,  56212,  57022,  57798,  58538,  59244,  59914,
     60547,  61145,  61705,  62228,  62714,  63162,  63572,  63944,
     64277,  64571,  64827,  65043,  65220,  65358,  65457,  65516,
     65536,  65516,  65457,  65358,  65220,  65043,  64827,  64571,
     64277,  63944,  63572,  63162,  62714,  62228,  61705,  61145,
     60547,  59914,  59244,  58538,  57798,  57022,  56212,  55368,
     54491,  53581,  52639,  51665,  50660,  49624,  48559,  47464,
     46341,  45190,  44011,  42806,  41576,  40320,  39040,  37736,
     36410,  35062,  33692,  32303,  30893,  29466,  28020,  26558,
     25080,  23586,  22078,  20557,  19024,  17479,  15924,  14359,
     12785,  11204,   9616,   8022,   6424,   4821,   3216,   1608,
         0,  -1608,  -3216,  -4821,  -6424,  -8022,  -9616, -11204,
    -12785, -14359, -15924, -17479, -19024, -20557, -22078, -23586,
    -25080, -26558, -28020, -29466, -30893, -32303, -33692, -35062,
    -36410, -37736, -39040, -40320, -41576, -42806, -44011, -45190,
    -46341, -47464, -48559, -49624, -50660, -51665, -52639, -53581,
    -54491, -55368, -56212, -57022, -57798, -58538, -59244, -59914,
    -60547, -61145, -61705, -62228, -62714, -63162, -63572, -63944,
    -64277, -64571, -64827, -65043, -65220, -65358, -65457, -65516,
    -65536, -65516, -65457, -65358, -65220, -65043, -64827, -64571,
    -64277, -63944, -63572, -63162, -62714, -62228, -61705, -61145,
    -60547, -59914, -59244, -58538, -57798, -57022, -56212, -55368,
    -54491, -53581, -52639, -51665, -50660, -49624, -48559, -47464,
    -46341, -45190, -44011, -42806, -41576, -40320, -39040, -37736,
    -36410, -35062, -33692, -32303, -30893, -29466, -28020, -26558,
    -25080, -23586, -22078, -20557, -19024, -17479, -15924, -14359,
    -12785, -11204,  -9616,  -8022,  -6424,  -4821,  -3216,  -1608,
];

/// Sine of an angle. Angle uses the Fixed16 full-circle convention:
/// `Fixed16::ONE` (raw 65536) = 360 degrees = 2*pi radians.
pub fn sin(angle: Fixed16) -> Fixed16 {
    // Mask to one full cycle (bottom 16 bits) and extract 8-bit table index
    let raw = angle.to_raw() & 0xFFFF;
    let idx = ((raw >> 8) & 0xFF) as usize;
    Fixed16::from_raw(SIN_TABLE[idx])
}

/// Cosine of an angle. Same convention as [`sin`].
pub fn cos(angle: Fixed16) -> Fixed16 {
    // cos(x) = sin(x + quarter turn). Quarter turn = 64 entries = 0x4000 raw.
    sin(angle + Fixed16::from_raw(0x4000))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Quarter turn in raw units: 65536 / 4 = 16384
    const QUARTER: Fixed16 = Fixed16::from_raw(16384);
    const HALF_TURN: Fixed16 = Fixed16::from_raw(32768);
    const THREE_QUARTER: Fixed16 = Fixed16::from_raw(49152);

    fn approx(a: Fixed16, b: Fixed16, tolerance: i32) -> bool {
        (a.to_raw() - b.to_raw()).abs() <= tolerance
    }

    #[test]
    fn sin_zero() {
        assert_eq!(sin(Fixed16::ZERO), Fixed16::ZERO);
    }

    #[test]
    fn sin_90() {
        assert!(approx(sin(QUARTER), Fixed16::ONE, 1));
    }

    #[test]
    fn sin_180() {
        assert!(approx(sin(HALF_TURN), Fixed16::ZERO, 1));
    }

    #[test]
    fn sin_270() {
        assert!(approx(sin(THREE_QUARTER), Fixed16::NEG_ONE, 1));
    }

    #[test]
    fn cos_zero() {
        assert!(approx(cos(Fixed16::ZERO), Fixed16::ONE, 1));
    }

    #[test]
    fn cos_90() {
        assert!(approx(cos(QUARTER), Fixed16::ZERO, 1));
    }

    #[test]
    fn sin_cos_identity() {
        // sin^2 + cos^2 = 1 for several angles
        for raw in [0, 8000, 16384, 24000, 32768, 50000] {
            let angle = Fixed16::from_raw(raw);
            let s = sin(angle);
            let c = cos(angle);
            let sum = s * s + c * c;
            assert!(
                approx(sum, Fixed16::ONE, 800),
                "sin^2+cos^2 at raw {} = {} (expected 1.0)",
                raw,
                sum.to_f32()
            );
        }
    }

    #[test]
    fn negative_angle_wraps() {
        // sin(-quarter) should equal sin(three_quarter) due to masking
        let neg = sin(Fixed16::from_raw(-16384));
        let pos = sin(THREE_QUARTER);
        assert_eq!(neg, pos);
    }
}
