//! 32-bit Galois LFSR pseudo-random number generator for the compact tier.
//!
//! Maximal-length polynomial: x^32 + x^22 + x^2 + x + 1
//! (taps: 0x80200003). Period: 2^32 - 1.

/// 32-bit Galois LFSR RNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsrRng32 {
    state: u32,
}

/// Feedback taps for maximal-length 32-bit LFSR.
const TAPS: u32 = 0x80200003;

impl LfsrRng32 {
    /// Create a new RNG with the given seed. Zero seeds are replaced
    /// with a default non-zero value (LFSR must never be zero).
    pub const fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0xDEAD_BEEF } else { seed },
        }
    }

    /// Advance the LFSR and return the next raw 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        let lsb = self.state & 1;
        self.state >>= 1;
        if lsb != 0 {
            self.state ^= TAPS;
        }
        self.state
    }

    /// Return the next value as a `u8` (low byte).
    pub fn next_u8(&mut self) -> u8 {
        self.next_u32() as u8
    }

    /// Return the next value as a `u16` (low two bytes).
    pub fn next_u16(&mut self) -> u16 {
        self.next_u32() as u16
    }

    /// Random value in `[min, max]` inclusive, using rejection sampling.
    pub fn range_u8(&mut self, min: u8, max: u8) -> u8 {
        if min >= max {
            return min;
        }
        let span = max as u16 - min as u16 + 1;
        if span == 256 {
            // Full u8 range — no modulo needed.
            return self.next_u8();
        }
        let span_u8 = span as u8;
        let reject = (256u16 % span) as u8;
        if reject == 0 {
            return min + (self.next_u8() % span_u8);
        }
        let threshold = (256u16 - reject as u16) as u8;
        loop {
            let r = self.next_u8();
            if r < threshold {
                return min + (r % span_u8);
            }
        }
    }

    /// 50/50 coin flip.
    pub fn coin(&mut self) -> bool {
        self.next_u32() & 1 != 0
    }

    /// Get current internal state (for seed display / save).
    pub const fn state(&self) -> u32 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence() {
        let mut a = LfsrRng32::new(42);
        let mut b = LfsrRng32::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn zero_seed_replaced() {
        let rng = LfsrRng32::new(0);
        assert_ne!(rng.state(), 0);
    }

    #[test]
    fn never_reaches_zero() {
        // Run enough steps to be confident the LFSR doesn't collapse.
        let mut rng = LfsrRng32::new(1);
        for _ in 0..10_000 {
            rng.next_u32();
            assert_ne!(rng.state(), 0);
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = LfsrRng32::new(1);
        let mut b = LfsrRng32::new(2);
        let vals_a: Vec<u32> = (0..10).map(|_| a.next_u32()).collect();
        let vals_b: Vec<u32> = (0..10).map(|_| b.next_u32()).collect();
        assert_ne!(vals_a, vals_b);
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut rng = LfsrRng32::new(0xCAFE);
        for _ in 0..1000 {
            let v = rng.range_u8(5, 15);
            assert!((5..=15).contains(&v));
        }
    }

    #[test]
    fn range_full_u8() {
        // 0..=255 must not panic (was a division-by-zero bug).
        let mut rng = LfsrRng32::new(0xCAFE);
        for _ in 0..1000 {
            let _ = rng.range_u8(0, 255);
        }
    }

    #[test]
    fn range_min_equals_max() {
        let mut rng = LfsrRng32::new(1);
        assert_eq!(rng.range_u8(7, 7), 7);
    }
}
