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
pub const VIC_RASTER: *mut u8 = 0xD012 as *mut u8;
pub const VIC_CTRL1: *mut u8 = 0xD011 as *mut u8;
pub const VIC_CTRL2: *mut u8 = 0xD016 as *mut u8;
pub const VIC_IRQ_STATUS: *mut u8 = 0xD019 as *mut u8;

// --- VIC-II sprite registers ---
pub const VIC_SPR0_X: *mut u8 = 0xD000 as *mut u8;
pub const VIC_SPR0_Y: *mut u8 = 0xD001 as *mut u8;
pub const VIC_SPR_ENABLE: *mut u8 = 0xD015 as *mut u8;
pub const VIC_SPR0_COLOR: *mut u8 = 0xD027 as *mut u8;
/// Sprite 0 data pointer (last 8 bytes of screen RAM: $07F8-$07FF)
pub const SPR0_PTR: *mut u8 = 0x07F8 as *mut u8;

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

// Voice 1 ($D400-$D406) — player attack SFX
const SID_V1_FREQ_LO: *mut u8 = 0xD400 as *mut u8;
const SID_V1_FREQ_HI: *mut u8 = 0xD401 as *mut u8;
const _SID_V1_PW_LO: *mut u8 = 0xD402 as *mut u8;
const _SID_V1_PW_HI: *mut u8 = 0xD403 as *mut u8;
const SID_V1_CTRL: *mut u8 = 0xD404 as *mut u8;
const SID_V1_AD: *mut u8 = 0xD405 as *mut u8;
const SID_V1_SR: *mut u8 = 0xD406 as *mut u8;

// Voice 2 ($D407-$D40D) — player hurt SFX
const SID_V2_FREQ_LO: *mut u8 = 0xD407 as *mut u8;
const SID_V2_FREQ_HI: *mut u8 = 0xD408 as *mut u8;
const SID_V2_PW_LO: *mut u8 = 0xD409 as *mut u8;
const SID_V2_PW_HI: *mut u8 = 0xD40A as *mut u8;
const SID_V2_CTRL: *mut u8 = 0xD40B as *mut u8;
const SID_V2_AD: *mut u8 = 0xD40C as *mut u8;
const SID_V2_SR: *mut u8 = 0xD40D as *mut u8;

// Voice 3 ($D40E-$D414) — music bass
const SID_V3_FREQ_LO: *mut u8 = 0xD40E as *mut u8;
const SID_V3_FREQ_HI: *mut u8 = 0xD40F as *mut u8;
const SID_V3_CTRL: *mut u8 = 0xD412 as *mut u8;
const SID_V3_AD: *mut u8 = 0xD413 as *mut u8;
const SID_V3_SR: *mut u8 = 0xD414 as *mut u8;

// Filter registers ($D415-$D417)
const SID_FILTER_LO: *mut u8 = 0xD415 as *mut u8;
const SID_FILTER_HI: *mut u8 = 0xD416 as *mut u8;
const SID_FILTER_ROUTE: *mut u8 = 0xD417 as *mut u8;

// Volume/filter mode ($D418)
const SID_VOL: *mut u8 = 0xD418 as *mut u8;

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

/// Write a raw screen code with color at position.
#[inline(always)]
pub fn draw_sc(x: u8, y: u8, sc: u8, color: u8) {
    let offset = (y as usize) * 40 + (x as usize);
    unsafe {
        write_volatile(SCREEN.add(offset), sc);
        write_volatile(COLOR_RAM.add(offset), color);
    }
}

/// Write a single ASCII character with color at position.
/// Converts to C64 screen code internally.
#[inline(always)]
pub fn draw_char(x: u8, y: u8, ascii: u8, color: u8) {
    draw_sc(x, y, to_screen_code(ascii), color);
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

/// Master switch for vblank synchronization in the render path.
/// Set to `false` to disable — the compiler dead-code-eliminates entirely.
pub const VBLANK_SYNC: bool = true;

/// Wait for vblank if `VBLANK_SYNC` is enabled; no-op otherwise.
#[inline(always)]
pub fn sync_frame() {
    if VBLANK_SYNC {
        wait_vblank();
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

    // Initialize SID for sound effects
    sid_init();

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
        draw_char(x, y, b'0', color);
        return 1;
    }
    while val > 0 {
        buf[len as usize] = b'0' + (val % 10) as u8;
        val /= 10;
        len += 1;
    }
    for i in 0..len {
        let ch = buf[(len - 1 - i) as usize];
        draw_char(x + i, y, ch, color);
    }
    len
}

/// Draw a decimal number (0-255) at position. Delegates to draw_number_u16.
#[inline(always)]
pub fn draw_number(x: u8, y: u8, val: u8, color: u8) -> u8 {
    draw_number_u16(x, y, val as u16, color)
}

// ---------------------------------------------------------------------------
// SID sound effects — percussive one-shots for combat feedback
// ---------------------------------------------------------------------------

/// Initialize SID for sound effects. Sets master volume to max.
/// Called once from init_hardware().
fn sid_init() {
    // Master volume = 15 (max), no filter routing
    poke(SID_VOL, 0x0F);
}

/// Play attack sound: short noise burst on Voice 1 (sword slash).
///
/// Noise waveform at mid-high frequency with instant attack and fast decay.
/// Gate off→on transition restarts the ADSR envelope each call.
///
/// Sets the V1 SFX holdoff counter ($033B) so the music handler won't
/// overwrite Voice 1 while the SFX is playing.
pub fn sfx_attack() {
    // Tell music handler to leave V1 alone for 12 frames (240ms)
    unsafe { write_volatile(0x033B as *mut u8, SFX_HOLDOFF_V1); }
    // Gate off to reset ADSR (needed if previous sound's gate is still on)
    poke(SID_V1_CTRL, 0x80);        // noise waveform, gate=0
    // Frequency: $2000 — mid-high noise for a sharp hiss
    poke(SID_V1_FREQ_LO, 0x00);
    poke(SID_V1_FREQ_HI, 0x20);
    // ADSR: A=0 (2ms), D=3 (72ms), S=0, R=0 (6ms)
    poke(SID_V1_AD, 0x03);
    poke(SID_V1_SR, 0x00);
    // Gate on — triggers ADSR: instant peak → 72ms decay to silence
    poke(SID_V1_CTRL, 0x81);        // noise + gate
}

/// Play hurt sound: low pulse thud on Voice 2 (taking damage).
///
/// Pulse waveform at low frequency with instant attack and medium decay.
/// Distinct from attack sound in both pitch and timbre.
///
/// Sets the V2 SFX holdoff counter ($033C) so the music handler won't
/// overwrite Voice 2 while the SFX is playing.
pub fn sfx_hurt() {
    // Tell music handler to leave V2 alone for 15 frames (300ms)
    unsafe { write_volatile(0x033C as *mut u8, SFX_HOLDOFF_V2); }
    // Gate off to reset ADSR
    poke(SID_V2_CTRL, 0x40);        // pulse waveform, gate=0
    // Frequency: $0800 — low tone for a dull impact
    poke(SID_V2_FREQ_LO, 0x00);
    poke(SID_V2_FREQ_HI, 0x08);
    // Pulse width: $0800 (50% duty cycle)
    poke(SID_V2_PW_LO, 0x00);
    poke(SID_V2_PW_HI, 0x08);
    // ADSR: A=0 (2ms), D=5 (168ms), S=0, R=0 (6ms)
    poke(SID_V2_AD, 0x05);
    poke(SID_V2_SR, 0x00);
    // Gate on — triggers ADSR: instant peak → 168ms decay to silence
    poke(SID_V2_CTRL, 0x41);        // pulse + gate
}

// ---------------------------------------------------------------------------
// Sprite-based loading spinner — animates a spinning sword during generation
// ---------------------------------------------------------------------------

/// Sprite frame data: 8 frames × 64 bytes (63 pixel data + 1 padding).
/// Designed in Spritemate, singlecolor, color $03 (cyan).
/// Copied to $0340-$053F at runtime (VIC-II sprite pointers 13-20).
const SPRITE_DATA: [u8; 512] = [
    // Frame 0: vertical sword pointing up
    0x00, 0x18, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C,
    0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00,
    0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00,
    0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C,
    0x00, 0x00, 0x3C, 0x00, 0x03, 0xBD, 0xC0, 0x01,
    0xFF, 0x80, 0x00, 0x7E, 0x00, 0x00, 0x18, 0x00,
    0x00, 0x18, 0x00, 0x00, 0x18, 0x00, 0x00, 0x18,
    0x00, 0x00, 0x3C, 0x00, 0x00, 0x18, 0x00, 0x00,
    // Frame 1: NE diagonal
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x18, 0x00, 0x00, 0x38, 0x00, 0x00, 0x78, 0x00,
    0x00, 0xF0, 0x00, 0x01, 0xE0, 0x00, 0x03, 0xC0,
    0x00, 0x07, 0x80, 0x00, 0x0F, 0x00, 0x04, 0x1E,
    0x00, 0x06, 0x3C, 0x00, 0x03, 0x78, 0x00, 0x01,
    0xF0, 0x00, 0x01, 0xE0, 0x00, 0x03, 0xF0, 0x00,
    0x0F, 0xB8, 0x00, 0x0F, 0x0C, 0x00, 0x0F, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Frame 2: horizontal sword pointing right
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x01,
    0x80, 0x00, 0x01, 0x80, 0x00, 0x03, 0x00, 0x00,
    0x43, 0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0x43, 0xFF, 0xFE, 0x03, 0x00, 0x00, 0x01,
    0x80, 0x00, 0x01, 0x80, 0x00, 0x00, 0x80, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Frame 3: SE diagonal
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x00,
    0x00, 0x0F, 0x0C, 0x00, 0x0F, 0xB8, 0x00, 0x03,
    0xF0, 0x00, 0x01, 0xE0, 0x00, 0x01, 0xF0, 0x00,
    0x03, 0x78, 0x00, 0x06, 0x3C, 0x00, 0x04, 0x1E,
    0x00, 0x00, 0x0F, 0x00, 0x00, 0x07, 0x80, 0x00,
    0x03, 0xC0, 0x00, 0x01, 0xE0, 0x00, 0x00, 0xF0,
    0x00, 0x00, 0x78, 0x00, 0x00, 0x38, 0x00, 0x00,
    0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Frame 4: vertical sword pointing down
    0x00, 0x18, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x18,
    0x00, 0x00, 0x18, 0x00, 0x00, 0x18, 0x00, 0x00,
    0x18, 0x00, 0x00, 0x7E, 0x00, 0x01, 0xFF, 0x80,
    0x03, 0xBD, 0xC0, 0x00, 0x3C, 0x00, 0x00, 0x3C,
    0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00,
    0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00,
    0x00, 0x3C, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x3C,
    0x00, 0x00, 0x3C, 0x00, 0x00, 0x18, 0x00, 0x00,
    // Frame 5: SW diagonal
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xF0, 0x00, 0x30, 0xF0, 0x00, 0x1D, 0xF0, 0x00,
    0x0F, 0xC0, 0x00, 0x07, 0x80, 0x00, 0x0F, 0x80,
    0x00, 0x1E, 0xC0, 0x00, 0x3C, 0x60, 0x00, 0x78,
    0x20, 0x00, 0xF0, 0x00, 0x01, 0xE0, 0x00, 0x03,
    0xC0, 0x00, 0x07, 0x80, 0x00, 0x0F, 0x00, 0x00,
    0x1E, 0x00, 0x00, 0x1C, 0x00, 0x00, 0x18, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Frame 6: horizontal sword pointing left
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x01, 0x80, 0x00, 0x01, 0x80, 0x00, 0x00, 0xC0,
    0x7F, 0xFF, 0xC2, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0x7F, 0xFF, 0xC2, 0x00, 0x00, 0xC0, 0x00,
    0x01, 0x80, 0x00, 0x01, 0x80, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // Frame 7: NW diagonal
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00,
    0x00, 0x1C, 0x00, 0x00, 0x1E, 0x00, 0x00, 0x0F,
    0x00, 0x00, 0x07, 0x80, 0x00, 0x03, 0xC0, 0x00,
    0x01, 0xE0, 0x00, 0x00, 0xF0, 0x00, 0x00, 0x78,
    0x20, 0x00, 0x3C, 0x60, 0x00, 0x1E, 0xC0, 0x00,
    0x0F, 0x80, 0x00, 0x07, 0x80, 0x00, 0x0F, 0xC0,
    0x00, 0x1D, 0xF0, 0x00, 0x30, 0xF0, 0x00, 0x00,
    0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Base address for sprite data in VIC bank 0.
const SPRITE_BASE_ADDR: u16 = 0x0340;
/// VIC-II sprite pointer value for first frame ($0340 / 64 = 13).
const SPRITE_BASE_PTR: u8 = (SPRITE_BASE_ADDR / 64) as u8; // 13

/// 6502 machine code for the raster IRQ handler (48 bytes).
///
/// Cycles sprite 0's data pointer through 8 frames every 6th VBlank
/// (~120ms per frame at 50Hz, full rotation ≈ 960ms).
///
/// State at absolute addresses ($0334-$0337):
///   - $0334: frame counter (decrements each VBlank, reloads to 6)
///   - $0335: sprite frame index (0-7)
///   - $0336: saved A register
///   - $0337: saved X register
///
/// No runtime patching needed — all addresses are compile-time constants.
#[used]
static SPINNER_HANDLER: [u8; 48] = [
    0x8D, 0x36, 0x03,       // 00: STA $0336  (save A)
    0x8E, 0x37, 0x03,       // 03: STX $0337  (save X)
    0xCE, 0x34, 0x03,       // 06: DEC $0334  (frame counter)
    0xD0, 0x16,             // 09: BNE +22 → offset 0x21 (ack)
    0xA9, 0x06,             // 0B: LDA #6     (reload counter)
    0x8D, 0x34, 0x03,       // 0D: STA $0334
    0xAD, 0x35, 0x03,       // 10: LDA $0335  (current frame)
    0x18,                   // 13: CLC
    0x69, 0x01,             // 14: ADC #1
    0x29, 0x07,             // 16: AND #7     (wrap 0-7)
    0x8D, 0x35, 0x03,       // 18: STA $0335
    0x18,                   // 1B: CLC
    0x69, 0x0D,             // 1C: ADC #13    (base pointer: $0340/64)
    0x8D, 0xF8, 0x07,       // 1E: STA $07F8  (sprite 0 pointer)
    // ack:
    0xA9, 0xFF,             // 21: LDA #$FF
    0x8D, 0x19, 0xD0,       // 23: STA $D019  (clear VIC-II IRQ)
    0xAD, 0x0D, 0xDC,       // 26: LDA $DC0D  (ack CIA)
    0xAE, 0x37, 0x03,       // 29: LDX $0337  (restore X)
    0xAD, 0x36, 0x03,       // 2C: LDA $0336  (restore A)
    0x40,                   // 2F: RTI
];

/// Start the sprite-based raster interrupt spinner.
///
/// Copies sprite frame data to $0340-$053F (VIC bank 0), sets up sprite 0,
/// and installs a VIC-II raster IRQ handler that cycles through 8 frames.
///
/// Screen RAM rows 0-7 are temporarily overwritten with sprite data;
/// color RAM for those rows is set to black so the garbage is invisible.
/// The full screen is redrawn after generation completes.
///
/// No SEI/CLI needed: after init_hardware() the CPU I flag is already clear
/// and all IRQ sources are disabled. Disabling the source in $D01A deasserts
/// the IRQ line immediately, making vector updates race-free.
#[inline(never)]
pub fn spinner_start() {
    unsafe {
        // Copy 512 bytes of sprite frame data to $0340-$053F
        let src = SPRITE_DATA.as_ptr();
        let dst = SPRITE_BASE_ADDR as *mut u8;
        for i in 0..512u16 {
            write_volatile(dst.add(i as usize), *src.add(i as usize));
        }

        // Zero color RAM for screen rows 0-7 so overwritten screen chars
        // are invisible (black on black). Rows 0-7 = 320 bytes at $D800.
        let color_base = COLOR_RAM;
        for i in 0..320u16 {
            write_volatile(color_base.add(i as usize), COLOR_BLACK);
        }

        // Configure sprite 0
        poke(VIC_SPR0_X, 168);    // centered on text (pixel 168+12=180)
        poke(VIC_SPR0_Y, 146);    // row 12, below "GENERATING..." at row 10
        poke(VIC_SPR0_COLOR, COLOR_CYAN);
        poke(SPR0_PTR, SPRITE_BASE_PTR); // initial frame
        poke(VIC_SPR_ENABLE, 0x01);      // enable sprite 0

        // Initialize spinner state
        write_volatile(0x0334 as *mut u8, 6);  // frame counter
        write_volatile(0x0335 as *mut u8, 0);  // frame index

        // Point hardware IRQ vector to our handler
        let handler_addr = core::ptr::addr_of!(SPINNER_HANDLER) as u16;
        write_volatile(0xFFFE as *mut u8, (handler_addr & 0xFF) as u8);
        write_volatile(0xFFFF as *mut u8, (handler_addr >> 8) as u8);

        // Configure VIC-II raster interrupt at line 251 (vblank)
        poke(VIC_RASTER, 251);
        let ctrl1 = peek(VIC_CTRL1 as *const u8);
        poke(VIC_CTRL1, ctrl1 & 0x7F); // clear raster bit 8

        // Acknowledge all pending interrupt sources
        poke(VIC_IRQ_STATUS, 0xFF);
        let _ = peek(CIA1_ICR as *const u8);
        let _ = peek(CIA2_ICR as *const u8);

        // Enable VIC-II raster interrupt — IRQs start firing immediately
        poke(VIC_IRQ_MASK, 0x01);
    }
}

/// Stop the raster interrupt spinner.
///
/// Disabling the VIC-II raster source in $D01A immediately deasserts
/// the IRQ line, so no more handler invocations occur after the write.
/// This means we can safely update $FFFE/$FFFF without SEI.
#[inline(never)]
pub fn spinner_stop() {
    // Disable VIC-II raster interrupt — IRQ line deasserts immediately
    poke(VIC_IRQ_MASK, 0x00);

    // Acknowledge any pending + clear CIA latches
    poke(VIC_IRQ_STATUS, 0xFF);
    let _ = peek(CIA1_ICR as *const u8);
    let _ = peek(CIA2_ICR as *const u8);

    // Disable sprite 0
    poke(VIC_SPR_ENABLE, 0x00);

    // Restore IRQ vector to safe RTI stub
    // Safe: source disabled above, no IRQ can fire mid-update
    unsafe {
        write_volatile(0xFFFE as *mut u8, 0x00); // $E000 low
        write_volatile(0xFFFF as *mut u8, 0xE0); // $E000 high
    }
}

// ---------------------------------------------------------------------------
// Screen shake — brief horizontal jolt on player attack via raster IRQ
// ---------------------------------------------------------------------------

/// Number of VBlank frames the screen shake lasts.
/// With alternating pattern: ceil(N/2) frames of visible shift.
const SHAKE_FRAMES: u8 = 4;

/// 6502 machine code for the screen shake IRQ handler (59 bytes).
///
/// Alternates VIC-II XSCROLL ($D016 bits 0-2) between 2 pixels and 0
/// each VBlank, creating a brief horizontal screen jolt. Auto-stops
/// after the frame counter expires: restores $D016, disables raster
/// IRQ, and resets the IRQ vector to the RTI stub at $E000.
///
/// State at absolute addresses (shared save slots with spinner — they
/// never run simultaneously):
///   - $0336: saved A register
///   - $0337: saved X register
///   - $0338: frame counter (decremented each VBlank, 0 = done)
#[used]
static SHAKE_HANDLER: [u8; 59] = [
    0x8D, 0x36, 0x03,       // 00: STA $0336  (save A)
    0x8E, 0x37, 0x03,       // 03: STX $0337  (save X)
    0xAE, 0x38, 0x03,       // 06: LDX $0338  (load counter)
    0xCA,                    // 09: DEX         (counter--)
    0x8E, 0x38, 0x03,       // 0A: STX $0338  (store counter)
    0xF0, 0x18,              // 0D: BEQ +24 → stop (offset 0x27)
    0x8A,                    // 0F: TXA         (A = counter)
    0x29, 0x01,              // 10: AND #$01    (odd = shift, even = center)
    0x0A,                    // 12: ASL A       (0 or 2)
    0x09, 0x08,              // 13: ORA #$08    (CSEL=1: 40 columns)
    0x8D, 0x16, 0xD0,       // 15: STA $D016   (apply XSCROLL)
    // ack:
    0xA9, 0xFF,              // 18: LDA #$FF
    0x8D, 0x19, 0xD0,       // 1A: STA $D019   (clear VIC-II IRQ)
    0xAD, 0x0D, 0xDC,       // 1D: LDA $DC0D   (ack CIA)
    0xAE, 0x37, 0x03,       // 20: LDX $0337   (restore X)
    0xAD, 0x36, 0x03,       // 23: LDA $0336   (restore A)
    0x40,                    // 26: RTI
    // stop:
    0xA9, 0x08,              // 27: LDA #$08
    0x8D, 0x16, 0xD0,       // 29: STA $D016   (restore XSCROLL=0)
    0xA9, 0x00,              // 2C: LDA #$00
    0x8D, 0x1A, 0xD0,       // 2E: STA $D01A   (disable raster IRQ)
    0x8D, 0xFE, 0xFF,       // 31: STA $FFFE   (IRQ vector lo = $00)
    0xA9, 0xE0,              // 34: LDA #$E0
    0x8D, 0xFF, 0xFF,       // 36: STA $FFFF   (IRQ vector hi = $E0)
    0xD0, 0xDD,              // 39: BNE -35 → ack (offset 0x18)
];

/// Trigger a screen shake effect.
///
/// Sets the shake frame counter at $0338. The music IRQ handler reads this
/// counter each VBlank and alternates `$D016` XSCROLL between 0 and 2 pixels,
/// restoring on expiry.
///
/// During gameplay the music handler is always active, so no handler swap is
/// needed — just poke the counter. Safe to call during an active shake
/// (resets the counter).
#[inline(never)]
pub fn shake_start() {
    unsafe {
        write_volatile(0x0338 as *mut u8, SHAKE_FRAMES);
    }
}

// ---------------------------------------------------------------------------
// SID ambient music — 3-voice dungeon soundtrack via raster IRQ
// ---------------------------------------------------------------------------
//
// Three triangle-wave voices play a slow A-minor pattern during gameplay:
//   V3 (bass):  root notes, always active
//   V1 (lead):  sparse mid-range melody, yielded to attack SFX
//   V2 (pad):   sustained harmony, yielded to hurt SFX
//
// SFX priority: per-voice holdoff counters at $033B/$033C prevent the music
// handler from overwriting SID registers while a combat SFX is active.
// When the holdoff expires, the handler reclaims the voice immediately
// (gate off → set freq/ADSR → gate on) for a clean ADSR restart.

/// Bass/pad ADSR: A=8 (100ms attack), D=0.
const MUSIC_AD: u8 = 0x80;
/// Bass/pad ADSR: S=F (max sustain), R=6 (300ms release).
const MUSIC_SR: u8 = 0xF6;
/// Lead ADSR: A=A (500ms attack), D=0 — notes swell in gradually.
const MUSIC_LEAD_AD: u8 = 0xA0;
/// Lead ADSR: S=A (~67% sustain), R=9 (750ms release) — sits below drone.
const MUSIC_LEAD_SR: u8 = 0xA9;
/// Triangle waveform + gate (all voices).
const MUSIC_TRI_ON: u8 = 0x11;
/// Triangle waveform, gate off (for ADSR restart / rest).
const MUSIC_TRI_OFF: u8 = 0x10;
/// Ticks per note step (1.0s at 50 Hz PAL).
const MUSIC_TEMPO: u8 = 50;
/// Number of notes in the loop.
const MUSIC_SEQ_LEN: u8 = 48;

/// Frames of music playback at full volume before fade-out begins.
const MUSIC_PLAY_FRAMES: u16 = (MUSIC_SEQ_LEN as u16) * (MUSIC_TEMPO as u16); // 2400
/// Minimum frames of silence between music loops (60 seconds at 50 Hz).
const MUSIC_SILENCE_MIN: u16 = 3000;
/// Frames between volume steps during fade (10 × 15 steps = 3.0s at 50 Hz).
const MUSIC_FADE_STEP: u8 = 10;

/// Auto-cycle phase constants.
const MUSIC_OFF: u8 = 0;
const MUSIC_PLAYING: u8 = 1;
const MUSIC_FADE_OUT: u8 = 2;
const MUSIC_SILENT: u8 = 3;
const MUSIC_FADE_IN: u8 = 4;

/// Current auto-cycle phase (MUSIC_OFF/PLAYING/FADE_OUT/SILENT/FADE_IN).
static mut MUSIC_PHASE: u8 = MUSIC_OFF;
/// Frames remaining in the current phase (play duration or silence duration).
static mut MUSIC_TIMER: u16 = 0;
/// Current master volume during fades (0-15).
static mut MUSIC_FADE_VOL: u8 = 15;
/// Frames until next volume step during fades.
static mut MUSIC_FADE_TICK: u8 = 0;

/// Frames to hold off music on V1 after attack SFX (240ms at 50 Hz).
/// Attack SFX ADSR: A=0, D=3 (72ms) — holdoff of 12 frames gives margin.
const SFX_HOLDOFF_V1: u8 = 12;
/// Frames to hold off music on V2 after hurt SFX (300ms at 50 Hz).
/// Hurt SFX ADSR: A=0, D=5 (168ms) — holdoff of 15 frames gives margin.
const SFX_HOLDOFF_V2: u8 = 15;

/// SID frequency lookup table — PAL clock (985248 Hz).
///
/// freq_register = note_hz × 16777216 / 985248
///
/// Each entry is (freq_lo, freq_hi). Notes span 3 octaves:
///   Bass (V3):  E2–Bb2  (82–117 Hz)  — triangle, LP filtered
///   Pad  (V2):  B2–E3   (123–165 Hz) — triangle
///   Lead (V1):  A3–E4   (220–330 Hz) — pulse wave
const NOTE_E2: (u8, u8)  = (0x7B, 0x05);   //  82.41 Hz  bass
const NOTE_F2: (u8, u8)  = (0xCF, 0x05);   //  87.31 Hz  bass
const NOTE_A2: (u8, u8)  = (0x51, 0x07);   // 110.00 Hz  bass
const NOTE_BB2: (u8, u8) = (0xC1, 0x07);   // 116.54 Hz  bass (Phrygian bII)
const NOTE_B2: (u8, u8)  = (0x37, 0x08);   // 123.47 Hz  pad
const NOTE_C3: (u8, u8)  = (0xB4, 0x08);   // 130.81 Hz  pad
const NOTE_D3: (u8, u8)  = (0xC4, 0x09);   // 146.83 Hz  pad
const NOTE_E3: (u8, u8)  = (0xF7, 0x0A);   // 164.81 Hz  pad
const NOTE_A3: (u8, u8)  = (0xA3, 0x0E);   // 220.00 Hz  lead
const NOTE_BB3: (u8, u8) = (0x81, 0x0F);   // 233.08 Hz  lead (Phrygian bII)
const NOTE_C4: (u8, u8)  = (0x68, 0x11);   // 261.63 Hz  lead
const NOTE_D4: (u8, u8)  = (0x89, 0x13);   // 293.66 Hz  lead
const NOTE_E4: (u8, u8)  = (0xEE, 0x15);   // 329.63 Hz  lead
/// Rest sentinel: freq_hi == 0 tells the handler to gate off the voice.
const REST: (u8, u8)     = (0x00, 0x00);

/// Note sequence tables: 48-step A Phrygian ambient (288 bytes).
///
/// Split across two memory regions:
///   $0340-$03FF: V3 freq_lo/hi + V1 freq_lo/hi (4 × 48 = 192 bytes)
///   $0200-$025F: V2 freq_lo/hi (2 × 48 = 96 bytes)
///
/// 48s loop (1s/step at 50 Hz PAL). Voices span 3 octaves.
/// Rest sentinel (freq_hi=0) gates voice off for silence.
///
/// Lead (~15% present): 7 notes in 48 steps, built from two motifs:
///   Opening: E4..D4 (gentle descent, 4s apart)
///   Closing: Bb3→A3 (Phrygian half-step cadence, immediate)
///   Between: solo notes A3, C4, E4 spaced 7-9s apart
///
/// Three-act arc:
///   A (0-15):  The Void — root drone, lead enters E4 then D4, pad fades
///   B (16-31): The Descent — bass drops E2→F2, pad shifts, lead sparse
///   C (32-47): Return — Bb2 bass dread, Bb3→A3 cadence, resolves home
///
/// Step  V3(bass)  V2(pad)   V1(lead)  Sonority
/// ───── ──────── ──────── ──────── ──────────────────
///  0    A2        E3        —         open 5th drone
///  1    A2        E3        —           │
///  2    A2        E3        —           │
///  3    A2        E3        —           │
///  4    A2        E3        —           │
///  5    A2        E3        —           │
///  6    A2        E3        E4        lead enters high: Am spread
///  7    A2        E3        —         lead fades
///  8    A2        —         —         pad drops — just bass drone
///  9    A2        —         —           │
/// 10    A2        —         D4        lead descends, exposed over bass
/// 11    A2        —         —         lead fades, bass drone alone
/// 12    A2        —         —           │
/// 13    A2        —         —           │
/// 14    A2        —         —           │
/// 15    A2        —         —           │
/// 16    E2        B2        —         bass descends, pad returns: Em open
/// 17    E2        B2        —           │
/// 18    E2        B2        A3        lead: root octave, grounding
/// 19    E2        B2        —         lead fades
/// 20    E2        C3        —         pad darkens: Am/E
/// 21    E2        C3        —           │
/// 22    E2        C3        —           │
/// 23    E2        C3        —           │
/// 24    F2        C3        —         bass → F (bVI): imposing F5
/// 25    F2        C3        C4        lead: C octave with pad, warm
/// 26    F2        C3        —         lead fades
/// 27    F2        C3        —           │
/// 28    F2        —         —         pad drops — F bass alone (desolate)
/// 29    F2        —         —           │
/// 30    F2        —         —           │
/// 31    F2        —         —           │
/// 32    E2        B2        —         bass → E, pad returns: Em open
/// 33    E2        B2        —           │
/// 34    E2        B2        E4        lead returns high: echo of step 6
/// 35    E2        B2        —         lead fades
/// 36    BB2       D3        —         !! Phrygian bII bass (Bb major)
/// 37    BB2       D3        —           │
/// 38    BB2       D3        —           │
/// 39    BB2       D3        —           │
/// 40    A2        E3        —         bass resolves home, open 5th
/// 41    A2        E3        —           │
/// 42    A2        E3        BB3       !! Phrygian shock: Am(b9)
/// 43    A2        E3        A3        resolves: Bb→A half-step cadence
/// 44    A2        E3        —         silence, settling
/// 45    A2        E3        —           │
/// 46    A2        E3        —           │
/// 47    A2        E3        —         (= step 0, seamless loop)
const NOTE_TABLES: [u8; 288] = [
    // ====== Copied to $0340-$03FF (192 bytes) ======

    // $0340: V3 freq_lo (bass — always plays) — 48 entries
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   //  0: A drone
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   //  4:   │
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   //  8:   │
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   // 12:   │
    NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,   // 16: →E (descent)
    NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,   // 20:   │
    NOTE_F2.0,  NOTE_F2.0,  NOTE_F2.0,  NOTE_F2.0,   // 24: →F (bVI)
    NOTE_F2.0,  NOTE_F2.0,  NOTE_F2.0,  NOTE_F2.0,   // 28:   │
    NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,  NOTE_E2.0,   // 32: →E
    NOTE_BB2.0, NOTE_BB2.0, NOTE_BB2.0, NOTE_BB2.0,  // 36: →Bb (bII!)
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   // 40: →A (home)
    NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,  NOTE_A2.0,   // 44:   │
    // $0370: V3 freq_hi (bass) — 48 entries
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   //  0: A drone
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   //  4:   │
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   //  8:   │
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   // 12:   │
    NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,   // 16: →E
    NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,   // 20:   │
    NOTE_F2.1,  NOTE_F2.1,  NOTE_F2.1,  NOTE_F2.1,   // 24: →F
    NOTE_F2.1,  NOTE_F2.1,  NOTE_F2.1,  NOTE_F2.1,   // 28:   │
    NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,  NOTE_E2.1,   // 32: →E
    NOTE_BB2.1, NOTE_BB2.1, NOTE_BB2.1, NOTE_BB2.1,  // 36: →Bb
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   // 40: →A
    NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,  NOTE_A2.1,   // 44:   │
    // $03A0: V1 freq_lo (lead — pulse wave, very sparse) — 48 entries
    REST.0,     REST.0,     REST.0,     REST.0,       //  0: silence
    REST.0,     REST.0,     NOTE_E4.0,  REST.0,       //  4: —,—,E4,—
    REST.0,     REST.0,     NOTE_D4.0,  REST.0,       //  8: —,—,D4,—
    REST.0,     REST.0,     REST.0,     REST.0,       // 12: silence
    REST.0,     REST.0,     NOTE_A3.0,  REST.0,       // 16: —,—,A3,—
    REST.0,     REST.0,     REST.0,     REST.0,       // 20: silence
    REST.0,     NOTE_C4.0,  REST.0,     REST.0,       // 24: —,C4,—,—
    REST.0,     REST.0,     REST.0,     REST.0,       // 28: silence
    REST.0,     REST.0,     NOTE_E4.0,  REST.0,       // 32: —,—,E4,—
    REST.0,     REST.0,     REST.0,     REST.0,       // 36: silence
    REST.0,     REST.0,     NOTE_BB3.0, NOTE_A3.0,    // 40: —,—,Bb3,A3
    REST.0,     REST.0,     REST.0,     REST.0,       // 44: silence
    // $03D0: V1 freq_hi (lead) — 48 entries (0x00 = rest)
    REST.1,     REST.1,     REST.1,     REST.1,       //  0: silence
    REST.1,     REST.1,     NOTE_E4.1,  REST.1,       //  4: —,—,E4,—
    REST.1,     REST.1,     NOTE_D4.1,  REST.1,       //  8: —,—,D4,—
    REST.1,     REST.1,     REST.1,     REST.1,       // 12: silence
    REST.1,     REST.1,     NOTE_A3.1,  REST.1,       // 16: —,—,A3,—
    REST.1,     REST.1,     REST.1,     REST.1,       // 20: silence
    REST.1,     NOTE_C4.1,  REST.1,     REST.1,       // 24: —,C4,—,—
    REST.1,     REST.1,     REST.1,     REST.1,       // 28: silence
    REST.1,     REST.1,     NOTE_E4.1,  REST.1,       // 32: —,—,E4,—
    REST.1,     REST.1,     REST.1,     REST.1,       // 36: silence
    REST.1,     REST.1,     NOTE_BB3.1, NOTE_A3.1,    // 40: —,—,Bb3,A3
    REST.1,     REST.1,     REST.1,     REST.1,       // 44: silence

    // ====== Copied to $0200-$025F (96 bytes) ======

    // $0200: V2 freq_lo (pad — triangle, rests at 8-15 and 28-31) — 48 entries
    NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,   //  0: E3 (open 5th)
    NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,   //  4:   │
    REST.0,     REST.0,     REST.0,     REST.0,       //  8: — (bass alone)
    REST.0,     REST.0,     REST.0,     REST.0,       // 12:   │
    NOTE_B2.0,  NOTE_B2.0,  NOTE_B2.0,  NOTE_B2.0,   // 16: B2 (Em open)
    NOTE_C3.0,  NOTE_C3.0,  NOTE_C3.0,  NOTE_C3.0,   // 20: C3 (Am/E)
    NOTE_C3.0,  NOTE_C3.0,  NOTE_C3.0,  NOTE_C3.0,   // 24: C3 (F5)
    REST.0,     REST.0,     REST.0,     REST.0,       // 28: — (F bass alone)
    NOTE_B2.0,  NOTE_B2.0,  NOTE_B2.0,  NOTE_B2.0,   // 32: B2 (Em open)
    NOTE_D3.0,  NOTE_D3.0,  NOTE_D3.0,  NOTE_D3.0,   // 36: D3 (Bb major)
    NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,   // 40: E3 (open 5th)
    NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,  NOTE_E3.0,   // 44:   │
    // $0230: V2 freq_hi (pad) — 48 entries (0x00 = rest)
    NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,   //  0: E3
    NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,   //  4:   │
    REST.1,     REST.1,     REST.1,     REST.1,       //  8: —
    REST.1,     REST.1,     REST.1,     REST.1,       // 12:   │
    NOTE_B2.1,  NOTE_B2.1,  NOTE_B2.1,  NOTE_B2.1,   // 16: B2
    NOTE_C3.1,  NOTE_C3.1,  NOTE_C3.1,  NOTE_C3.1,   // 20: C3
    NOTE_C3.1,  NOTE_C3.1,  NOTE_C3.1,  NOTE_C3.1,   // 24: C3
    REST.1,     REST.1,     REST.1,     REST.1,       // 28: —
    NOTE_B2.1,  NOTE_B2.1,  NOTE_B2.1,  NOTE_B2.1,   // 32: B2
    NOTE_D3.1,  NOTE_D3.1,  NOTE_D3.1,  NOTE_D3.1,   // 36: D3
    NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,   // 40: E3
    NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,  NOTE_E3.1,   // 44:   │
];

/// 6502 machine code for the combined music + shake IRQ handler (238 bytes).
///
/// Runs every VBlank (50 Hz). Handles screen shake (reads $0338), SFX holdoff
/// voice reclaim ($033B/$033C), and 3-voice music stepping ($0339/$033A).
///
/// State at absolute addresses:
///   - $0336: saved A register (shared with spinner/shake)
///   - $0337: saved X register (shared with spinner/shake)
///   - $0338: shake frame counter (0 = inactive)
///   - $0339: music tick counter (decrements each VBlank)
///   - $033A: note sequence index (0-15)
///   - $033B: V1 SFX holdoff counter (0 = music owns voice)
///   - $033C: V2 SFX holdoff counter (0 = music owns voice)
///   - $0200-$025F: V2 frequency tables (2 × 48 bytes)
///   - $0340-$03FF: V3 + V1 frequency tables (4 × 48 bytes)
#[used]
static MUSIC_HANDLER: [u8; 238] = [
    // === Save registers ===
    0x8D, 0x36, 0x03,       // 00: STA $0336      (save A)
    0x8E, 0x37, 0x03,       // 03: STX $0337      (save X)

    // === Shake section ===
    0xAD, 0x38, 0x03,       // 06: LDA $0338      (shake counter)
    0xF0, 0x13,              // 09: BEQ +19 → .no_shake (0x1E)
    0x38,                    // 0B: SEC
    0xE9, 0x01,              // 0C: SBC #1         (counter--)
    0x8D, 0x38, 0x03,       // 0E: STA $0338
    0xF0, 0x06,              // 11: BEQ +6 → .shk_rest (0x19)
    0x29, 0x01,              // 13: AND #$01       (odd = shift)
    0x0A,                    // 15: ASL A          (0 or 2)
    0x09, 0x08,              // 16: ORA #$08       (CSEL=1)
    0x2C,                    // 18: .byte $2C      (BIT abs — skip LDA)
    // .shk_rest:
    0xA9, 0x08,              // 19: LDA #$08       (XSCROLL=0, CSEL=1)
    0x8D, 0x16, 0xD0,       // 1B: STA $D016

    // === V1 holdoff — decrement, reclaim on expiry ===
    // .no_shake:
    0xAD, 0x3B, 0x03,       // 1E: LDA $033B      (V1 holdoff)
    0xF0, 0x2B,              // 21: BEQ +43 → .v1_done (0x4E)
    0x38,                    // 23: SEC
    0xE9, 0x01,              // 24: SBC #1
    0x8D, 0x3B, 0x03,       // 26: STA $033B
    0xD0, 0x23,              // 29: BNE +35 → .v1_done (0x4E)
    // Reclaim V1: gate off → freq + ADSR → gate on
    0xA9, MUSIC_TRI_OFF,     // 2B: LDA #$10       (tri, gate off)
    0x8D, 0x04, 0xD4,       // 2D: STA $D404
    0xAE, 0x3A, 0x03,       // 30: LDX $033A      (note index)
    0xBD, 0xA0, 0x03,       // 33: LDA $03A0,X    (V1 freq_lo)
    0x8D, 0x00, 0xD4,       // 36: STA $D400
    0xBD, 0xD0, 0x03,       // 39: LDA $03D0,X    (V1 freq_hi)
    0x8D, 0x01, 0xD4,       // 3C: STA $D401
    0xA9, MUSIC_LEAD_AD,     // 3F: LDA #$A0       (AD: A=A, D=0)
    0x8D, 0x05, 0xD4,       // 41: STA $D405
    0xA9, MUSIC_LEAD_SR,     // 44: LDA #$A9       (SR: S=A, R=9)
    0x8D, 0x06, 0xD4,       // 46: STA $D406
    0xA9, MUSIC_TRI_ON,      // 49: LDA #$11       (tri + gate on)
    0x8D, 0x04, 0xD4,       // 4B: STA $D404

    // === V2 holdoff — decrement, reclaim on expiry ===
    // .v1_done:
    0xAD, 0x3C, 0x03,       // 4E: LDA $033C      (V2 holdoff)
    0xF0, 0x2B,              // 51: BEQ +43 → .v2_done (0x7E)
    0x38,                    // 53: SEC
    0xE9, 0x01,              // 54: SBC #1
    0x8D, 0x3C, 0x03,       // 56: STA $033C
    0xD0, 0x23,              // 59: BNE +35 → .v2_done (0x7E)
    // Reclaim V2: gate off → freq + ADSR → gate on
    0xA9, MUSIC_TRI_OFF,     // 5B: LDA #$10       (tri, gate off)
    0x8D, 0x0B, 0xD4,       // 5D: STA $D40B
    0xAE, 0x3A, 0x03,       // 60: LDX $033A      (note index)
    0xBD, 0x00, 0x02,       // 63: LDA $0200,X    (V2 freq_lo)
    0x8D, 0x07, 0xD4,       // 66: STA $D407
    0xBD, 0x30, 0x02,       // 69: LDA $0230,X    (V2 freq_hi)
    0x8D, 0x08, 0xD4,       // 6C: STA $D408
    0xA9, MUSIC_AD,          // 6F: LDA #$80       (AD)
    0x8D, 0x0C, 0xD4,       // 71: STA $D40C
    0xA9, MUSIC_SR,          // 74: LDA #$F6       (SR)
    0x8D, 0x0D, 0xD4,       // 76: STA $D40D
    0xA9, MUSIC_TRI_ON,      // 79: LDA #$11       (tri + gate on)
    0x8D, 0x0B, 0xD4,       // 7B: STA $D40B

    // === Music tick — rest-aware note stepping ===
    // .v2_done:
    0xCE, 0x39, 0x03,       // 7E: DEC $0339      (tick counter)
    0xD0, 0x5C,              // 81: BNE +92 → .ack (0xDF)
    // Tick expired — reload counter and update note frequencies
    0xA9, MUSIC_TEMPO,       // 83: LDA #50        (tempo reload)
    0x8D, 0x39, 0x03,       // 85: STA $0339
    0xAE, 0x3A, 0x03,       // 88: LDX $033A      (note index)
    // V3 (bass): always update — no rests in bass table
    0xBD, 0x40, 0x03,       // 8B: LDA $0340,X    (V3 freq_lo)
    0x8D, 0x0E, 0xD4,       // 8E: STA $D40E
    0xBD, 0x70, 0x03,       // 91: LDA $0370,X    (V3 freq_hi)
    0x8D, 0x0F, 0xD4,       // 94: STA $D40F
    // V1 (lead): rest-aware — freq_hi==0 gates off, nonzero gates on
    0xAD, 0x3B, 0x03,       // 97: LDA $033B      (V1 holdoff)
    0xD0, 0x1A,              // 9A: BNE +26 → .skip_v1 (0xB6)
    0xBD, 0xD0, 0x03,       // 9C: LDA $03D0,X    (V1 freq_hi)
    0xF0, 0x10,              // 9F: BEQ +16 → .v1_rest (0xB1)
    0x8D, 0x01, 0xD4,       // A1: STA $D401      (freq_hi)
    0xBD, 0xA0, 0x03,       // A4: LDA $03A0,X    (V1 freq_lo)
    0x8D, 0x00, 0xD4,       // A7: STA $D400      (freq_lo)
    0xA9, MUSIC_TRI_ON,      // AA: LDA #$11       (tri + gate on)
    0x8D, 0x04, 0xD4,       // AC: STA $D404
    0xD0, 0x05,              // AF: BNE +5 → .skip_v1 (0xB6)
    // .v1_rest:
    0xA9, MUSIC_TRI_OFF,     // B1: LDA #$10       (tri, gate off)
    0x8D, 0x04, 0xD4,       // B3: STA $D404
    // .skip_v1:
    // V2 (pad): rest-aware — freq_hi==0 gates off, nonzero gates on
    0xAD, 0x3C, 0x03,       // B6: LDA $033C      (V2 holdoff)
    0xD0, 0x1A,              // B9: BNE +26 → .skip_v2 (0xD5)
    0xBD, 0x30, 0x02,       // BB: LDA $0230,X    (V2 freq_hi)
    0xF0, 0x10,              // BE: BEQ +16 → .v2_rest (0xD0)
    0x8D, 0x08, 0xD4,       // C0: STA $D408      (freq_hi)
    0xBD, 0x00, 0x02,       // C3: LDA $0200,X    (V2 freq_lo)
    0x8D, 0x07, 0xD4,       // C6: STA $D407      (freq_lo)
    0xA9, MUSIC_TRI_ON,      // C9: LDA #$11       (tri + gate on)
    0x8D, 0x0B, 0xD4,       // CB: STA $D40B
    0xD0, 0x05,              // CE: BNE +5 → .skip_v2 (0xD5)
    // .v2_rest:
    0xA9, MUSIC_TRI_OFF,     // D0: LDA #$10       (tri, gate off)
    0x8D, 0x0B, 0xD4,       // D2: STA $D40B
    // .skip_v2:
    // Advance note index (wraps at 32)
    0xE8,                    // D5: INX
    0xE0, MUSIC_SEQ_LEN,    // D6: CPX #32
    0x90, 0x02,              // D8: BCC +2 → .store_idx (0xDC)
    0xA2, 0x00,              // DA: LDX #0         (wrap)
    // .store_idx:
    0x8E, 0x3A, 0x03,       // DC: STX $033A

    // === Ack + restore ===
    // .ack:
    0xA9, 0xFF,              // DF: LDA #$FF
    0x8D, 0x19, 0xD0,       // E1: STA $D019      (clear VIC-II IRQ)
    0xAD, 0x0D, 0xDC,       // E4: LDA $DC0D      (ack CIA)
    0xAE, 0x37, 0x03,       // E7: LDX $0337      (restore X)
    0xAD, 0x36, 0x03,       // EA: LDA $0336      (restore A)
    0x40,                    // ED: RTI
];

/// Hardware-level music init: configure SID voices, copy note tables,
/// install IRQ handler. Does NOT touch $D418 (volume/filter mode) —
/// the caller sets volume to avoid pops during fades.
fn music_start_hw() {
    unsafe {
        // --- Configure Voice 1 (lead — triangle, slow attack) ---
        // Step 0 is a rest — configure ADSR but leave gated off.
        // The handler will gate on when the lead enters at step 6.
        poke(SID_V1_CTRL, MUSIC_TRI_OFF);          // gate off (rest)
        poke(SID_V1_AD, MUSIC_LEAD_AD);             // 500ms attack
        poke(SID_V1_SR, MUSIC_LEAD_SR);             // 67% sustain, 750ms release

        // --- Configure Voice 2 (pad — triangle wave) ---
        poke(SID_V2_CTRL, MUSIC_TRI_OFF);          // gate off first
        poke(SID_V2_FREQ_LO, NOTE_TABLES[192]);    // V2 freq_lo[0]
        poke(SID_V2_FREQ_HI, NOTE_TABLES[240]);    // V2 freq_hi[0]
        poke(SID_V2_AD, MUSIC_AD);
        poke(SID_V2_SR, MUSIC_SR);
        poke(SID_V2_CTRL, MUSIC_TRI_ON);           // gate on

        // --- Configure Voice 3 (bass — triangle wave) ---
        poke(SID_V3_CTRL, MUSIC_TRI_OFF);
        poke(SID_V3_FREQ_LO, NOTE_TABLES[0]);      // V3 freq_lo[0]
        poke(SID_V3_FREQ_HI, NOTE_TABLES[48]);     // V3 freq_hi[0]
        poke(SID_V3_AD, MUSIC_AD);
        poke(SID_V3_SR, MUSIC_SR);
        poke(SID_V3_CTRL, MUSIC_TRI_ON);

        // --- Low-pass filter on Voice 3 for warm bass ---
        poke(SID_FILTER_LO, 0x00);                 // cutoff lo (bits 0-2)
        poke(SID_FILTER_HI, 0x30);                 // cutoff hi → ~$300
        poke(SID_FILTER_ROUTE, 0x44);              // resonance=4, route V3
        // NOTE: $D418 (SID_VOL) intentionally NOT set here

        // --- Copy note tables to RAM ---
        let src = NOTE_TABLES.as_ptr();
        // V3 + V1 tables → $0340-$03FF (192 bytes)
        let dst_a = 0x0340 as *mut u8;
        for i in 0..192usize {
            write_volatile(dst_a.add(i), *src.add(i));
        }
        // V2 tables → $0200-$025F (96 bytes)
        let dst_b = 0x0200 as *mut u8;
        for i in 0..96usize {
            write_volatile(dst_b.add(i), *src.add(192 + i));
        }

        // --- Initialize music state ---
        write_volatile(0x0339 as *mut u8, MUSIC_TEMPO); // tick counter
        write_volatile(0x033A as *mut u8, 0);            // note index
        write_volatile(0x033B as *mut u8, 0);            // V1 holdoff
        write_volatile(0x033C as *mut u8, 0);            // V2 holdoff
        write_volatile(0x0338 as *mut u8, 0);            // shake counter

        // --- Install music IRQ handler ---
        let handler_addr = core::ptr::addr_of!(MUSIC_HANDLER) as u16;
        write_volatile(0xFFFE as *mut u8, (handler_addr & 0xFF) as u8);
        write_volatile(0xFFFF as *mut u8, (handler_addr >> 8) as u8);

        // Configure VIC-II raster interrupt at line 251 (vblank)
        poke(VIC_RASTER, 251);
        let ctrl1 = peek(VIC_CTRL1 as *const u8);
        poke(VIC_CTRL1, ctrl1 & 0x7F); // clear raster bit 8

        // Acknowledge all pending interrupt sources
        poke(VIC_IRQ_STATUS, 0xFF);
        let _ = peek(CIA1_ICR as *const u8);
        let _ = peek(CIA2_ICR as *const u8);

        // Enable VIC-II raster interrupt — music starts on next vblank
        poke(VIC_IRQ_MASK, 0x01);
    }
}

/// Start the 3-voice ambient music engine at full volume.
///
/// Configures all three SID voices with triangle wave and music ADSR,
/// copies note frequency tables to $0340-$03FF, and installs the combined
/// music + shake IRQ handler on the VBlank raster interrupt.
///
/// Must not overlap with spinner (they share register save slots and the
/// IRQ vector). Call music_stop() before spinner_start().
#[inline(never)]
pub fn music_start() {
    music_start_hw();
    poke(SID_VOL, 0x1F); // low-pass mode + vol=15
    unsafe {
        // Start intermittent music auto-cycle
        MUSIC_PHASE = MUSIC_PLAYING;
        MUSIC_TIMER = MUSIC_PLAY_FRAMES;
    }
}

/// Hardware-level music teardown: disable IRQ, gate off voices, clear filter
/// routing. Does NOT touch $D418 (volume/filter mode) — the caller manages
/// volume to avoid pops during fades.
fn stop_sid_quiet() {
    // Disable VIC-II raster interrupt
    poke(VIC_IRQ_MASK, 0x00);

    // Acknowledge pending + clear CIA latches
    poke(VIC_IRQ_STATUS, 0xFF);
    let _ = peek(CIA1_ICR as *const u8);
    let _ = peek(CIA2_ICR as *const u8);

    // Gate off all voices — ADSR release phase silences them
    poke(SID_V1_CTRL, MUSIC_TRI_OFF);
    poke(SID_V2_CTRL, MUSIC_TRI_OFF);
    poke(SID_V3_CTRL, MUSIC_TRI_OFF);

    // Clear filter routing (leave $D418 untouched)
    poke(SID_FILTER_ROUTE, 0x00);

    // Restore IRQ vector to safe RTI stub
    unsafe {
        write_volatile(0xFFFE as *mut u8, 0x00); // $E000 low
        write_volatile(0xFFFF as *mut u8, 0xE0); // $E000 high
    }
}

/// Stop the ambient music engine.
///
/// Disables the raster IRQ, gates off all three voices (triggering their
/// release phase for a smooth fade), resets the filter, and restores the
/// IRQ vector to the RTI stub at $E000.
///
/// Also disables the intermittent music timer so `music_auto_tick()` is inert
/// until the next `music_start()`.
#[inline(never)]
pub fn music_stop() {
    stop_sid_quiet();
    poke(SID_VOL, 0x0F); // no filter + vol 15 (for SFX during silence)
    unsafe {
        MUSIC_PHASE = MUSIC_OFF;
        MUSIC_TIMER = 0;
    }
}

/// Called each frame from input wait loops. Manages intermittent music
/// with smooth volume fades: play one loop → fade out (3s) → silence
/// (60-123s) → fade in (3s) → repeat.
///
/// Fades use the SID master volume register ($D418) to ramp 0-15 over
/// 15 steps at 10 frames/step (3.0 seconds). Music continues playing
/// during both fades — volume alone controls audibility.
///
/// Volume-safe: never jumps $D418 abruptly. Uses `stop_sid_quiet()` (no
/// volume change) for teardown and `music_start_hw()` (no volume set) for
/// init, so the only $D418 writes are single-step increments/decrements.
pub fn music_auto_tick() {
    unsafe {
        match MUSIC_PHASE {
            MUSIC_PLAYING => {
                if MUSIC_TIMER > 0 {
                    MUSIC_TIMER -= 1;
                } else {
                    // Play period ended → begin fade out
                    MUSIC_PHASE = MUSIC_FADE_OUT;
                    MUSIC_FADE_VOL = 15;
                    MUSIC_FADE_TICK = MUSIC_FADE_STEP;
                }
            }
            MUSIC_FADE_OUT => {
                if MUSIC_FADE_TICK > 0 {
                    MUSIC_FADE_TICK -= 1;
                } else if MUSIC_FADE_VOL > 0 {
                    MUSIC_FADE_VOL -= 1;
                    poke(SID_VOL, 0x10 | MUSIC_FADE_VOL); // low-pass + volume
                    MUSIC_FADE_TICK = MUSIC_FADE_STEP;
                } else {
                    // Fade complete at vol 0 — quiet teardown (no volume jump)
                    stop_sid_quiet();
                    MUSIC_PHASE = MUSIC_SILENT;
                    let random = (peek(CIA1_TIMER_LO) as u16) & 0x3F; // 0-63
                    MUSIC_TIMER = MUSIC_SILENCE_MIN + random * 50;
                }
            }
            MUSIC_SILENT => {
                if MUSIC_TIMER > 0 {
                    MUSIC_TIMER -= 1;
                } else {
                    // Silence ended → start music at zero volume, fade in
                    // Set vol 0 BEFORE starting voices — no spike
                    poke(SID_VOL, 0x10); // low-pass + vol 0
                    music_start_hw();
                    MUSIC_PHASE = MUSIC_FADE_IN;
                    MUSIC_FADE_VOL = 0;
                    MUSIC_FADE_TICK = MUSIC_FADE_STEP;
                }
            }
            MUSIC_FADE_IN => {
                if MUSIC_FADE_TICK > 0 {
                    MUSIC_FADE_TICK -= 1;
                } else if MUSIC_FADE_VOL < 15 {
                    MUSIC_FADE_VOL += 1;
                    poke(SID_VOL, 0x10 | MUSIC_FADE_VOL); // low-pass + volume
                    MUSIC_FADE_TICK = MUSIC_FADE_STEP;
                } else {
                    // Fade in complete → normal play
                    MUSIC_PHASE = MUSIC_PLAYING;
                    MUSIC_TIMER = MUSIC_PLAY_FRAMES;
                }
            }
            _ => {} // MUSIC_OFF — no-op
        }
    }
}
