// Tests for the page-based memory algorithms used in the C64 builtin overrides.
//
// The actual overrides are 6502 assembly in crates/c64/src/c64.rs. These tests
// verify the ALGORITHM is correct by running equivalent Rust implementations
// against the standard library. If the algorithm matches std for all edge cases,
// and the assembly faithfully implements the algorithm, the overrides are correct.

#[cfg(test)]
mod tests {
    // --- memcpy: page-based forward copy ---
    // Matches the assembly: copy full pages (256 bytes) using inner loop,
    // then copy remaining bytes.
    fn our_memcpy(dest: &mut [u8], src: &[u8], n: usize) {
        if n == 0 {
            return;
        }
        let pages = n >> 8; // n / 256
        let remainder = n & 0xFF; // n % 256
        let mut di = 0usize;

        // Copy full pages
        for _ in 0..pages {
            for _ in 0..256 {
                dest[di] = src[di];
                di += 1;
            }
        }
        // Copy remainder
        for _ in 0..remainder {
            dest[di] = src[di];
            di += 1;
        }
    }

    // --- memmove: overlap-aware copy ---
    // If dest - src >= n (unsigned), copy forward. Otherwise copy backward.
    fn our_memmove(dest: &mut [u8], src: &[u8], n: usize, dest_addr: usize, src_addr: usize) {
        if n == 0 {
            return;
        }
        let delta = dest_addr.wrapping_sub(src_addr);
        if delta >= n {
            // Forward copy (same as memcpy)
            for i in 0..n {
                dest[i] = src[i];
            }
        } else {
            // Backward copy
            let mut i = n;
            while i > 0 {
                i -= 1;
                dest[i] = src[i];
            }
        }
    }

    // --- memcmp: byte-by-byte comparison ---
    // Returns first non-equal difference as i16 (a - b), or 0 if equal.
    fn our_memcmp(s1: &[u8], s2: &[u8], n: usize) -> i16 {
        for i in 0..n {
            if s1[i] != s2[i] {
                return s1[i] as i16 - s2[i] as i16;
            }
        }
        0
    }

    // --- memset: page-based fill ---
    fn our_memset(dest: &mut [u8], val: u8, n: usize) {
        if n == 0 {
            return;
        }
        let pages = n >> 8;
        let remainder = n & 0xFF;
        let mut di = 0usize;

        for _ in 0..pages {
            for _ in 0..256 {
                dest[di] = val;
                di += 1;
            }
        }
        for _ in 0..remainder {
            dest[di] = val;
            di += 1;
        }
    }

    // ======================================================================
    // memcpy tests
    // ======================================================================

    #[test]
    fn memcpy_zero_length() {
        let src = [1, 2, 3];
        let mut dst = [0u8; 3];
        our_memcpy(&mut dst, &src, 0);
        assert_eq!(dst, [0, 0, 0]);
    }

    #[test]
    fn memcpy_one_byte() {
        let src = [42];
        let mut dst = [0u8; 1];
        our_memcpy(&mut dst, &src, 1);
        assert_eq!(dst, [42]);
    }

    #[test]
    fn memcpy_small() {
        let src: Vec<u8> = (0..13).collect();
        let mut dst = vec![0u8; 13];
        our_memcpy(&mut dst, &src, 13);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_255_bytes() {
        let src: Vec<u8> = (0..255).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 255];
        our_memcpy(&mut dst, &src, 255);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_256_bytes_exact_page() {
        let src: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 256];
        our_memcpy(&mut dst, &src, 256);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_257_bytes_page_plus_one() {
        let src: Vec<u8> = (0..257).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 257];
        our_memcpy(&mut dst, &src, 257);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_512_bytes_two_pages() {
        let src: Vec<u8> = (0..512).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 512];
        our_memcpy(&mut dst, &src, 512);
        assert_eq!(dst, src);
    }

    // ======================================================================
    // memmove tests
    // ======================================================================

    #[test]
    fn memmove_non_overlapping() {
        let src = [1, 2, 3, 4, 5];
        let mut dst = [0u8; 5];
        our_memmove(&mut dst, &src, 5, 100, 0);
        assert_eq!(dst, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn memmove_overlap_forward() {
        // src=0..5, dest=2..7: dest > src, overlap, needs backward copy
        let mut buf = [1, 2, 3, 4, 5, 0, 0];
        let n = 5;
        let src_addr = 0usize;
        let dst_addr = 2usize;
        // Simulate overlap by using the same buffer
        let delta = dst_addr.wrapping_sub(src_addr); // 2
        assert!(delta < n); // overlapping → backward copy
        // Backward copy from buf[0..5] to buf[2..7]
        let mut i = n;
        while i > 0 {
            i -= 1;
            buf[dst_addr + i] = buf[src_addr + i];
        }
        assert_eq!(buf, [1, 2, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn memmove_overlap_backward() {
        // src=2..7, dest=0..5: dest < src, delta wraps large, forward copy is safe
        let mut buf = [0, 0, 1, 2, 3, 4, 5];
        let n = 5;
        let src_addr = 2usize;
        let dst_addr = 0usize;
        let delta = dst_addr.wrapping_sub(src_addr); // wraps to large value
        assert!(delta >= n); // no overlap concern → forward copy
        for i in 0..n {
            buf[dst_addr + i] = buf[src_addr + i];
        }
        assert_eq!(buf, [1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn memmove_zero_length() {
        let src = [1, 2, 3];
        let mut dst = [0u8; 3];
        our_memmove(&mut dst, &src, 0, 100, 0);
        assert_eq!(dst, [0, 0, 0]);
    }

    #[test]
    fn memmove_same_address() {
        // delta = 0, which is >= n=0 only if n=0. For n>0, delta=0 < n, backward copy.
        // Backward copy of same region is a no-op.
        let mut buf = [1, 2, 3, 4, 5];
        let original = buf;
        our_memmove(&mut buf, &original, 5, 0, 0);
        assert_eq!(buf, original);
    }

    // ======================================================================
    // memcmp tests
    // ======================================================================

    #[test]
    fn memcmp_equal() {
        assert_eq!(our_memcmp(&[1, 2, 3], &[1, 2, 3], 3), 0);
    }

    #[test]
    fn memcmp_first_byte_differs() {
        assert!(our_memcmp(&[1, 2, 3], &[2, 2, 3], 3) < 0);
        assert!(our_memcmp(&[3, 2, 3], &[2, 2, 3], 3) > 0);
    }

    #[test]
    fn memcmp_last_byte_differs() {
        assert!(our_memcmp(&[1, 2, 3], &[1, 2, 4], 3) < 0);
        assert!(our_memcmp(&[1, 2, 5], &[1, 2, 4], 3) > 0);
    }

    #[test]
    fn memcmp_zero_length() {
        assert_eq!(our_memcmp(&[1], &[2], 0), 0);
    }

    #[test]
    fn memcmp_one_byte() {
        assert_eq!(our_memcmp(&[42], &[42], 1), 0);
        assert!(our_memcmp(&[0], &[255], 1) < 0);
        assert!(our_memcmp(&[255], &[0], 1) > 0);
    }

    #[test]
    fn memcmp_returns_correct_sign() {
        // Must return a - b for first differing byte (as i16 to handle full u8 range)
        assert_eq!(our_memcmp(&[0], &[1], 1), -1);
        assert_eq!(our_memcmp(&[255], &[0], 1), 255);
        assert_eq!(our_memcmp(&[0], &[255], 1), -255);
    }

    // ======================================================================
    // memset tests
    // ======================================================================

    #[test]
    fn memset_zero_length() {
        let mut buf = [1, 2, 3];
        our_memset(&mut buf, 0, 0);
        assert_eq!(buf, [1, 2, 3]);
    }

    #[test]
    fn memset_one_byte() {
        let mut buf = [0u8; 1];
        our_memset(&mut buf, 0xAA, 1);
        assert_eq!(buf, [0xAA]);
    }

    #[test]
    fn memset_255_bytes() {
        let mut buf = vec![0u8; 255];
        our_memset(&mut buf, 0x42, 255);
        assert!(buf.iter().all(|&b| b == 0x42));
    }

    #[test]
    fn memset_256_bytes_exact_page() {
        let mut buf = vec![0u8; 256];
        our_memset(&mut buf, 0xFF, 256);
        assert!(buf.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn memset_257_bytes_page_plus_one() {
        let mut buf = vec![0u8; 257];
        our_memset(&mut buf, 0x55, 257);
        assert!(buf.iter().all(|&b| b == 0x55));
    }

    #[test]
    fn memset_512_bytes_two_pages() {
        let mut buf = vec![0u8; 512];
        our_memset(&mut buf, 0xBE, 512);
        assert!(buf.iter().all(|&b| b == 0xBE));
    }

    // ======================================================================
    // Division override algorithm tests
    // ======================================================================

    fn our_udivqi3(a: u8, b: u8) -> u8 {
        if b == 0 { return 0; }
        let mut q: u8 = 0;
        let mut r = a;
        while r >= b { r -= b; q += 1; }
        q
    }

    fn our_umodqi3(a: u8, b: u8) -> u8 {
        if b == 0 { return 0; }
        let mut r = a;
        while r >= b { r -= b; }
        r
    }

    fn our_udivmodhi4(a: u16, b: u16) -> (u16, u16) {
        if b == 0 { return (0, 0); }
        let mut q: u16 = 0;
        let mut r = a;
        while r >= b { r -= b; q += 1; }
        (q, r)
    }

    fn our_udivhi3(a: u16, b: u16) -> u16 {
        our_udivmodhi4(a, b).0
    }

    fn our_umodhi3(a: u16, b: u16) -> u16 {
        our_udivmodhi4(a, b).1
    }

    #[test]
    fn div_u8_basic() {
        assert_eq!(our_udivqi3(10, 3), 3);
        assert_eq!(our_udivqi3(255, 1), 255);
        assert_eq!(our_udivqi3(0, 5), 0);
        assert_eq!(our_udivqi3(7, 7), 1);
        assert_eq!(our_udivqi3(6, 7), 0);
    }

    #[test]
    fn div_u8_zero_divisor() {
        assert_eq!(our_udivqi3(10, 0), 0);
    }

    #[test]
    fn mod_u8_basic() {
        assert_eq!(our_umodqi3(10, 3), 1);
        assert_eq!(our_umodqi3(255, 10), 5);
        assert_eq!(our_umodqi3(0, 5), 0);
        assert_eq!(our_umodqi3(7, 7), 0);
        assert_eq!(our_umodqi3(6, 7), 6);
    }

    #[test]
    fn div_u8_exhaustive() {
        for a in 0..=255u8 {
            for b in 1..=255u8 {
                assert_eq!(our_udivqi3(a, b), a / b, "udivqi3({a}, {b})");
                assert_eq!(our_umodqi3(a, b), a % b, "umodqi3({a}, {b})");
            }
        }
    }

    #[test]
    fn divmod_u16_basic() {
        assert_eq!(our_udivmodhi4(1000, 10), (100, 0));
        assert_eq!(our_udivmodhi4(65535, 36), (1820, 15));
        assert_eq!(our_udivmodhi4(0, 100), (0, 0));
        assert_eq!(our_udivmodhi4(99, 100), (0, 99));
    }

    #[test]
    fn divmod_u16_zero_divisor() {
        assert_eq!(our_udivmodhi4(100, 0), (0, 0));
    }

    #[test]
    fn div_u16_matches_native() {
        let test_values: &[u16] = &[0, 1, 2, 9, 10, 11, 99, 100, 255, 256, 1000, 10000, 65535];
        let divisors: &[u16] = &[1, 2, 3, 5, 10, 36, 100, 255, 256, 1000];
        for &a in test_values {
            for &b in divisors {
                assert_eq!(our_udivhi3(a, b), a / b, "udivhi3({a}, {b})");
                assert_eq!(our_umodhi3(a, b), a % b, "umodhi3({a}, {b})");
                let (q, r) = our_udivmodhi4(a, b);
                assert_eq!(q, a / b, "udivmodhi4 quotient({a}, {b})");
                assert_eq!(r, a % b, "udivmodhi4 remainder({a}, {b})");
            }
        }
    }
}
