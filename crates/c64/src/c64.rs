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

// --- Kernal keyboard buffer ---
pub const KEYBUF: *const u8 = 0x0277 as *const u8; // keyboard buffer (10 bytes)
pub const KEYBUF_LEN: *mut u8 = 0xC6 as *mut u8;   // number of chars in buffer
pub const CURSOR_FLAG: *mut u8 = 0xCC as *mut u8;   // 0=blink, nonzero=no blink

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

// --- CIA data direction registers ---
pub const CIA1_DDRA: *mut u8 = 0xDC02 as *mut u8; // Port A direction (keyboard cols)
pub const CIA1_DDRB: *mut u8 = 0xDC03 as *mut u8; // Port B direction (keyboard rows)

/// Initialize C64 hardware for game use.
/// NOTE: When loaded via BASIC (LOAD then RUN/SYS), Kernal IRQs are
/// already running and CIA1 DDRs are already configured. We just set
/// them explicitly as a safety net.
pub fn init_hardware() {
    // Set up CIA1 data direction registers for keyboard/joystick scanning.
    poke(CIA1_DDRA, 0xFF); // Port A = all output (keyboard columns)
    poke(CIA1_DDRB, 0x00); // Port B = all input  (keyboard rows)

    // Disable cursor blink (Kernal writes to screen during blink)
    poke(CURSOR_FLAG, 1);
    // Black background and border
    poke(VIC_BG, COLOR_BLACK);
    poke(VIC_BORDER, COLOR_BLACK);
    // Clear screen
    clear_screen();
}

/// Draw a decimal number (0-255) at position. Returns number of digits written.
pub fn draw_number(x: u8, y: u8, mut val: u8, color: u8) -> u8 {
    let mut buf = [0u8; 3];
    let mut len: u8 = 0;
    if val == 0 {
        draw_char(x, y, to_screen_code(b'0'), color);
        return 1;
    }
    while val > 0 {
        buf[len as usize] = b'0' + (val % 10);
        val /= 10;
        len += 1;
    }
    // buf is reversed; write digits in correct order
    for i in 0..len {
        let ch = buf[(len - 1 - i) as usize];
        draw_char(x + i, y, to_screen_code(ch), color);
    }
    len
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
