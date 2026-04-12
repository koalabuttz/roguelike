#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod debug_hud;
#[cfg(not(feature = "software3d"))]
mod gpu_sink;
#[cfg(not(feature = "software3d"))]
mod gx;

use core::arch::naked_asm;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

use roguelike_core::rules::command::GameCommand;
use roguelike_core::rules::direction::Direction;
#[allow(unused_imports)]
use roguelike_core::rules::game_view::GameView;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH};
#[cfg(feature = "software3d")]
use roguelike_renderer3d::framebuffer::Framebuffer;
#[cfg(feature = "software3d")]
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

        // --- MPU region configuration (ARM946E-S §Cache and Write Buffer) ---
        //
        // ARM946 requires the MPU for cache functionality: the cacheable and
        // bufferable attributes live in per-region MPU state. Without any
        // regions configured, the caches are effectively bypassed even if
        // the cache enable bits are set in CP15 c1.
        //
        // Region register layout (c6, cN, 0):
        //   bits [31:12] = base address (aligned to region size)
        //   bits [5:1]   = size field (region size = 2^(field+1))
        //   bit  [0]     = enable
        //
        // Region priority: higher index wins on overlap. Region 0 is the
        // lowest-priority background catch-all; regions 1-3 are the
        // specific memory areas we care about.

        // Region 0: background 4 GB at 0x00000000 (catch-all)
        //   size field = 31 (2^32 = 4 GB), encoded 31<<1 = 0x3E + enable
        "ldr r0, =0x0000003F",
        "mcr p15, 0, r0, c6, c0, 0",

        // Region 1: main RAM 4 MB at 0x02000000
        //   size field = 21 (2^22 = 4 MB), encoded 21<<1 = 0x2A + enable
        "ldr r0, =0x0200002B",
        "mcr p15, 0, r0, c6, c1, 0",

        // Region 2: I/O 16 MB at 0x04000000 (hardware registers)
        //   size field = 23 (2^24 = 16 MB), encoded 23<<1 = 0x2E + enable
        "ldr r0, =0x0400002F",
        "mcr p15, 0, r0, c6, c2, 0",

        // Region 3: VRAM 8 MB at 0x06000000
        //   size field = 22 (2^23 = 8 MB), encoded 22<<1 = 0x2C + enable
        "ldr r0, =0x0600002D",
        "mcr p15, 0, r0, c6, c3, 0",

        // Regions 4-7: disabled (write zero to clear enable bit)
        "mov r0, #0",
        "mcr p15, 0, r0, c6, c4, 0",
        "mcr p15, 0, r0, c6, c5, 0",
        "mcr p15, 0, r0, c6, c6, 0",
        "mcr p15, 0, r0, c6, c7, 0",

        // Access permissions: ARM946E-S uses the **extended** AP format,
        // 4 bits per region × 8 regions = 32 bits total.
        //   0x0 = no access
        //   0x1 = privileged RW, user no access
        //   0x2 = privileged RW, user RO
        //   0x3 = privileged RW, user RW      ← what we use
        //   0x5 = privileged RO (GOTCHA: this is read-only, not priv-RW!)
        // Pattern: 0x3 repeated 8 times = 0x33333333 (all regions RW).
        // This is a correction from a prior `0x5555` value that made
        // main RAM read-only and caused the BSS-clear loop to data-abort.
        "ldr r0, =0x33333333",
        "mcr p15, 0, r0, c5, c0, 2",    // EDAP: data access permissions
        "mcr p15, 0, r0, c5, c0, 3",    // EIAP: instruction access permissions

        // Cacheable bits: one bit per region. Only region 1 (main RAM) is
        // cacheable — I/O must be strongly ordered, VRAM is shared with the
        // 3D engine and must not sit in the CPU cache, and region 0 is a
        // catch-all we want to be non-cacheable by default.
        "mov r0, #0x02",                // only region 1 bit set
        "mcr p15, 0, r0, c2, c0, 0",    // D-cacheable bits
        "mcr p15, 0, r0, c2, c0, 1",    // I-cacheable bits

        // Bufferable bits: main RAM can use the write buffer for faster
        // writes. Same single bit for region 1.
        "mcr p15, 0, r0, c3, c0, 0",    // write-buffer enable bits

        // Re-enable DTCM, ITCM, caches, AND the MPU.
        // Setting MPU=1 without any configured regions would fault the
        // first memory access; the configuration above must precede this.
        "mrc p15, 0, r0, c1, c0, 0",
        "orr r0, r0, #0x1",        // enable MPU (protection unit)
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

// Hardware 3D display configuration:
//   DISPCNT bits 0-2   = BG mode 0 (tile backgrounds only)
//   DISPCNT bit 3      = BG0 in 3D mode (Engine A only feature)
//   DISPCNT bit 8      = BG0 display enable
//   DISPCNT bits 16-17 = display mode 1 (graphics display)
// The 3D engine output is composited into BG0 when bit 3 is set.
#[cfg(not(feature = "software3d"))]
const DISPCNT_3D_MODE: u32 = (1 << 16) | (1 << 8) | (1 << 3);

// VRAMCNT_A: enable (bit 7), MST = 1 (Engine A BG), offset = 0 → bank A
// is mapped as Engine A BG slot 0 at 0x06000000.
#[cfg(not(feature = "software3d"))]
const VRAMCNT_ENGINE_A_BG: u8 = 0x81;

// BG0 control register (Engine A). In 3D mode only priority matters —
// tile/map bases are ignored because the 3D engine provides pixels.
#[cfg(not(feature = "software3d"))]
const REG_BG0CNT: *mut u16 = 0x0400_0008 as *mut u16;

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
#[cfg(not(feature = "software3d"))]
const KEY_SELECT: u16 = 1 << 2; // used for fog tuning (hardware 3D only)
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
        // Power on LCD + 2D engine A + 3D render/geom + 2D engine B,
        // Engine A on top (POWCNT1 bit 15 = 0).
        REG_POWCNT1.write_volatile(POWCNT1_ENABLE);

        #[cfg(feature = "software3d")]
        {
            // Software 3D path: VRAM bank A → LCDC (direct CPU access
            // at 0x06800000), display mode 2 shows bank A as a raw
            // 256×192 RGB555 bitmap.
            REG_VRAMCNT_A.write_volatile(VRAMCNT_LCDC);
            REG_DISPCNT.write_volatile(DISPCNT_MODE_FB0);
        }

        #[cfg(not(feature = "software3d"))]
        {
            // Hardware 3D path: VRAM bank A → Engine A BG slot 0.
            // DISPCNT selects graphics display mode with BG0 enabled
            // in 3D mode — the 3D engine output feeds into BG0 and is
            // composited to the top screen.
            REG_VRAMCNT_A.write_volatile(VRAMCNT_ENGINE_A_BG);
            REG_DISPCNT.write_volatile(DISPCNT_3D_MODE);
            REG_BG0CNT.write_volatile(0); // priority 0 (highest)
        }
    }
}

/// Swizzle renderer RGB555 (R<<10|G<<5|B) to DS hardware format (B<<10|G<<5|R)
/// and set bit 15 (opaque pixel).
#[cfg(feature = "software3d")]
#[inline]
fn swizzle_rgb555(c: u16) -> u16 {
    let r = (c >> 10) & 0x1F;
    let g = (c >> 5) & 0x1F;
    let b = c & 0x1F;
    0x8000 | (b << 10) | (g << 5) | r
}

/// Copy the framebuffer's color data to VRAM bank A with channel swap.
#[cfg(feature = "software3d")]
fn blit_to_vram(fb: &Framebuffer) {
    let src = fb.color_slice();
    for (i, &pixel) in src.iter().enumerate() {
        unsafe {
            VRAM_A.add(i).write_volatile(swizzle_rgb555(pixel));
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

pub(crate) fn read_timer32() -> u32 {
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
// HUD formatting
// ---------------------------------------------------------------------------

/// Format frame ticks as two HUD rows:
///
/// - Row 0: `"FPS NN MS MMMM"` — total frame time (render + vblank +
///   swap + HUD update, everything between two main-loop iterations)
/// - Row 1: `"GEN MMMM"` — time spent inside `generate_map_geometry`
///   (all tile-walking + all `GpuSink::emit` work, i.e. everything the
///   hardware 3D render actually pays for)
///
/// The difference between the two is main-loop overhead outside the
/// rendering hot path. Helps bisect where the frame-time budget is
/// going.
fn update_hud_fps(frame_ticks: u32) {
    let ms = frame_ticks / 33514;
    // checked_div handles the ms=0 case (faster than a millisecond): we
    // cap the displayed FPS at 99 and fall back to 99 on divide-by-zero.
    let fps = 1000u32.checked_div(ms).unwrap_or(99).min(99);

    // Row 0: total frame time
    let mut row0 = [b' '; 16];
    let mut p = debug_hud::write_str(&mut row0, 0, b"FPS ");
    p = debug_hud::write_u32_dec(&mut row0, p, fps);
    p = debug_hud::write_str(&mut row0, p, b" MS ");
    let _ = debug_hud::write_u32_dec(&mut row0, p, ms);
    debug_hud::write_text(0, 0, &row0);

    // Row 1: generate_map_geometry time (rendering hot path only).
    // Only meaningful in the hardware-3D path where gpu_sink tracks it.
    #[cfg(not(feature = "software3d"))]
    {
        let gen_ms = gpu_sink::last_gen_ticks() / 33514;
        let mut row1 = [b' '; 16];
        let p = debug_hud::write_str(&mut row1, 0, b"GEN ");
        let _ = debug_hud::write_u32_dec(&mut row1, p, gen_ms);
        debug_hud::write_text(0, 1, &row1);

        // Row 2: fog tuning parameters (Select + d-pad to adjust).
        let mut row2 = [b' '; 16];
        let mut p = debug_hud::write_str(&mut row2, 0, b"FOG ");
        p = debug_hud::write_u16_hex(&mut row2, p, gpu_sink::fog_offset() as u16);
        p = debug_hud::write_str(&mut row2, p, b" SH ");
        let _ = debug_hud::write_u32_dec(&mut row2, p, gpu_sink::fog_shift());
        debug_hud::write_text(0, 2, &row2);
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Fill the top rows of the screen with red to signal a panic
    // visually. In hardware-3D mode, bank A is mapped as Engine A BG
    // (not LCDC), so we first switch back to VRAM direct bitmap mode
    // so the panic indicator is actually visible regardless of which
    // build is running.
    unsafe {
        REG_VRAMCNT_A.write_volatile(VRAMCNT_LCDC);
        REG_DISPCNT.write_volatile(DISPCNT_MODE_FB0);
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
    debug_hud::init();
    init_timers();

    // Initialise the DS 3D engine (hardware path only). Must come
    // after init_display() which powers on the 3D engine bits in
    // POWCNT1.
    #[cfg(not(feature = "software3d"))]
    gx::init();

    // Placement-construct game state into static (avoids 6KB stack allocation)
    unsafe {
        CompactGameState::new_into((*GAME_SLOT.0.get()).as_mut_ptr(), 42, MAP_WIDTH, MAP_HEIGHT);
    }

    // Software path keeps a main-RAM framebuffer + per-frame blit.
    // Hardware path renders directly into BG0 via the 3D engine.
    #[cfg(feature = "software3d")]
    let mut fb = Framebuffer::new(256, 192);

    let mut frame: u32 = 0;
    let mut prev_keys: u16 = 0;

    loop {
        let keys = read_keys();

        // Select + d-pad: fog parameter tuning (hardware path only).
        // Holding SELECT hijacks the d-pad for fog controls instead of
        // player movement. This avoids a rebuild/flash cycle per tuning
        // iteration on real DS hardware.
        #[cfg(not(feature = "software3d"))]
        if keys & KEY_SELECT != 0 {
            let pressed = keys & !prev_keys;
            if pressed & KEY_UP != 0 {
                let v = gpu_sink::fog_offset().saturating_add(0x100).min(0x7F00);
                gpu_sink::set_fog_offset(v);
            } else if pressed & KEY_DOWN != 0 {
                let v = gpu_sink::fog_offset().saturating_sub(0x100);
                gpu_sink::set_fog_offset(v);
            } else if pressed & KEY_RIGHT != 0 {
                let v = gpu_sink::fog_shift().saturating_add(1).min(10);
                gpu_sink::set_fog_shift(v);
            } else if pressed & KEY_LEFT != 0 {
                let v = gpu_sink::fog_shift().saturating_sub(1);
                gpu_sink::set_fog_shift(v);
            }
        } else if let Some(cmd) = keys_to_command(keys, prev_keys) {
            game().step_view(cmd);
        }

        // Software path: always dispatch normally (no fog tuning).
        #[cfg(feature = "software3d")]
        if let Some(cmd) = keys_to_command(keys, prev_keys) {
            game().step_view(cmd);
        }

        prev_keys = keys;

        let t0 = read_timer32();

        #[cfg(feature = "software3d")]
        render_scene(game(), &mut fb, frame);
        #[cfg(not(feature = "software3d"))]
        gpu_sink::render_scene_ds(game(), frame);

        let t1 = read_timer32();
        let frame_ticks = t1.wrapping_sub(t0);

        // Display "FPS NN MS MMMM" on Engine B (top screen). Works in
        // both hardware and software 3D paths — the HUD lives on Engine
        // B regardless of which engine the game is on.
        update_hud_fps(frame_ticks);

        wait_vblank();

        #[cfg(feature = "software3d")]
        blit_to_vram(&fb);
        #[cfg(not(feature = "software3d"))]
        unsafe {
            gx::swap_buffers();
        }

        // Software path draws a debug HUD directly into VRAM bank A:
        // row 0 = frame counter (one white pixel per frame), row 1 =
        // frame time bar (red, width in ms). Hardware path can't do
        // this because bank A is mapped as Engine A BG, not LCDC.
        // Phase 4 will add a proper HUD on the bottom screen.
        #[cfg(feature = "software3d")]
        unsafe {
            let frame_ms = frame_ticks / 33514; // ticks to milliseconds
            let x = (frame % 256) as usize;
            VRAM_A.add(x).write_volatile(0xFFFF); // white pixel
            let bar_len = (frame_ms as usize).min(256);
            let row1 = 256; // offset to row 1
            for i in 0..256 {
                let color = if i < bar_len { 0x801F } else { 0x8000 };
                VRAM_A.add(row1 + i).write_volatile(color);
            }
        }

        frame = frame.wrapping_add(1);
    }
}
