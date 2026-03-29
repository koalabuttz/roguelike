//! KERNAL disk I/O for C64 save/load.
//!
//! Uses KERNAL SAVE ($FFD8) and LOAD ($FFD5) for raw memory-range I/O.
//! These handle the entire IEC serial bus protocol in a single call —
//! no byte-by-byte CHROUT/CHRIN, no channel management, no CR issues.
//!
//! **File format:** PRG (default on 1541). SAVE writes a 2-byte load
//! address header + data. LOAD with SA=0 reads the header, discards it,
//! and loads data to the specified address.
//!
//! **IRQ handling:** KERNAL disk routines need their default IRQ
//! environment. We save/restore the IRQ vector AND reset the CIA1
//! timer to the KERNAL default (keyboard scan at 60Hz) so LOAD/SAVE
//! don't hang on serial bus timing.
//!
//! **Banking:** $36 = KERNAL + I/O, no BASIC. Keeps $A000-$BFFF as
//! RAM so SAVE_BUF (which spans into that range) is accessible.

use core::arch::asm;
use crate::c64::{poke, peek, CPU_PORT};

// CBM DOS file parameters.
const DEVICE: u8 = 8;

// Filenames (PETSCII).
const SAVE_NAME: &[u8] = b"@0:SAVE";  // PRG, atomic replace
const LOAD_NAME: &[u8] = b"SAVE";      // Just the name for LOAD
const SCRATCH_NAME: &[u8] = b"S0:SAVE"; // Command channel scratch

// KERNAL IRQ default: $EA31 (keyboard scan + cursor blink)
const KERNAL_IRQ: u16 = 0xEA31;

// Hardware addresses.
const IRQ_VEC: *mut u8 = 0x0314 as *mut u8;    // Software IRQ vector (lo)
const CIA1_CTRL_A: *mut u8 = 0xDC0E as *mut u8; // CIA1 Timer A control
const CIA1_TIMER_LO: *mut u8 = 0xDC04 as *mut u8;
const CIA1_TIMER_HI: *mut u8 = 0xDC05 as *mut u8;
const VIC_CTRL1: *mut u8 = 0xD011 as *mut u8;  // VIC control register 1
const VIC_RASTER: *mut u8 = 0xD012 as *mut u8;  // VIC raster compare
const VIC_IRQ_EN: *mut u8 = 0xD01A as *mut u8;  // VIC IRQ enable mask

// ZP $FB/$FC are free for user programs on C64 (KERNAL doesn't use them).
// $9A is KERNAL DFLTO (default output device) — clobbered by SAVE, restore after.
const ZP_PTR_ADDR: *mut u8 = 0xFB as *mut u8;

// ---------------------------------------------------------------------------
// KERNAL wrappers
// ---------------------------------------------------------------------------

fn kernal_setlfs(lfn: u8, device: u8, sa: u8) {
    unsafe {
        asm!(
            "jsr $FFBA",
            in("a") lfn,
            in("x") device,
            in("y") sa,
            clobber_abi("C"),
        );
    }
}

fn kernal_setnam(name: &[u8]) {
    let len = name.len() as u8;
    let ptr = name.as_ptr() as u16;
    unsafe {
        asm!(
            "jsr $FFBD",
            in("a") len,
            in("x") ptr as u8,
            in("y") (ptr >> 8) as u8,
            clobber_abi("C"),
        );
    }
}

/// KERNAL SAVE ($FFD8): save memory range to file.
/// A = ZP pointer to start address, X/Y = end address + 1.
/// Returns true on success (carry clear).
fn kernal_save(end_lo: u8, end_hi: u8) -> bool {
    let carry: u8;
    unsafe {
        asm!(
            "lda #$FB",     // ZP pointer address (hardcoded)
            "jsr $FFD8",
            "bcc 1f",
            "lda #1",
            "bne 2f",
            "1: lda #0",
            "2:",
            in("x") end_lo,
            in("y") end_hi,
            out("a") carry,
            clobber_abi("C"),
        );
    }
    carry == 0
}

/// KERNAL LOAD ($FFD5): load file into memory.
/// A = 0 (load), X/Y = destination address (when SA=0).
/// Returns (success, end_lo, end_hi).
fn kernal_load(dest_lo: u8, dest_hi: u8) -> (bool, u8, u8) {
    let carry: u8;
    let end_lo: u8;
    let end_hi: u8;
    unsafe {
        asm!(
            "lda #0",       // 0 = load (not verify)
            "jsr $FFD5",
            "bcc 1f",
            "lda #1",
            "bne 2f",
            "1: lda #0",
            "2:",
            in("x") dest_lo,
            in("y") dest_hi,
            out("a") carry,
            lateout("x") end_lo,
            lateout("y") end_hi,
            clobber_abi("C"),
        );
    }
    (carry == 0, end_lo, end_hi)
}

fn kernal_open() -> bool {
    let carry: u8;
    unsafe {
        asm!(
            "jsr $FFC0",
            "bcc 1f",
            "lda #1",
            "bne 2f",
            "1: lda #0",
            "2:",
            out("a") carry,
            clobber_abi("C"),
        );
    }
    carry == 0
}

fn kernal_close(lfn: u8) {
    unsafe {
        asm!(
            "jsr $FFC3",
            in("a") lfn,
            clobber_abi("C"),
        );
    }
}

fn kernal_clrchn() {
    unsafe {
        asm!(
            "jsr $FFCC",
            clobber_abi("C"),
        );
    }
}

// ---------------------------------------------------------------------------
// IRQ + banking environment for KERNAL I/O
// ---------------------------------------------------------------------------

// Additional hardware registers that KERNAL I/O may clobber.
const VIC_MEMPTR: *mut u8 = 0xD018 as *mut u8;  // VIC memory setup
const KERNAL_MSGFLAG: *mut u8 = 0x9D as *mut u8; // KERNAL message control

struct SavedIrqState {
    irq_lo: u8,
    irq_hi: u8,
    cia_ctrl: u8,
    cia_timer_lo: u8,
    cia_timer_hi: u8,
    vic_ctrl1: u8,
    vic_raster: u8,
    vic_irq_en: u8,
    vic_memptr: u8,
}

/// Save our IRQ state, install KERNAL defaults, bank KERNAL in.
fn enter_kernal_io() -> SavedIrqState {
    unsafe { asm!("sei", options(nomem, nostack)); }

    let saved = SavedIrqState {
        irq_lo: peek(IRQ_VEC as *const u8),
        irq_hi: peek(unsafe { IRQ_VEC.add(1) } as *const u8),
        cia_ctrl: peek(CIA1_CTRL_A as *const u8),
        cia_timer_lo: peek(CIA1_TIMER_LO as *const u8),
        cia_timer_hi: peek(CIA1_TIMER_HI as *const u8),
        vic_ctrl1: peek(VIC_CTRL1 as *const u8),
        vic_raster: peek(VIC_RASTER as *const u8),
        vic_irq_en: peek(VIC_IRQ_EN as *const u8),
        vic_memptr: peek(VIC_MEMPTR as *const u8),
    };

    // Suppress KERNAL messages ("SAVING...", "OK", etc.)
    poke(KERNAL_MSGFLAG, 0x00);

    // Install KERNAL default IRQ vector.
    poke(IRQ_VEC, KERNAL_IRQ as u8);
    poke(unsafe { IRQ_VEC.add(1) }, (KERNAL_IRQ >> 8) as u8);

    // Reset CIA1 Timer A to KERNAL default (~60Hz keyboard scan).
    // NTSC: $4025 (16421), PAL: $4295 (17045). Use PAL.
    poke(CIA1_CTRL_A, 0x00);    // Stop timer
    poke(CIA1_TIMER_LO, 0x95);  // PAL default lo
    poke(CIA1_TIMER_HI, 0x42);  // PAL default hi
    poke(CIA1_CTRL_A, 0x11);    // Start timer, continuous, force load

    // Disable VIC raster IRQ (our game uses raster IRQs for music).
    poke(VIC_IRQ_EN, 0x00);

    // Bank KERNAL in, keep I/O, no BASIC.
    poke(CPU_PORT, 0x36);

    unsafe { asm!("cli", options(nomem, nostack)); }

    saved
}

/// Restore our IRQ state and banking.
fn leave_kernal_io(saved: SavedIrqState) {
    unsafe { asm!("sei", options(nomem, nostack)); }

    // Bank back to game default.
    poke(CPU_PORT, 0x35);

    // Restore CIA1 Timer A.
    poke(CIA1_CTRL_A, 0x00);
    poke(CIA1_TIMER_LO, saved.cia_timer_lo);
    poke(CIA1_TIMER_HI, saved.cia_timer_hi);
    poke(CIA1_CTRL_A, saved.cia_ctrl);

    // Restore VIC state.
    poke(VIC_MEMPTR, saved.vic_memptr);
    poke(VIC_CTRL1, saved.vic_ctrl1);
    poke(VIC_RASTER, saved.vic_raster);
    poke(VIC_IRQ_EN, saved.vic_irq_en);

    // Restore our IRQ vector.
    poke(IRQ_VEC, saved.irq_lo);
    poke(unsafe { IRQ_VEC.add(1) }, saved.irq_hi);

    unsafe { asm!("cli", options(nomem, nostack)); }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Write a byte buffer to the save file on disk using KERNAL SAVE.
/// Returns true on success.
#[inline(never)]
pub fn save_buf_to_disk(data: &[u8]) -> bool {
    let start = data.as_ptr() as u16;
    let end = start + data.len() as u16;

    // Store start address in ZP $FB/$FC for KERNAL SAVE.
    poke(ZP_PTR_ADDR, start as u8);
    poke(unsafe { ZP_PTR_ADDR.add(1) }, (start >> 8) as u8);

    let saved = enter_kernal_io();

    kernal_setlfs(1, DEVICE, 1);
    kernal_setnam(SAVE_NAME);
    let ok = kernal_save(end as u8, (end >> 8) as u8);
    kernal_clrchn();

    leave_kernal_io(saved);
    ok
}

/// Load save file into a buffer using KERNAL LOAD.
/// Returns number of bytes loaded, or None on error.
#[inline(never)]
pub fn load_buf_from_disk(buf: &mut [u8]) -> Option<usize> {
    let dest = buf.as_ptr() as u16;

    let saved = enter_kernal_io();

    // SA=0: load to address in X/Y (ignores PRG header in file).
    kernal_setlfs(0, DEVICE, 0);
    kernal_setnam(LOAD_NAME);
    let (ok, end_lo, end_hi) = kernal_load(dest as u8, (dest >> 8) as u8);
    kernal_clrchn();

    leave_kernal_io(saved);

    if !ok {
        return None;
    }

    let end_addr = (end_lo as u16) | ((end_hi as u16) << 8);
    let bytes_loaded = end_addr.wrapping_sub(dest) as usize;

    if bytes_loaded == 0 || bytes_loaded > buf.len() {
        None
    } else {
        Some(bytes_loaded)
    }
}

/// Delete the save file from disk (via command channel scratch).
#[inline(never)]
pub fn delete_save() {
    let saved = enter_kernal_io();

    kernal_setlfs(15, DEVICE, 15);
    kernal_setnam(SCRATCH_NAME);
    if kernal_open() {
        kernal_close(15);
    }

    leave_kernal_io(saved);
}

/// Check if a save file exists by attempting KERNAL LOAD.
/// If successful, the data is already in SAVE_BUF for deserialization.
/// Returns (exists, bytes_loaded).
#[inline(never)]
pub fn has_save_and_preload(buf: &mut [u8]) -> Option<usize> {
    load_buf_from_disk(buf)
}
