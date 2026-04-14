//! DS touchscreen input: reads ARM7 shared memory, calibrates to screen
//! coordinates, and maps taps to world tiles.
//!
//! The ARM7 polls the TSC (touchscreen controller) via SPI and writes
//! raw 12-bit ADC coordinates to shared memory at [`SHMEM_BASE`].
//! This module reads that data, applies a linear calibration to convert
//! ADC values to screen pixels, and maps screen pixels to world tile
//! coordinates using the automap viewport offset.

use roguelike_core::rules::command::GameCommand;

use crate::automap;

// Shared memory layout (written by ARM7, read by ARM9):
//   +0x00: u16 raw_x      (12-bit ADC, 0-4095)
//   +0x02: u16 raw_y      (12-bit ADC, 0-4095)
//   +0x04: u16 pen_down   (1=touching, 0=not)
//   +0x06: u16 counter    (diagnostic loop counter)
//
// Placed in .shared section so the linker assigns the address within our
// binary's footprint. The ARM7 gen_arm7.py SHMEM constant must match.
#[link_section = ".shared"]
#[used]
static TOUCH_SHMEM: [u16; 4] = [0; 4];

fn shmem_base() -> *const u16 {
    TOUCH_SHMEM.as_ptr()
}
const TOUCH_RAW_X_OFF: usize = 0;
const TOUCH_RAW_Y_OFF: usize = 1;
const TOUCH_PEN_DOWN_OFF: usize = 2;

/// ADC calibration ranges (from GBATEK gbatek.txt:11457-11461).
///
/// X-Position: "somewhat 100h..ED0h" (256..3792)
/// Y-Position: "somewhat 0B0h..F20h" (176..3872)
///
/// These are typical values; individual DS units vary. Tune on real
/// hardware by tapping corners and reading raw ADC via the debug HUD.
const ADC_X_MIN: u16 = 0x100;
const ADC_X_MAX: u16 = 0xED0;
const ADC_Y_MIN: u16 = 0x0B0;
const ADC_Y_MAX: u16 = 0xF20;

/// Bottom screen dimensions in pixels.
const SCREEN_W: u32 = 256;
const SCREEN_H: u32 = 192;

/// Tile size in pixels (8x8 tiles on Engine B).
const TILE_PX: usize = 8;

/// Invalidate the D-cache line containing the shared memory.
///
/// The ARM7 writes touch data directly to main RAM (no cache). The ARM9
/// has D-cache enabled for main RAM, so without invalidation it reads
/// stale cached values. This flushes the specific cache line so the
/// next read fetches fresh data from RAM.
#[inline(always)]
fn invalidate_shmem_cache() {
    unsafe {
        let addr = shmem_base() as u32;
        // mcr p15, 0, Rd, c7, c6, 1 = Invalidate D-cache single entry (MVA)
        // ARM946E-S cache line = 32 bytes; our 8-byte struct fits in one line.
        core::arch::asm!(
            "mcr p15, 0, {0}, c7, c6, 1",
            in(reg) addr,
            options(nomem, nostack),
        );
    }
}

/// Read raw touch data from ARM7 shared memory.
///
/// Returns `Some((raw_x, raw_y))` if the pen is touching, `None` otherwise.
pub fn read_raw() -> Option<(u16, u16)> {
    invalidate_shmem_cache();
    unsafe {
        let base = shmem_base();
        if base.add(TOUCH_PEN_DOWN_OFF).read_volatile() != 0 {
            let rx = base.add(TOUCH_RAW_X_OFF).read_volatile();
            let ry = base.add(TOUCH_RAW_Y_OFF).read_volatile();
            Some((rx, ry))
        } else {
            None
        }
    }
}

/// Address of the shared memory region (for diagnostic display).
pub fn shmem_addr() -> usize {
    TOUCH_SHMEM.as_ptr() as usize
}

/// Calibrate raw 12-bit ADC values to screen pixel coordinates.
///
/// Uses a linear mapping from the ADC range to 0..255 (X) and 0..191 (Y).
/// Values outside the expected ADC range are clamped.
fn calibrate(raw_x: u16, raw_y: u16) -> (u16, u16) {
    let clamped_x = raw_x.clamp(ADC_X_MIN, ADC_X_MAX) - ADC_X_MIN;
    let clamped_y = raw_y.clamp(ADC_Y_MIN, ADC_Y_MAX) - ADC_Y_MIN;
    let range_x = (ADC_X_MAX - ADC_X_MIN) as u32;
    let range_y = (ADC_Y_MAX - ADC_Y_MIN) as u32;

    let sx = (clamped_x as u32 * (SCREEN_W - 1) / range_x) as u16;
    let sy = (clamped_y as u32 * (SCREEN_H - 1) / range_y) as u16;
    (sx.min(SCREEN_W as u16 - 1), sy.min(SCREEN_H as u16 - 1))
}

/// Read touch with edge detection.
///
/// Returns `(current_pen_down, Option<calibrated_screen_pixel>)`.
/// The `Option` is `Some` only on the pen-DOWN transition (rising edge),
/// not while the pen is held. This prevents repeated commands from a
/// single tap.
pub fn read_edge(prev_pen_down: bool) -> (bool, Option<(u16, u16)>) {
    match read_raw() {
        Some((rx, ry)) => {
            if prev_pen_down {
                (true, None) // pen held — no new event
            } else {
                (true, Some(calibrate(rx, ry))) // pen just touched down
            }
        }
        None => (false, None),
    }
}

/// Convert a screen pixel coordinate to a world tile coordinate.
///
/// Returns `None` if the tap falls outside the automap area (rows 0-19),
/// i.e. on the status bar (row 20) or message log (rows 21-23).
pub fn screen_to_world(
    screen_x: u16,
    screen_y: u16,
    view_x: usize,
    view_y: usize,
) -> Option<(i32, i32)> {
    let tile_col = screen_x as usize / TILE_PX;
    let tile_row = screen_y as usize / TILE_PX;

    if tile_row >= automap::MAP_ROWS || tile_col >= automap::MAP_COLS {
        return None;
    }

    let world_x = (view_x + tile_col) as i32;
    let world_y = (view_y + tile_row) as i32;
    Some((world_x, world_y))
}

/// Button bar row on Engine B (tile row 23 = pixel y 184..191).
const BUTTON_ROW_PX: u16 = 23 * 8;

/// Check if a screen tap hit a touch button on row 23.
///
/// Returns the corresponding `GameCommand` if a button was hit, `None` if
/// the tap was outside the button bar. Column ranges match the labels
/// rendered by `hud::render_button_bar()`.
///
/// ```text
/// Col: 0-2   5-6   9-10  13-15  18-20  23-25
///      INV    LK    WT    RUN    EXP    MSG
/// ```
pub fn screen_to_button(screen_x: u16, screen_y: u16) -> Option<GameCommand> {
    if screen_y < BUTTON_ROW_PX {
        return None;
    }
    let col = screen_x as usize / TILE_PX;
    match col {
        0..=3 => Some(GameCommand::OpenInventory),
        4..=7 => Some(GameCommand::Look),
        8..=11 => Some(GameCommand::Wait),
        12..=16 => None, // RUN — handled by caller (needs direction)
        17..=21 => Some(GameCommand::AutoExplore),
        22..=26 => Some(GameCommand::MessageHistory),
        _ => None,
    }
}

/// Check if a screen tap hit the RUN button (cols 12-16 on row 23).
pub fn is_run_button(screen_x: u16, screen_y: u16) -> bool {
    screen_y >= BUTTON_ROW_PX && {
        let col = screen_x as usize / TILE_PX;
        (12..=16).contains(&col)
    }
}
