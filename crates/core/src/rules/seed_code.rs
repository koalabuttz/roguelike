//! No-std seed encoding and decoding.
//!
//! Base36 encode/decode using fixed-size buffers. These functions are
//! used by the standard-tier `seed_code` module (which wraps them with
//! `String`-based APIs) and will be used directly by micro/compact tiers.

use core::mem::size_of;

// ── Tier ──────────────────────────────────────────────────────────────

/// Which capability tier a seed targets, inferred from its numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Tier {
    /// seed <= 0xFFFF — 16-bit seeds for C64 (u8 coords, LFSR-16).
    Micro = 0,
    /// seed <= 0xFFFF_FFFF — 32-bit seeds for GBA (i16 coords, LFSR-32).
    Compact = 1,
    /// seed > 0xFFFF_FFFF — 64-bit seeds for Vita/PC (i32 coords, ChaCha20).
    Standard = 2,
}

const _: () = assert!(size_of::<Tier>() == 1);

/// Determine the capability tier from a seed value.
pub const fn tier_from_seed(seed: u64) -> Tier {
    if seed <= 0xFFFF {
        Tier::Micro
    } else if seed <= 0xFFFF_FFFF {
        Tier::Compact
    } else {
        Tier::Standard
    }
}

// ── Base36 encode/decode ──────────────────────────────────────────────

const BASE36_CHARS: [u8; 36] = *b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Maximum number of base36 digits for a `u64` value.
/// `u64::MAX` = `3w5e11264sgsf` = 13 digits.
pub const MAX_BASE36_LEN: usize = 13;

/// Encode a `u64` seed into base36, writing ASCII bytes into `buf`.
///
/// Returns the number of bytes written (1..=13). The encoded digits
/// occupy `buf[0..len]`.
pub const fn encode_to_buf(mut seed: u64, buf: &mut [u8; MAX_BASE36_LEN]) -> usize {
    if seed == 0 {
        buf[0] = b'0';
        return 1;
    }

    // Write least-significant digits forward, then reverse in-place.
    let mut count = 0usize;
    while seed > 0 {
        buf[count] = BASE36_CHARS[(seed % 36) as usize];
        seed /= 36;
        count += 1;
    }

    // Reverse buf[0..count] in-place.
    let mut lo = 0;
    let mut hi = count - 1;
    while lo < hi {
        let tmp = buf[lo];
        buf[lo] = buf[hi];
        buf[hi] = tmp;
        lo += 1;
        hi -= 1;
    }
    count
}

// ── Micro seed code encode ──────────────────────────────────────────

/// Maximum length of a micro-tier seed code: `"{base36}-{W}x{H}"`.
///
/// Max base36 for u16: 4 chars (`1ekf`), `-`: 1, width: 3, `x`: 1, height: 3 = 12.
pub const MAX_MICRO_SEED_CODE_LEN: usize = 12;

/// Write a `u8` value as 1–3 decimal ASCII digits into `buf` at `offset`.
///
/// Returns the new offset (one past the last digit written).
const fn write_u8_decimal(val: u8, buf: &mut [u8], mut offset: usize) -> usize {
    if val >= 100 {
        buf[offset] = b'0' + val / 100;
        offset += 1;
    }
    if val >= 10 {
        buf[offset] = b'0' + (val / 10) % 10;
        offset += 1;
    }
    buf[offset] = b'0' + val % 10;
    offset + 1
}

/// Encode a micro-tier seed code into a fixed buffer.
///
/// Format: `"{base36_seed}-{width}x{height}"` (always includes dimensions).
/// Returns the number of bytes written to `buf[0..len]`.
pub const fn encode_micro_to_buf(
    seed: u16,
    width: u8,
    height: u8,
    buf: &mut [u8; MAX_MICRO_SEED_CODE_LEN],
) -> usize {
    // Encode the base36 seed into a temporary buffer.
    let mut b36 = [0u8; MAX_BASE36_LEN];
    let seed_len = encode_to_buf(seed as u64, &mut b36);

    // Copy base36 digits.
    let mut pos = 0;
    while pos < seed_len {
        buf[pos] = b36[pos];
        pos += 1;
    }

    // Separator.
    buf[pos] = b'-';
    pos += 1;

    // Width, 'x', height.
    pos = write_u8_decimal(width, buf, pos);
    buf[pos] = b'x';
    pos += 1;
    pos = write_u8_decimal(height, buf, pos);

    pos
}

// ── Base36 decode ───────────────────────────────────────────────────

/// Error from decoding a base36 byte slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SeedDecodeError {
    /// Input was empty.
    Empty = 0,
    /// Encountered a byte that is not `0-9`, `a-z`, or `A-Z`.
    InvalidChar(u8) = 1,
    /// Value overflowed `u64`.
    Overflow = 2,
    /// Seed value exceeds micro tier range (> 0xFFFF).
    NotMicroTier = 3,
    /// Width or height out of valid range.
    InvalidDimensions = 4,
}

const _: () = assert!(size_of::<SeedDecodeError>() == 2);

/// Decoded micro-tier seed code parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroSeedParams {
    pub seed: u16,
    pub width: u8,
    pub height: u8,
}

/// Decode a base36-encoded byte slice into a `u64`.
///
/// Case-insensitive: accepts ASCII `0-9`, `a-z`, and `A-Z`.
pub const fn decode_from_bytes(bytes: &[u8]) -> Result<u64, SeedDecodeError> {
    if bytes.is_empty() {
        return Err(SeedDecodeError::Empty);
    }

    let mut result: u64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let digit = if b >= b'0' && b <= b'9' {
            (b - b'0') as u64
        } else if b >= b'a' && b <= b'z' {
            (b - b'a') as u64 + 10
        } else if b >= b'A' && b <= b'Z' {
            (b - b'A') as u64 + 10
        } else {
            return Err(SeedDecodeError::InvalidChar(b));
        };

        let Some(r) = result.checked_mul(36) else {
            return Err(SeedDecodeError::Overflow);
        };
        let Some(r) = r.checked_add(digit) else {
            return Err(SeedDecodeError::Overflow);
        };
        result = r;
        i += 1;
    }

    Ok(result)
}

// ── Micro seed code decode ─────────────────────────────────────────

/// Parse a `u8` decimal number from a byte slice starting at `offset`.
///
/// Returns `(value, new_offset)` or `None` if no digits found.
const fn parse_u8_decimal(bytes: &[u8], mut offset: usize) -> Option<(u8, usize)> {
    if offset >= bytes.len() || bytes[offset] < b'0' || bytes[offset] > b'9' {
        return None;
    }
    let mut val: u16 = 0;
    while offset < bytes.len() && bytes[offset] >= b'0' && bytes[offset] <= b'9' {
        val = val * 10 + (bytes[offset] - b'0') as u16;
        if val > 255 {
            return None; // overflow for u8
        }
        offset += 1;
    }
    Some((val as u8, offset))
}

/// Decode a micro-tier seed code from raw bytes (no_std, no heap).
///
/// Accepts formats:
/// - `"abc"` — base36 seed only, uses default C64 dimensions (64×48)
/// - `"abc-64x48"` — base36 seed with explicit dimensions
///
/// Case-insensitive. Returns error if seed > 0xFFFF or dimensions are
/// out of range.
pub const fn decode_micro_from_bytes(bytes: &[u8]) -> Result<MicroSeedParams, SeedDecodeError> {
    use crate::rules::balance;

    if bytes.is_empty() {
        return Err(SeedDecodeError::Empty);
    }

    // Find '-' separator
    let mut dash_pos: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'-' {
            dash_pos = Some(i);
            break;
        }
        i += 1;
    }

    // Decode seed part (everything before '-', or all bytes if no '-')
    let seed_end = match dash_pos {
        Some(pos) => pos,
        None => bytes.len(),
    };

    // Decode base36 seed using u16 arithmetic directly.
    // Avoids calling decode_from_bytes (u64) which pulls in __muldi3 (236B on 6502).
    if seed_end == 0 {
        return Err(SeedDecodeError::Empty);
    }

    let mut result: u16 = 0;
    let mut j = 0;
    while j < seed_end {
        let b = bytes[j];
        let digit: u16 = if b >= b'0' && b <= b'9' {
            (b - b'0') as u16
        } else if b >= b'a' && b <= b'z' {
            (b - b'a') as u16 + 10
        } else if b >= b'A' && b <= b'Z' {
            (b - b'A') as u16 + 10
        } else {
            return Err(SeedDecodeError::InvalidChar(b));
        };

        // Manual overflow guard instead of checked_mul/checked_add.
        // checked_mul(36) on u16 widens to u32 multiply (__mulsi3, 103B on 6502).
        // 1820 * 36 = 65520; anything above 1820 overflows u16 after * 36.
        if result > 1820 {
            return Err(SeedDecodeError::NotMicroTier);
        }
        result = result * 36;
        // Max after multiply: 65520. Max digit: 35. 65520 + 35 = 65555 > 65535.
        // So checked_add is still needed for the final digit.
        let Some(r) = result.checked_add(digit) else {
            return Err(SeedDecodeError::NotMicroTier);
        };
        result = r;
        j += 1;
    }

    let seed = result;

    // Parse optional dimensions suffix
    let (width, height) = match dash_pos {
        None => (balance::MICRO_MAP_WIDTH, balance::MICRO_MAP_HEIGHT),
        Some(pos) => {
            let suffix_start = pos + 1;
            if suffix_start >= bytes.len() {
                return Err(SeedDecodeError::Empty);
            }

            // Parse width
            let (w, after_w) = match parse_u8_decimal(bytes, suffix_start) {
                Some(v) => v,
                None => return Err(SeedDecodeError::InvalidDimensions),
            };

            // Expect 'x' or 'X'
            if after_w >= bytes.len() || (bytes[after_w] != b'x' && bytes[after_w] != b'X') {
                return Err(SeedDecodeError::InvalidDimensions);
            }

            // Parse height
            let (h, after_h) = match parse_u8_decimal(bytes, after_w + 1) {
                Some(v) => v,
                None => return Err(SeedDecodeError::InvalidDimensions),
            };

            // Nothing should follow after height
            if after_h != bytes.len() {
                return Err(SeedDecodeError::InvalidDimensions);
            }

            // Validate ranges
            if w < balance::MIN_MAP_WIDTH
                || h < balance::MIN_MAP_HEIGHT
                || w > balance::MICRO_MAX_MAP_WIDTH
                || h > balance::MICRO_MAX_MAP_HEIGHT
            {
                return Err(SeedDecodeError::InvalidDimensions);
            }

            (w, h)
        }
    };

    Ok(MicroSeedParams {
        seed,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_roundtrip(value: u64) {
        let mut buf = [0u8; MAX_BASE36_LEN];
        let len = encode_to_buf(value, &mut buf);
        let decoded = decode_from_bytes(&buf[..len]).unwrap();
        assert_eq!(decoded, value, "roundtrip failed for {value}");
    }

    #[test]
    fn roundtrip_zero() {
        encode_roundtrip(0);
    }

    #[test]
    fn roundtrip_one() {
        encode_roundtrip(1);
    }

    #[test]
    fn roundtrip_35() {
        // Last single-digit base36 value.
        encode_roundtrip(35);
    }

    #[test]
    fn roundtrip_36() {
        // First two-digit base36 value.
        encode_roundtrip(36);
    }

    #[test]
    fn roundtrip_micro_max() {
        encode_roundtrip(0xFFFF);
    }

    #[test]
    fn roundtrip_compact_max() {
        encode_roundtrip(0xFFFF_FFFF);
    }

    #[test]
    fn roundtrip_u64_max() {
        encode_roundtrip(u64::MAX);
    }

    #[test]
    fn encode_zero_is_single_zero() {
        let mut buf = [0u8; MAX_BASE36_LEN];
        let len = encode_to_buf(0, &mut buf);
        assert_eq!(&buf[..len], b"0");
    }

    #[test]
    fn encode_u64_max_length() {
        let mut buf = [0u8; MAX_BASE36_LEN];
        let len = encode_to_buf(u64::MAX, &mut buf);
        assert_eq!(len, 13);
        // Known value: 3w5e11264sgsf
        assert_eq!(&buf[..len], b"3w5e11264sgsf");
    }

    #[test]
    fn decode_empty_is_error() {
        assert_eq!(decode_from_bytes(b""), Err(SeedDecodeError::Empty));
    }

    #[test]
    fn decode_invalid_char() {
        assert_eq!(
            decode_from_bytes(b"abc!def"),
            Err(SeedDecodeError::InvalidChar(b'!'))
        );
    }

    #[test]
    fn decode_case_insensitive() {
        assert_eq!(decode_from_bytes(b"abc"), decode_from_bytes(b"ABC"));
        assert_eq!(decode_from_bytes(b"abc"), decode_from_bytes(b"AbC"));
    }

    #[test]
    fn decode_overflow() {
        // A very long base36 string that would overflow u64.
        assert_eq!(
            decode_from_bytes(b"zzzzzzzzzzzzzz"),
            Err(SeedDecodeError::Overflow)
        );
    }

    #[test]
    fn tier_micro_boundary() {
        assert_eq!(tier_from_seed(0), Tier::Micro);
        assert_eq!(tier_from_seed(0xFFFF), Tier::Micro);
    }

    #[test]
    fn tier_compact_boundary() {
        assert_eq!(tier_from_seed(0x1_0000), Tier::Compact);
        assert_eq!(tier_from_seed(0xFFFF_FFFF), Tier::Compact);
    }

    #[test]
    fn tier_standard_boundary() {
        assert_eq!(tier_from_seed(0x1_0000_0000), Tier::Standard);
        assert_eq!(tier_from_seed(u64::MAX), Tier::Standard);
    }

    // ── Micro seed code tests ───────────────────────────────────────

    fn micro_code(seed: u16, w: u8, h: u8) -> ([u8; MAX_MICRO_SEED_CODE_LEN], usize) {
        let mut buf = [0u8; MAX_MICRO_SEED_CODE_LEN];
        let len = encode_micro_to_buf(seed, w, h, &mut buf);
        (buf, len)
    }

    #[test]
    fn micro_code_known_value() {
        let (buf, len) = micro_code(42, 64, 48);
        assert_eq!(&buf[..len], b"16-64x48");
    }

    #[test]
    fn micro_code_seed_zero() {
        let (buf, len) = micro_code(0, 80, 40);
        assert_eq!(&buf[..len], b"0-80x40");
    }

    #[test]
    fn micro_code_max_seed() {
        let (buf, len) = micro_code(0xFFFF, 80, 60);
        assert_eq!(&buf[..len], b"1ekf-80x60");
    }

    #[test]
    fn micro_code_small_dims() {
        let (buf, len) = micro_code(1, 20, 15);
        assert_eq!(&buf[..len], b"1-20x15");
    }

    #[test]
    fn micro_code_fits_buffer() {
        // Worst case: 4-char seed + 3-digit dims.
        let (_, len) = micro_code(0xFFFF, 255, 255);
        assert!(len <= MAX_MICRO_SEED_CODE_LEN);
    }

    #[test]
    fn write_u8_decimal_values() {
        let mut buf = [0u8; 3];
        assert_eq!(write_u8_decimal(0, &mut buf, 0), 1);
        assert_eq!(buf[0], b'0');

        assert_eq!(write_u8_decimal(9, &mut buf, 0), 1);
        assert_eq!(buf[0], b'9');

        assert_eq!(write_u8_decimal(42, &mut buf, 0), 2);
        assert_eq!(&buf[..2], b"42");

        assert_eq!(write_u8_decimal(255, &mut buf, 0), 3);
        assert_eq!(&buf[..3], b"255");
    }

    // ── decode_micro_from_bytes tests ───────────────────────────────

    #[test]
    fn micro_decode_seed_only() {
        let p = decode_micro_from_bytes(b"16").unwrap();
        assert_eq!(p.seed, 42);
        assert_eq!(p.width, 64); // C64 default
        assert_eq!(p.height, 48);
    }

    #[test]
    fn micro_decode_with_dims() {
        let p = decode_micro_from_bytes(b"16-80x40").unwrap();
        assert_eq!(p.seed, 42);
        assert_eq!(p.width, 80);
        assert_eq!(p.height, 40);
    }

    #[test]
    fn micro_decode_roundtrip() {
        let (buf, len) = micro_code(42, 64, 48);
        let p = decode_micro_from_bytes(&buf[..len]).unwrap();
        assert_eq!(p.seed, 42);
        assert_eq!(p.width, 64);
        assert_eq!(p.height, 48);
    }

    #[test]
    fn micro_decode_roundtrip_max_seed() {
        let (buf, len) = micro_code(0xFFFF, 80, 60);
        let p = decode_micro_from_bytes(&buf[..len]).unwrap();
        assert_eq!(p.seed, 0xFFFF);
        assert_eq!(p.width, 80);
        assert_eq!(p.height, 60);
    }

    #[test]
    fn micro_decode_roundtrip_zero_seed() {
        let (buf, len) = micro_code(0, 64, 48);
        let p = decode_micro_from_bytes(&buf[..len]).unwrap();
        assert_eq!(p.seed, 0);
        assert_eq!(p.width, 64);
        assert_eq!(p.height, 48);
    }

    #[test]
    fn micro_decode_case_insensitive() {
        let p = decode_micro_from_bytes(b"1EKF-80X60").unwrap();
        assert_eq!(p.seed, 0xFFFF);
        assert_eq!(p.width, 80);
        assert_eq!(p.height, 60);
    }

    #[test]
    fn micro_decode_not_micro_tier() {
        // "10000" in base36 = 36^4 = 1679616 > 0xFFFF
        assert_eq!(
            decode_micro_from_bytes(b"10000"),
            Err(SeedDecodeError::NotMicroTier)
        );
    }

    #[test]
    fn micro_decode_empty() {
        assert_eq!(decode_micro_from_bytes(b""), Err(SeedDecodeError::Empty));
    }

    #[test]
    fn micro_decode_invalid_char() {
        assert_eq!(
            decode_micro_from_bytes(b"1!"),
            Err(SeedDecodeError::InvalidChar(b'!'))
        );
    }

    #[test]
    fn micro_decode_dims_too_small() {
        assert_eq!(
            decode_micro_from_bytes(b"16-10x10"),
            Err(SeedDecodeError::InvalidDimensions)
        );
    }

    #[test]
    fn micro_decode_dims_too_large() {
        assert_eq!(
            decode_micro_from_bytes(b"16-255x255"),
            Err(SeedDecodeError::InvalidDimensions)
        );
    }

    #[test]
    fn micro_decode_missing_height() {
        assert_eq!(
            decode_micro_from_bytes(b"16-64x"),
            Err(SeedDecodeError::InvalidDimensions)
        );
    }

    #[test]
    fn micro_decode_missing_x() {
        assert_eq!(
            decode_micro_from_bytes(b"16-64"),
            Err(SeedDecodeError::InvalidDimensions)
        );
    }

    #[test]
    fn micro_decode_trailing_junk() {
        assert_eq!(
            decode_micro_from_bytes(b"16-64x48z"),
            Err(SeedDecodeError::InvalidDimensions)
        );
    }

    #[test]
    fn micro_decode_min_valid_dims() {
        let p = decode_micro_from_bytes(b"16-20x15").unwrap();
        assert_eq!(p.width, 20);
        assert_eq!(p.height, 15);
    }

    #[test]
    fn micro_decode_max_valid_dims() {
        let p = decode_micro_from_bytes(b"16-80x60").unwrap();
        assert_eq!(p.width, 80);
        assert_eq!(p.height, 60);
    }
}
