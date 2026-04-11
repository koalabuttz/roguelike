#![no_std]
#![no_main]

extern crate alloc;

mod allocator;

use core::arch::naked_asm;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

use roguelike_core::rules::command::GameCommand;
use roguelike_core::rules::direction::Direction;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH};
use roguelike_renderer3d::framebuffer::Framebuffer;
use roguelike_renderer3d::scene::render_scene;

#[global_allocator]
static ALLOC: allocator::BumpAlloc = allocator::BumpAlloc;

// ---------------------------------------------------------------------------
// ARM9 bootstrap with full CP15 initialization
// ---------------------------------------------------------------------------

/// ARM9 entry point. Initializes CP15 (caches, TCM), clears BSS, sets up
/// the stack in DTCM, then jumps to Rust main.
///
/// On the DS, the ARM9 starts with:
/// - MPU may or may not be configured (depends on boot path)
/// - DTCM/ITCM may not be at expected addresses
/// - Caches are typically disabled
/// - IRQs may be enabled with no handler set up
///
/// We must configure all of this before any Rust code can run.
#[unsafe(naked)]
#[no_mangle]
#[link_section = ".text._start"]
extern "C" fn _start() -> ! {
    naked_asm!(
        // Force ARM mode (not Thumb) for CP15 access
        ".arm",

        // Disable IRQs and FIQs
        "mrs r0, cpsr",
        "orr r0, r0, #0xC0",   // set I and F bits
        "msr cpsr_c, r0",

        // Disable caches and MPU before reconfiguring
        "mrc p15, 0, r0, c1, c0, 0",
        "bic r0, r0, #0x1",        // disable MPU
        "bic r0, r0, #0x4",        // disable D-cache
        "bic r0, r0, #0x1000",     // disable I-cache
        "bic r0, r0, #0x10000",    // disable DTCM
        "bic r0, r0, #0x40000",    // disable ITCM
        "mcr p15, 0, r0, c1, c0, 0",

        // Invalidate caches
        "mov r0, #0",
        "mcr p15, 0, r0, c7, c5, 0",  // invalidate I-cache
        "mcr p15, 0, r0, c7, c6, 0",  // invalidate D-cache

        // Set up DTCM at 0x0B000000, 16 KB
        // Format: base[31:12] | size[5:1] | enable[0]
        // 16 KB = 2^14, size field = (14-1)/2 ... actually:
        // CP15 c9,c1,0: bits[31:12]=base, bits[5:1]=size code
        // Size code for 16KB: size = 2^(code+9) → code = 5 → 16384
        "ldr r0, =0x0B00000A",    // base 0x0B000000 | size code 5 (16KB)
        "mcr p15, 0, r0, c9, c1, 0",

        // Set up ITCM at 0x00000000, 32 KB (size code 6)
        "mov r0, #0x0C",          // base 0x00000000 | size code 6 (32KB)
        "mcr p15, 0, r0, c9, c1, 1",

        // Re-enable DTCM and ITCM, enable caches, keep MPU disabled
        // (MPU disabled = all memory accessible, simplest for homebrew)
        "mrc p15, 0, r0, c1, c0, 0",
        "orr r0, r0, #0x10000",    // enable DTCM
        "orr r0, r0, #0x40000",    // enable ITCM
        "orr r0, r0, #0x4",        // enable D-cache
        "orr r0, r0, #0x1000",     // enable I-cache
        "mcr p15, 0, r0, c1, c0, 0",

        // Now DTCM is at 0x0B000000 — set stack to top of DTCM
        "ldr sp, =0x0B004000",

        // Disable ARM9 IRQ master enable (IME = 0)
        "ldr r0, =0x04000208",
        "mov r1, #0",
        "str r1, [r0]",

        // Clear BSS
        "ldr r0, =__bss_start",
        "ldr r1, =__bss_end",
        "mov r2, #0",
        "0:",
        "cmp r0, r1",
        "strlt r2, [r0], #4",
        "blt 0b",

        // Jump to Rust main
        "b main",
    )
}

// ---------------------------------------------------------------------------
// DS hardware registers
// ---------------------------------------------------------------------------

const REG_DISPCNT: *mut u32 = 0x0400_0000 as *mut u32;
const REG_POWCNT1: *mut u16 = 0x0400_0304 as *mut u16;
const REG_VRAMCNT_A: *mut u8 = 0x0400_0240 as *mut u8;
const REG_VCOUNT: *const u16 = 0x0400_0006 as *const u16;
const REG_KEYINPUT: *const u16 = 0x0400_0130 as *const u16;

/// VRAM bank A base address in LCDC mapping mode.
const VRAM_A: *mut u16 = 0x0680_0000 as *mut u16;

// DISPCNT display mode 2 = VRAM direct display (bits 16-17)
// VRAM block A = 0 (bits 18-19)
const DISPCNT_MODE_FB0: u32 = 2 << 16;

// VRAMCNT: enable (bit 7), MST = 0 (LCDC mode)
const VRAMCNT_LCDC: u8 = 0x80;

// POWCNT1 bits:
//   0: LCD power
//   1: Engine A (2D)
//   2: 3D rendering engine
//   3: 3D geometry engine
//   9: Engine B (2D)
//  15: Display swap (0 = Engine A top, 1 = Engine A bottom)
// Enable everything, Engine A on top (bit 15 = 0)
const POWCNT1_ENABLE: u16 = 0x020F; // LCD + 2D-A + 3D + 3D-geo + 2D-B, top

// Key masks (active-low register, we invert on read)
const KEY_A: u16 = 1 << 0;
const KEY_RIGHT: u16 = 1 << 4;
const KEY_LEFT: u16 = 1 << 5;
const KEY_UP: u16 = 1 << 6;
const KEY_DOWN: u16 = 1 << 7;

// ---------------------------------------------------------------------------
// Timer registers for frame timing measurement
// ---------------------------------------------------------------------------

const REG_TM0CNT_L: *mut u16 = 0x0400_0100 as *mut u16;
const REG_TM0CNT_H: *mut u16 = 0x0400_0102 as *mut u16;
const REG_TM1CNT_L: *mut u16 = 0x0400_0104 as *mut u16;
const REG_TM1CNT_H: *mut u16 = 0x0400_0106 as *mut u16;

// Timer control: enable (bit 7), cascade for timer 1 (bit 2)
const TIMER_ENABLE: u16 = 0x0080;
const TIMER_CASCADE: u16 = 0x0004;

// ---------------------------------------------------------------------------
// Game state (placement-constructed, same pattern as GBA)
// ---------------------------------------------------------------------------

/// Game state storage. UnsafeCell avoids the `static mut` reference warnings
/// while keeping the same placement-construction pattern as the GBA port.
struct GameSlot(UnsafeCell<MaybeUninit<CompactGameState>>);
// SAFETY: DS is single-threaded.
unsafe impl Sync for GameSlot {}

static GAME_SLOT: GameSlot = GameSlot(UnsafeCell::new(MaybeUninit::uninit()));

fn game() -> &'static mut CompactGameState {
    unsafe { &mut *(*GAME_SLOT.0.get()).as_mut_ptr() }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

fn init_display() {
    unsafe {
        // Power on LCD and 2D engine A
        REG_POWCNT1.write_volatile(POWCNT1_ENABLE);
        // Map VRAM bank A to LCDC (direct CPU access at 0x06800000)
        REG_VRAMCNT_A.write_volatile(VRAMCNT_LCDC);
        // Display mode 2: show VRAM bank A as raw 256x192 bitmap
        REG_DISPCNT.write_volatile(DISPCNT_MODE_FB0);
    }
}

/// Swizzle renderer RGB555 (R<<10|G<<5|B) to DS hardware format (B<<10|G<<5|R)
/// and set bit 15 (opaque pixel).
#[inline]
fn swizzle_rgb555(c: u16) -> u16 {
    let r = (c >> 10) & 0x1F;
    let g = (c >> 5) & 0x1F;
    let b = c & 0x1F;
    0x8000 | (b << 10) | (g << 5) | r
}

/// Copy the framebuffer's color data to VRAM bank A with channel swap.
fn blit_to_vram(fb: &Framebuffer) {
    let src = fb.color_slice();
    unsafe {
        for i in 0..src.len() {
            VRAM_A.add(i).write_volatile(swizzle_rgb555(src[i]));
        }
    }
}

fn wait_vblank() {
    unsafe {
        // Wait until we're in vblank (scanline >= 192)
        while REG_VCOUNT.read_volatile() < 192 {}
        // Wait until vblank ends (so we don't double-trigger)
        while REG_VCOUNT.read_volatile() >= 192 {}
    }
}

// ---------------------------------------------------------------------------
// Timer (cascaded 32-bit: TM0 counts at bus clock, TM1 cascades)
// ---------------------------------------------------------------------------

fn init_timers() {
    unsafe {
        // Timer 0: bus clock (~33.514 MHz), enable
        REG_TM0CNT_L.write_volatile(0);
        REG_TM0CNT_H.write_volatile(TIMER_ENABLE);
        // Timer 1: cascade from timer 0 (32-bit counter), enable
        REG_TM1CNT_L.write_volatile(0);
        REG_TM1CNT_H.write_volatile(TIMER_ENABLE | TIMER_CASCADE);
    }
}

fn read_timer32() -> u32 {
    unsafe {
        let lo = REG_TM0CNT_L.read_volatile() as u32;
        let hi = REG_TM1CNT_L.read_volatile() as u32;
        (hi << 16) | lo
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn read_keys() -> u16 {
    unsafe { !REG_KEYINPUT.read_volatile() & 0x03FF }
}

fn keys_to_command(keys: u16, prev: u16) -> Option<GameCommand> {
    let pressed = keys & !prev;

    if pressed & KEY_UP != 0 {
        Some(GameCommand::Move(Direction::North))
    } else if pressed & KEY_DOWN != 0 {
        Some(GameCommand::Move(Direction::South))
    } else if pressed & KEY_LEFT != 0 {
        Some(GameCommand::Move(Direction::West))
    } else if pressed & KEY_RIGHT != 0 {
        Some(GameCommand::Move(Direction::East))
    } else if pressed & KEY_A != 0 {
        Some(GameCommand::Descend)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Fill top rows of screen with red to signal a panic visually.
    unsafe {
        let red = 0x801F_u16; // DS format: bit15 + R in low bits
        for i in 0..(256 * 16) {
            VRAM_A.add(i).write_volatile(red);
        }
    }
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[no_mangle]
extern "C" fn main() -> ! {
    init_display();
    init_timers();

    // Placement-construct game state into static (avoids 6KB stack allocation)
    unsafe {
        CompactGameState::new_into((*GAME_SLOT.0.get()).as_mut_ptr(), 42, MAP_WIDTH, MAP_HEIGHT);
    }

    let mut fb = Framebuffer::new(256, 192);
    let mut frame: u32 = 0;
    let mut prev_keys: u16 = 0;

    loop {
        let keys = read_keys();
        if let Some(cmd) = keys_to_command(keys, prev_keys) {
            game().step_view(cmd);
        }
        prev_keys = keys;

        let t0 = read_timer32();
        render_scene(game(), &mut fb, frame);
        let t1 = read_timer32();
        let frame_ticks = t1.wrapping_sub(t0);

        wait_vblank();
        blit_to_vram(&fb);

        // Draw a frame counter in the top-left: one white pixel per frame rendered.
        // This lets you visually confirm the loop is advancing even if it's slow.
        // Also draw a bar proportional to frame time (1 pixel per ~1ms).
        unsafe {
            let frame_ms = frame_ticks / 33514; // ticks to milliseconds
            // Frame counter: row 0, one pixel per frame (wraps at 256)
            let x = (frame % 256) as usize;
            VRAM_A.add(x).write_volatile(0xFFFF); // white pixel
            // Frame time bar: row 1, width = milliseconds (capped at 256)
            let bar_len = (frame_ms as usize).min(256);
            let row1 = 256; // offset to row 1
            for i in 0..256 {
                let color = if i < bar_len { 0x801F } else { 0x8000 }; // red bar on black
                VRAM_A.add(row1 + i).write_volatile(color);
            }
        }

        frame = frame.wrapping_add(1);
    }
}
