// Tests for the algorithms used in the C64 builtin overrides.
//
// The actual overrides are 6502 assembly in crates/c64/src/c64.rs. These tests
// verify the ALGORITHM is correct by running equivalent Rust implementations
// against the standard library. If the algorithm matches std for all edge cases,
// and the assembly faithfully implements the algorithm, the overrides are correct.

#[cfg(test)]
mod tests {
    // --- memcpy: page-based forward copy ---
    fn our_memcpy(dest: &mut [u8], src: &[u8], n: usize) {
        let pages = n >> 8;
        let remainder = n & 0xFF;
        let mut di = 0usize;
        for _ in 0..pages {
            for _ in 0..256 {
                dest[di] = src[di];
                di += 1;
            }
        }
        for _ in 0..remainder {
            dest[di] = src[di];
            di += 1;
        }
    }

    // --- memmove: overlap-aware copy ---
    // Operates on a SINGLE buffer (simulating real memmove with overlapping regions).
    fn our_memmove_buf(buf: &mut [u8], dst_off: usize, src_off: usize, n: usize) {
        if n == 0 {
            return;
        }
        let delta = dst_off.wrapping_sub(src_off);
        if delta >= n {
            for i in 0..n {
                buf[dst_off + i] = buf[src_off + i];
            }
        } else {
            let mut i = n;
            while i > 0 {
                i -= 1;
                buf[dst_off + i] = buf[src_off + i];
            }
        }
    }

    // --- memcmp: byte-by-byte comparison ---
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

    // --- mulhi3: shift-and-add u16 multiply ---
    fn our_mulhi3(a: u16, b: u16) -> u16 {
        let mut result: u16 = 0;
        let mut multiplicand = a;
        let mut multiplier = b;
        while multiplier != 0 {
            if multiplier & 1 != 0 {
                result = result.wrapping_add(multiplicand);
            }
            multiplicand = multiplicand.wrapping_shl(1);
            multiplier >>= 1;
        }
        result
    }

    // --- ashlqi3: u8 left shift ---
    fn our_ashlqi3(val: u8, count: u8) -> u8 {
        if count == 0 {
            return val;
        }
        let mut v = val;
        for _ in 0..count {
            v <<= 1;
        }
        v
    }

    // --- Division overrides ---
    fn our_udivqi3(a: u8, b: u8) -> u8 {
        if b == 0 {
            return 0;
        }
        let mut q: u8 = 0;
        let mut r = a;
        while r >= b {
            r -= b;
            q += 1;
        }
        q
    }

    fn our_umodqi3(a: u8, b: u8) -> u8 {
        if b == 0 {
            return 0;
        }
        let mut r = a;
        while r >= b {
            r -= b;
        }
        r
    }

    fn our_udivmodhi4(a: u16, b: u16) -> (u16, u16) {
        if b == 0 {
            return (0, 0);
        }
        let mut q: u16 = 0;
        let mut r = a;
        while r >= b {
            r -= b;
            q += 1;
        }
        (q, r)
    }

    fn our_udivhi3(a: u16, b: u16) -> u16 {
        our_udivmodhi4(a, b).0
    }
    fn our_umodhi3(a: u16, b: u16) -> u16 {
        our_udivmodhi4(a, b).1
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
    fn memcpy_256_exact_page() {
        let src: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 256];
        our_memcpy(&mut dst, &src, 256);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_257_page_plus_one() {
        let src: Vec<u8> = (0..257).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 257];
        our_memcpy(&mut dst, &src, 257);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_512_two_pages() {
        let src: Vec<u8> = (0..512).map(|i| (i & 0xFF) as u8).collect();
        let mut dst = vec![0u8; 512];
        our_memcpy(&mut dst, &src, 512);
        assert_eq!(dst, src);
    }

    #[test]
    fn memcpy_matches_std() {
        // Compare against standard copy for various sizes
        for &n in &[0, 1, 127, 128, 255, 256, 257, 511, 512, 1000] {
            let src: Vec<u8> = (0..n)
                .map(|i: usize| (i.wrapping_mul(37) & 0xFF) as u8)
                .collect();
            let mut dst_ours = vec![0u8; n];
            let mut dst_std = vec![0u8; n];
            our_memcpy(&mut dst_ours, &src, n);
            dst_std[..n].copy_from_slice(&src[..n]);
            assert_eq!(dst_ours, dst_std, "memcpy mismatch at n={n}");
        }
    }

    // ======================================================================
    // memmove tests (using single-buffer overlap simulation)
    // ======================================================================

    #[test]
    fn memmove_non_overlapping() {
        let mut buf = [0u8; 10];
        buf[5..10].copy_from_slice(&[1, 2, 3, 4, 5]);
        our_memmove_buf(&mut buf, 0, 5, 5);
        assert_eq!(&buf[0..5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn memmove_overlap_dst_after_src() {
        // src=0..5, dst=2..7: overlapping, needs backward copy
        let mut buf = [1, 2, 3, 4, 5, 0, 0];
        our_memmove_buf(&mut buf, 2, 0, 5);
        assert_eq!(buf, [1, 2, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn memmove_overlap_dst_before_src() {
        // src=2..7, dst=0..5: overlapping, forward copy is safe
        let mut buf = [0, 0, 1, 2, 3, 4, 5];
        our_memmove_buf(&mut buf, 0, 2, 5);
        assert_eq!(buf, [1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn memmove_same_address() {
        let mut buf = [1, 2, 3, 4, 5];
        our_memmove_buf(&mut buf, 0, 0, 5);
        assert_eq!(buf, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn memmove_zero_length() {
        let mut buf = [1, 2, 3];
        our_memmove_buf(&mut buf, 0, 1, 0);
        assert_eq!(buf, [1, 2, 3]);
    }

    #[test]
    fn memmove_one_byte_overlap() {
        let mut buf = [10, 20];
        our_memmove_buf(&mut buf, 1, 0, 1);
        assert_eq!(buf, [10, 10]);
    }

    #[test]
    fn memmove_overlap_direction_matches_std() {
        // Test the overlap direction detection logic against std memmove behavior.
        // On MOS (u16 pointers), delta = dest.wrapping_sub(src).
        // forward safe when delta >= n, backward when delta < n.
        for &(dst, src, n) in &[
            (0usize, 0usize, 5usize), // same address
            (2, 0, 5),                // dst > src, overlap
            (0, 2, 5),                // dst < src, wrapping delta is large
            (100, 0, 5),              // no overlap
            (0, 100, 5),              // no overlap, wrapping
            (1, 0, 1),                // adjacent, overlap by 1
        ] {
            let delta = dst.wrapping_sub(src);
            let need_backward = delta < n;
            // Verify: if dst > src and the gap is less than n, we need backward
            if dst > src && dst < src + n {
                assert!(
                    need_backward,
                    "should need backward for dst={dst} src={src} n={n}"
                );
            }
        }
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
    fn memcmp_exact_difference_values() {
        assert_eq!(our_memcmp(&[0], &[1], 1), -1);
        assert_eq!(our_memcmp(&[255], &[0], 1), 255);
        assert_eq!(our_memcmp(&[0], &[255], 1), -255);
        assert_eq!(our_memcmp(&[100], &[200], 1), -100);
    }

    #[test]
    fn memcmp_page_boundary_difference() {
        // Difference at byte 256 — exactly where the page-based loop transitions
        let mut a = vec![0x42u8; 257];
        let mut b = vec![0x42u8; 257];
        b[256] = 0x43; // differ at first byte of second page
        assert!(our_memcmp(&a, &b, 257) < 0);
        a[256] = 0x44;
        assert!(our_memcmp(&a, &b, 257) > 0);
    }

    #[test]
    fn memcmp_large_equal() {
        let a = vec![0xABu8; 513];
        let b = vec![0xABu8; 513];
        assert_eq!(our_memcmp(&a, &b, 513), 0);
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
    fn memset_fill_zero() {
        let mut buf = vec![0xFFu8; 300];
        our_memset(&mut buf, 0x00, 300);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn memset_fill_0xff() {
        let mut buf = vec![0u8; 300];
        our_memset(&mut buf, 0xFF, 300);
        assert!(buf.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn memset_255_bytes() {
        let mut buf = vec![0u8; 255];
        our_memset(&mut buf, 0x42, 255);
        assert!(buf.iter().all(|&b| b == 0x42));
    }

    #[test]
    fn memset_256_exact_page() {
        let mut buf = vec![0u8; 256];
        our_memset(&mut buf, 0xFF, 256);
        assert!(buf.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn memset_257_page_plus_one() {
        let mut buf = vec![0u8; 257];
        our_memset(&mut buf, 0x55, 257);
        assert!(buf.iter().all(|&b| b == 0x55));
    }

    #[test]
    fn memset_1000_clear_screen_size() {
        // clear_screen fills 1000 bytes — verify the 3*256+232 decomposition
        assert_eq!(3 * 256 + 232, 1000);
        let mut buf = vec![0u8; 1000];
        our_memset(&mut buf, 0x20, 1000);
        assert!(buf.iter().all(|&b| b == 0x20));
    }

    // ======================================================================
    // Division override tests
    // ======================================================================

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
        assert_eq!(our_umodqi3(10, 0), 0);
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
    fn divmod_u16_max_values() {
        assert_eq!(our_udivmodhi4(65535, 1), (65535, 0));
        assert_eq!(our_udivmodhi4(65535, 65535), (1, 0));
        assert_eq!(our_udivmodhi4(65535, 65534), (1, 1));
        assert_eq!(our_udivmodhi4(1, 65535), (0, 1));
    }

    #[test]
    fn divmod_u16_zero_divisor() {
        assert_eq!(our_udivmodhi4(100, 0), (0, 0));
        assert_eq!(our_udivhi3(100, 0), 0);
        assert_eq!(our_umodhi3(100, 0), 0);
    }

    #[test]
    fn div_u16_matches_native() {
        let test_values: &[u16] = &[0, 1, 2, 9, 10, 11, 99, 100, 255, 256, 1000, 10000, 65535];
        let divisors: &[u16] = &[1, 2, 3, 5, 10, 36, 100, 255, 256, 1000, 65535];
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

    // ======================================================================
    // Multiply and shift override tests
    // ======================================================================

    #[test]
    fn mulhi3_basic() {
        assert_eq!(our_mulhi3(0, 0), 0);
        assert_eq!(our_mulhi3(1, 1), 1);
        assert_eq!(our_mulhi3(7, 6), 42);
        assert_eq!(our_mulhi3(100, 100), 10000);
        assert_eq!(our_mulhi3(256, 256), 0); // overflow wraps
    }

    #[test]
    fn mulhi3_commutative() {
        for &(a, b) in &[(3u16, 7), (100, 200), (255, 2), (0, 65535), (1, 65535)] {
            assert_eq!(
                our_mulhi3(a, b),
                our_mulhi3(b, a),
                "mulhi3 not commutative for ({a}, {b})"
            );
        }
    }

    #[test]
    fn mulhi3_matches_native() {
        let values: &[u16] = &[0, 1, 2, 3, 5, 10, 40, 100, 255, 256, 1000];
        for &a in values {
            for &b in values {
                assert_eq!(our_mulhi3(a, b), a.wrapping_mul(b), "mulhi3({a}, {b})");
            }
        }
    }

    #[test]
    fn mulhi3_identity_and_zero() {
        for v in 0..=255u16 {
            assert_eq!(our_mulhi3(v, 0), 0);
            assert_eq!(our_mulhi3(0, v), 0);
            assert_eq!(our_mulhi3(v, 1), v);
            assert_eq!(our_mulhi3(1, v), v);
        }
    }

    #[test]
    fn ashlqi3_basic() {
        assert_eq!(our_ashlqi3(1, 0), 1);
        assert_eq!(our_ashlqi3(1, 1), 2);
        assert_eq!(our_ashlqi3(1, 7), 128);
        assert_eq!(our_ashlqi3(0xFF, 1), 0xFE);
        assert_eq!(our_ashlqi3(0x80, 1), 0); // overflow
    }

    #[test]
    fn ashlqi3_matches_native() {
        for val in 0..=255u8 {
            for count in 0..8u8 {
                assert_eq!(
                    our_ashlqi3(val, count),
                    val << count,
                    "ashlqi3({val}, {count})"
                );
            }
        }
    }

    // ======================================================================
    // VIC-II constant tests
    // ======================================================================

    const SCREEN_BASE: usize = 0x0400;
    const COLOR_BASE: usize = 0xD800;

    fn row_addr(y: u8) -> u16 {
        SCREEN_BASE as u16 + (y as u16) * 40
    }

    fn to_screen_code(ascii: u8) -> u8 {
        match ascii {
            b'@' => 0,
            b'A'..=b'Z' => ascii - 64,
            b'a'..=b'z' => ascii - 96,
            _ => ascii,
        }
    }

    #[test]
    fn row_table_values() {
        let expected_lo: [u8; 25] = [
            0x00, 0x28, 0x50, 0x78, 0xA0, 0xC8, 0xF0, 0x18, 0x40, 0x68, 0x90, 0xB8, 0xE0, 0x08,
            0x30, 0x58, 0x80, 0xA8, 0xD0, 0xF8, 0x20, 0x48, 0x70, 0x98, 0xC0,
        ];
        let expected_hi: [u8; 25] = [
            0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x06,
            0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x07, 0x07, 0x07, 0x07,
        ];
        for y in 0..25u8 {
            let addr = row_addr(y);
            assert_eq!(addr & 0xFF, expected_lo[y as usize] as u16, "ROW_LO[{y}]");
            assert_eq!(addr >> 8, expected_hi[y as usize] as u16, "ROW_HI[{y}]");
        }
    }

    #[test]
    fn color_ram_offset() {
        // Color RAM = screen addr high byte + $D4
        for y in 0..25u8 {
            let scr = row_addr(y);
            let clr_hi = (scr >> 8) as u8 + 0xD4;
            let expected_hi = ((COLOR_BASE + (y as usize) * 40) >> 8) as u8;
            assert_eq!(clr_hi, expected_hi, "color hi mismatch at row {y}");
        }
    }

    #[test]
    fn screen_code_conversion() {
        assert_eq!(to_screen_code(b'@'), 0);
        assert_eq!(to_screen_code(b'A'), 1);
        assert_eq!(to_screen_code(b'Z'), 26);
        assert_eq!(to_screen_code(b'a'), 1);
        assert_eq!(to_screen_code(b'z'), 26);
        // Pass-through
        assert_eq!(to_screen_code(b' '), b' ');
        for ch in b'0'..=b'9' {
            assert_eq!(to_screen_code(ch), ch);
        }
        for &ch in &[b'!', b'#', b'-', b':', b'/'] {
            assert_eq!(to_screen_code(ch), ch);
        }
    }

    #[test]
    fn screen_code_boundary_values() {
        // Test boundary bytes around the converted ranges
        assert_eq!(to_screen_code(b'@'), 0); // 0x40 → 0
        assert_eq!(to_screen_code(b'A'), 1); // 0x41 → 1
        assert_eq!(to_screen_code(b'Z'), 26); // 0x5A → 26
        assert_eq!(to_screen_code(b'['), b'['); // 0x5B → passthrough
        assert_eq!(to_screen_code(b'`'), b'`'); // 0x60 → passthrough
        assert_eq!(to_screen_code(b'a'), 1); // 0x61 → 1
        assert_eq!(to_screen_code(b'z'), 26); // 0x7A → 26
        assert_eq!(to_screen_code(b'{'), b'{'); // 0x7B → passthrough
        assert_eq!(to_screen_code(b'?'), b'?'); // 0x3F → passthrough (just before @)
    }

    #[test]
    fn draw_text_screen_codes() {
        let text = b"Hello";
        let codes: Vec<u8> = text.iter().map(|&ch| to_screen_code(ch)).collect();
        assert_eq!(codes, vec![8, 5, 12, 12, 15]);
    }
}
