//! 16-bit Galois LFSR pseudo-random number generator for the micro tier.
//!
//! Maximal-length polynomial: x^16 + x^14 + x^13 + x^11 + 1
//! (taps: 0xB400). Period: 2^16 - 1.

/// 16-bit Galois LFSR RNG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsrRng16 {
    state: u16,
}

/// Feedback taps for maximal-length 16-bit LFSR.
const TAPS: u16 = 0xB400;

impl LfsrRng16 {
    /// Create a new RNG with the given seed. Zero seeds are replaced
    /// with a default non-zero value (LFSR must never be zero).
    pub const fn new(seed: u16) -> Self {
        Self {
            state: if seed == 0 { 0xACE1 } else { seed },
        }
    }

    /// Advance the LFSR and return the next raw 8-bit value.
    pub fn next_u8(&mut self) -> u8 {
        let lsb = self.state & 1;
        self.state >>= 1;
        if lsb != 0 {
            self.state ^= TAPS;
        }
        self.state as u8
    }

    /// Return the next 16-bit value (two LFSR steps, little-endian).
    pub fn next_u16(&mut self) -> u16 {
        let lo = self.next_u8() as u16;
        let hi = self.next_u8() as u16;
        (hi << 8) | lo
    }

    /// Random value in `[min, max]` inclusive, using bitmask rejection sampling.
    ///
    /// Uses bitmasking instead of modulo to avoid pulling in `__umodhi3`
    /// (159 bytes) and `__udivmodhi4` (234 bytes) on 6502. Masks to the
    /// smallest power-of-2 >= span, then rejects values outside range.
    /// Worst-case rejection rate is 50% (when span is just above a power of 2).
    pub fn range_u8(&mut self, min: u8, max: u8) -> u8 {
        if min >= max {
            return min;
        }
        let span = (max - min) as u16 + 1;
        if span >= 256 {
            return self.next_u8();
        }
        let span_u8 = span as u8;
        // Find smallest bitmask >= span: next_power_of_two - 1.
        let mask = if span_u8 & (span_u8 - 1) == 0 {
            span_u8 - 1
        } else {
            // Manual next_power_of_two for u8 to avoid widening.
            let mut v = span_u8 - 1;
            v |= v >> 1;
            v |= v >> 2;
            v |= v >> 4;
            v // v is now (next_power_of_two - 1), i.e. the bitmask
        };
        loop {
            let r = self.next_u8() & mask;
            if r < span_u8 {
                return min + r;
            }
        }
    }

    /// 50/50 coin flip.
    pub fn coin(&mut self) -> bool {
        self.next_u8() & 1 != 0
    }

    /// Get current internal state (for seed display / save).
    pub const fn state(&self) -> u16 {
        self.state
    }

    /// Restore RNG from a previously saved state.
    /// Zero is guarded against (LFSR must never be zero).
    pub const fn from_raw_state(raw: u16) -> Self {
        Self {
            state: if raw == 0 { 0xACE1 } else { raw },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence() {
        let mut a = LfsrRng16::new(42);
        let mut b = LfsrRng16::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u8(), b.next_u8());
        }
    }

    #[test]
    fn zero_seed_replaced() {
        let rng = LfsrRng16::new(0);
        assert_ne!(rng.state(), 0);
    }

    #[test]
    fn never_reaches_zero() {
        let mut rng = LfsrRng16::new(1);
        for _ in 0..10_000 {
            rng.next_u8();
            assert_ne!(rng.state(), 0);
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = LfsrRng16::new(1);
        let mut b = LfsrRng16::new(2);
        let mut vals_a = [0u8; 10];
        let mut vals_b = [0u8; 10];
        for v in &mut vals_a {
            *v = a.next_u8();
        }
        for v in &mut vals_b {
            *v = b.next_u8();
        }
        assert_ne!(vals_a, vals_b);
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut rng = LfsrRng16::new(0xCAFE);
        for _ in 0..1000 {
            let v = rng.range_u8(5, 15);
            assert!((5..=15).contains(&v));
        }
    }

    #[test]
    fn range_full_u8() {
        let mut rng = LfsrRng16::new(0xCAFE);
        for _ in 0..1000 {
            let _ = rng.range_u8(0, 255);
        }
    }

    #[test]
    fn range_min_equals_max() {
        let mut rng = LfsrRng16::new(1);
        assert_eq!(rng.range_u8(7, 7), 7);
    }

    #[test]
    fn full_cycle_is_maximal_length() {
        // A maximal-length 16-bit LFSR visits all 2^16-1 = 65535 non-zero
        // states before returning to the initial state.
        let mut rng = LfsrRng16::new(1);
        let initial = rng.state();
        for i in 0u32..65_535 {
            rng.next_u8();
            if i < 65_534 {
                assert_ne!(
                    rng.state(),
                    initial,
                    "returned to initial state after only {i} steps"
                );
            }
        }
        assert_eq!(
            rng.state(),
            initial,
            "should return to initial state after 65535 steps"
        );
    }
}
