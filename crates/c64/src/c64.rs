// C64 hardware register definitions and low-level helpers.
//
// All access to VIC-II, SID, CIA, and memory-mapped I/O goes through this
// module. Functions are #[inline(always)] so the compiler can emit direct
// LDA/STA instructions instead of JSR overhead.

#![allow(dead_code)] // Hardware registers are defined for completeness

use core::ptr::{read_volatile, write_volatile};

// --- Screen memory ---
pub const SCREEN: *mut u8 = 0x0400 as *mut u8;
pub const COLOR_RAM: *mut u8 = 0xD800 as *mut u8;
pub const SCREEN_WIDTH: u8 = 40;
pub const SCREEN_HEIGHT: u8 = 25;

// --- VIC-II registers ---
pub const VIC_BORDER: *mut u8 = 0xD020 as *mut u8;
pub const VIC_BG: *mut u8 = 0xD021 as *mut u8;
pub const VIC_RASTER: *const u8 = 0xD012 as *const u8;
pub const VIC_CTRL1: *mut u8 = 0xD011 as *mut u8;

// --- CIA 1 (keyboard + joystick) ---
pub const CIA1_PA: *mut u8 = 0xDC00 as *mut u8;   // port A: keyboard col / joy2
pub const CIA1_PB: *const u8 = 0xDC01 as *const u8; // port B: keyboard row / joy1
pub const CIA1_TIMER_LO: *const u8 = 0xDC04 as *const u8;
pub const CIA1_ICR: *mut u8 = 0xDC0D as *mut u8;  // interrupt control register
pub const CIA2_ICR: *mut u8 = 0xDD0D as *mut u8;
pub const VIC_IRQ_MASK: *mut u8 = 0xD01A as *mut u8;

// --- 6510 processor port ---
pub const CPU_PORT: *mut u8 = 0x01 as *mut u8;

// --- SID registers ---
pub const SID_BASE: *mut u8 = 0xD400 as *mut u8;

// --- C64 color palette (4-bit values for color RAM) ---
pub const COLOR_BLACK: u8 = 0;
pub const COLOR_WHITE: u8 = 1;
pub const COLOR_RED: u8 = 2;
pub const COLOR_CYAN: u8 = 3;
pub const COLOR_PURPLE: u8 = 4;
pub const COLOR_GREEN: u8 = 5;
pub const COLOR_BLUE: u8 = 6;
pub const COLOR_YELLOW: u8 = 7;
pub const COLOR_ORANGE: u8 = 8;
pub const COLOR_BROWN: u8 = 9;
pub const COLOR_LRED: u8 = 10;
pub const COLOR_DGREY: u8 = 11;
pub const COLOR_GREY: u8 = 12;
pub const COLOR_LGREEN: u8 = 13;
pub const COLOR_LBLUE: u8 = 14;
pub const COLOR_LGREY: u8 = 15;

// --- Low-level memory access ---

#[inline(always)]
pub fn poke(addr: *mut u8, val: u8) {
    unsafe { write_volatile(addr, val); }
}

#[inline(always)]
pub fn peek(addr: *const u8) -> u8 {
    unsafe { read_volatile(addr) }
}

// --- Screen helpers ---

/// Convert ASCII byte to C64 screen code.
#[inline(always)]
pub fn to_screen_code(ascii: u8) -> u8 {
    match ascii {
        b'@' => 0,
        b'A'..=b'Z' => ascii - 64,
        b'a'..=b'z' => ascii - 96,
        _ => ascii,
    }
}

/// Write an ASCII string to screen memory with color.
pub fn draw_text(x: u8, y: u8, text: &[u8], color: u8) {
    let base = (y as usize) * 40 + (x as usize);
    for (i, &ch) in text.iter().enumerate() {
        if (x as usize) + i >= 40 { break; }
        unsafe {
            write_volatile(SCREEN.add(base + i), to_screen_code(ch));
            write_volatile(COLOR_RAM.add(base + i), color);
        }
    }
}

/// Write a single character (screen code) with color at position.
#[inline(always)]
pub fn draw_char(x: u8, y: u8, sc: u8, color: u8) {
    let offset = (y as usize) * 40 + (x as usize);
    unsafe {
        write_volatile(SCREEN.add(offset), sc);
        write_volatile(COLOR_RAM.add(offset), color);
    }
}

/// Fill a screen row with a character and color.
pub fn fill_row(y: u8, sc: u8, color: u8) {
    let base = (y as usize) * 40;
    for i in 0..40 {
        unsafe {
            write_volatile(SCREEN.add(base + i), sc);
            write_volatile(COLOR_RAM.add(base + i), color);
        }
    }
}

/// Clear entire screen (all 1000 cells).
pub fn clear_screen() {
    for i in 0..1000usize {
        unsafe {
            write_volatile(SCREEN.add(i), 0x20); // space
            write_volatile(COLOR_RAM.add(i), COLOR_LGREY);
        }
    }
}

/// Wait for vertical blank (raster line 251+). Useful for timing and
/// avoiding screen tearing during bulk writes.
pub fn wait_vblank() {
    unsafe {
        while read_volatile(VIC_RASTER) != 251 {}
    }
}

/// Wait for the next video frame. Two-phase wait ensures exactly one
/// frame per call: first exit the current vblank (if in one), then
/// wait for the next vblank to arrive. Provides ~50 Hz (PAL) or
/// ~60 Hz (NTSC) timing for input polling and repeat counters.
pub fn wait_next_frame() {
    unsafe {
        while read_volatile(VIC_RASTER) == 251 {}
        while read_volatile(VIC_RASTER) != 251 {}
    }
}

// --- CIA data direction registers ---
pub const CIA1_DDRA: *mut u8 = 0xDC02 as *mut u8; // Port A direction (keyboard cols)
pub const CIA1_DDRB: *mut u8 = 0xDC03 as *mut u8; // Port B direction (keyboard rows)

/// Initialize C64 hardware for game use.
///
/// Unmaps KERNAL ROM to free $E000-$FFFF as RAM (~8 KB). Since we lose
/// the KERNAL IRQ handler (which provided keyboard buffer scanning), we
/// disable all interrupts and scan the keyboard matrix directly.
/// KERNAL can be temporarily re-mapped for disk I/O (save games) by
/// setting CPU port back to $37.
pub fn init_hardware() {
    // Set up CIA1 data direction registers for keyboard/joystick scanning.
    poke(CIA1_DDRA, 0xFF); // Port A = all output (keyboard columns)
    poke(CIA1_DDRB, 0x00); // Port B = all input  (keyboard rows)

    // --- Unmap KERNAL ROM ---
    // 1. Disable all interrupt sources
    poke(CIA1_ICR, 0x7F);    // disable all CIA1 interrupts
    poke(CIA2_ICR, 0x7F);    // disable all CIA2 interrupts
    poke(VIC_IRQ_MASK, 0x00); // disable VIC-II raster IRQ
    let _ = peek(CIA1_ICR as *const u8); // acknowledge pending
    let _ = peek(CIA2_ICR as *const u8);

    // 2. Write RTI opcode + interrupt vectors to RAM under KERNAL.
    //    While ROM is mapped, writes go to underlying RAM.
    const RTI_ADDR: u16 = 0xE000;
    const RTI_OPCODE: u8 = 0x40;
    unsafe {
        write_volatile(RTI_ADDR as *mut u8, RTI_OPCODE);
        // NMI vector ($FFFA) — RESTORE key triggers NMI, must be valid
        write_volatile(0xFFFA as *mut u8, (RTI_ADDR & 0xFF) as u8);
        write_volatile(0xFFFB as *mut u8, (RTI_ADDR >> 8) as u8);
        // RESET vector ($FFFC)
        write_volatile(0xFFFC as *mut u8, (RTI_ADDR & 0xFF) as u8);
        write_volatile(0xFFFD as *mut u8, (RTI_ADDR >> 8) as u8);
        // IRQ vector ($FFFE)
        write_volatile(0xFFFE as *mut u8, (RTI_ADDR & 0xFF) as u8);
        write_volatile(0xFFFF as *mut u8, (RTI_ADDR >> 8) as u8);
    }

    // 3. Unmap KERNAL + BASIC, keep I/O: CPU port $35
    //    LORAM=1, HIRAM=0, CHAREN=1
    //    → BASIC off (needs both LORAM+HIRAM), KERNAL off (needs HIRAM),
    //      I/O visible at $D000-$DFFF (needs CHAREN + at least one of LORAM/HIRAM),
    //      RAM at $E000-$FFFF
    //    NOTE: $3C (LORAM=0, HIRAM=0) would unmap I/O too — the PLA only
    //    enables I/O when at least one ROM select bit is set.
    poke(CPU_PORT, 0x35);

    // 4. Relocate soft stack to freed KERNAL region ($E000-$FFF7).
    //    MUST happen AFTER KERNAL is unmapped — otherwise reads from the
    //    stack area would return KERNAL ROM data instead of RAM.
    //    CRT init sets rc0:rc1 ($02:$03) = $D000. We move it to $FFF8
    //    so the stack grows down into the 8 KB KERNAL region.
    poke(0x02 as *mut u8, 0xF8); // rc0 (low byte)
    poke(0x03 as *mut u8, 0xFF); // rc1 (high byte)

    // Black background and border
    poke(VIC_BG, COLOR_BLACK);
    poke(VIC_BORDER, COLOR_BLACK);
    // Clear screen
    clear_screen();
}

// ---------------------------------------------------------------------------
// Direct keyboard matrix scanning (replaces KERNAL keyboard buffer)
// ---------------------------------------------------------------------------

// Internal key codes for cursor keys (need shift detection).
const KEY_CRSR_VERT: u8 = 0x80;  // down/up depending on shift
const KEY_CRSR_HORIZ: u8 = 0x81; // right/left depending on shift

/// PETSCII codes returned by scan_keyboard().
pub const PETSCII_RETURN: u8 = 0x0D;
pub const PETSCII_DELETE: u8 = 0x14;
pub const PETSCII_SPACE: u8 = 0x20;
pub const PETSCII_STOP: u8 = 0x03;
pub const PETSCII_UP: u8 = 0x91;
pub const PETSCII_DOWN: u8 = 0x11;
pub const PETSCII_LEFT: u8 = 0x9D;
pub const PETSCII_RIGHT: u8 = 0x1D;

/// 8x8 keyboard matrix → key code lookup.
/// Row = CIA1 Port A bit (PA0-PA7), Column = CIA1 Port B bit (PB0-PB7).
/// 0 = unused/ignored key.
const KEY_MATRIX: [[u8; 8]; 8] = [
    // PA0: DEL, RETURN, CRSR_H, F7, F1, F3, F5, CRSR_V
    [PETSCII_DELETE, PETSCII_RETURN, KEY_CRSR_HORIZ, 0, 0, 0, 0, KEY_CRSR_VERT],
    // PA1: 3, W, A, 4, Z, S, E, LSHIFT
    [b'3', b'W', b'A', b'4', b'Z', b'S', b'E', 0],
    // PA2: 5, R, D, 6, C, F, T, X
    [b'5', b'R', b'D', b'6', b'C', b'F', b'T', b'X'],
    // PA3: 7, Y, G, 8, B, H, U, V
    [b'7', b'Y', b'G', b'8', b'B', b'H', b'U', b'V'],
    // PA4: 9, I, J, 0, M, K, O, N
    [b'9', b'I', b'J', b'0', b'M', b'K', b'O', b'N'],
    // PA5: +, P, L, -, ., :, @, ,
    [0, b'P', b'L', b'-', 0, 0, 0, 0],
    // PA6: pound, *, ;, HOME, RSHIFT, =, up-arrow, /
    [0; 8],
    // PA7: 1, left-arrow, CTRL, 2, SPACE, C=, Q, STOP
    [b'1', 0, 0, b'2', PETSCII_SPACE, 0, b'Q', PETSCII_STOP],
];

/// Previous keyboard state for edge detection (one byte per row).
static mut PREV_KEYS: [u8; 8] = [0; 8];

/// Scan the CIA1 keyboard matrix directly. Returns a key code on new
/// keypress (rising edge), or 0 if no new key. No KERNAL dependency.
pub fn scan_keyboard() -> u8 {
    let mut rows = [0u8; 8];

    // Scan all 8 rows
    for row in 0..8u8 {
        poke(CIA1_PA, !(1u8 << row));
        // Double-read for CIA settle time
        let _ = peek(CIA1_PB);
        let val = peek(CIA1_PB) ^ 0xFF; // invert: 1 = pressed
        rows[row as usize] = val;
    }
    poke(CIA1_PA, 0xFF); // restore for joystick reads

    // Check shift state: LSHIFT = PA1/PB7, RSHIFT = PA6/PB4
    let shifted = (rows[1] & 0x80 != 0) || (rows[6] & 0x10 != 0);

    // Find first newly pressed key (edge detection)
    let mut result: u8 = 0;
    for row in 0..8u8 {
        let prev = unsafe { PREV_KEYS[row as usize] };
        let newly = rows[row as usize] & !prev;
        if newly != 0 && result == 0 {
            for col in 0..8u8 {
                if newly & (1 << col) != 0 {
                    let key = KEY_MATRIX[row as usize][col as usize];
                    if key != 0 {
                        result = match key {
                            KEY_CRSR_VERT => if shifted { PETSCII_UP } else { PETSCII_DOWN },
                            KEY_CRSR_HORIZ => if shifted { PETSCII_LEFT } else { PETSCII_RIGHT },
                            _ => key,
                        };
                        break;
                    }
                }
            }
        }
    }

    unsafe { PREV_KEYS = rows; }
    result
}

/// Draw a decimal number (0-65535) at position. Returns number of digits written.
pub fn draw_number_u16(x: u8, y: u8, mut val: u16, color: u8) -> u8 {
    let mut buf = [0u8; 5];
    let mut len: u8 = 0;
    if val == 0 {
        draw_char(x, y, to_screen_code(b'0'), color);
        return 1;
    }
    while val > 0 {
        buf[len as usize] = b'0' + (val % 10) as u8;
        val /= 10;
        len += 1;
    }
    for i in 0..len {
        let ch = buf[(len - 1 - i) as usize];
        draw_char(x + i, y, to_screen_code(ch), color);
    }
    len
}

/// Draw a decimal number (0-255) at position. Delegates to draw_number_u16.
#[inline(always)]
pub fn draw_number(x: u8, y: u8, val: u8, color: u8) -> u8 {
    draw_number_u16(x, y, val as u16, color)
}
