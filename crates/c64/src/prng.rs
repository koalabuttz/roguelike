// 16-bit Galois LFSR pseudo-random number generator.
//
// The C64 has no hardware RNG. We use a linear feedback shift register
// seeded from the CIA timer (human timing jitter) at title screen.
// Polynomial: x^16 + x^14 + x^13 + x^11 + 1 (taps: 0x002D as feedback)
//
// This matches the approach in the C64 port proposal (§3.2).

static mut RNG_STATE: u16 = 0xACE1; // non-zero default seed

/// Seed the PRNG. Must be non-zero.
pub fn seed(s: u16) {
    unsafe {
        RNG_STATE = if s == 0 { 0xACE1 } else { s };
    }
}

/// Generate next pseudo-random byte.
#[inline(always)]
pub fn next_u8() -> u8 {
    unsafe {
        let mut state = RNG_STATE;
        let lsb = state & 1;
        state >>= 1;
        if lsb != 0 {
            state ^= 0xB400; // taps for maximal-length 16-bit LFSR
        }
        RNG_STATE = state;
        state as u8
    }
}

/// Generate next pseudo-random 16-bit value.
pub fn next_u16() -> u16 {
    let lo = next_u8() as u16;
    let hi = next_u8() as u16;
    (hi << 8) | lo
}

/// Random value in [min, max] inclusive.
/// Uses rejection sampling to avoid modulo bias.
pub fn range(min: u8, max: u8) -> u8 {
    if min >= max {
        return min;
    }
    let span = max - min + 1;
    let reject = (256u16 % span as u16) as u8;
    if reject == 0 {
        // span evenly divides 256 (e.g. 2, 4, 8...) — no bias, accept any value
        return min + (next_u8() % span);
    }
    let threshold = (256u16 - reject as u16) as u8; // wraps to 0 only if reject==0, handled above
    loop {
        let r = next_u8();
        if r < threshold {
            return min + (r % span);
        }
    }
}

/// 50/50 coin flip.
#[inline(always)]
pub fn coin() -> bool {
    next_u8() & 1 != 0
}

/// Get current RNG state (for seed display / save).
pub fn state() -> u16 {
    unsafe { RNG_STATE }
}
