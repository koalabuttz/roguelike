//! GBA SRAM save/load wrapper.
//!
//! Cartridge SRAM is 32 KB at `0x0E00_0000`, 8-bit bus — all accesses must
//! be byte-by-byte volatile reads/writes. Uses the compact tier's streaming
//! serializer directly: each `emit(byte)` writes one byte to SRAM.
//!
//! Single auto-save slot for now (slot 0). Multi-slot UI can come later.

use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::save::{self, SaveError};

const SRAM_BASE: usize = 0x0E00_0000;

/// Maximum bytes for one save slot.
/// Compact tier: 80×40 map = 3200 tiles + 400 explored + entities + items + overhead.
/// Measured at ~4500 bytes typical, allowing up to 6144 for headroom.
const SLOT_SIZE: usize = 6144;

/// Header at SRAM offset 0: 4-byte magic + validity marker.
const HEADER_MAGIC: [u8; 4] = *b"RGSV";
const HEADER_SIZE: usize = 8; // magic(4) + save_size(2) + reserved(2)

/// Byte offset where slot 0 data begins.
const SLOT0_OFFSET: usize = HEADER_SIZE;

// ---------------------------------------------------------------------------
// Low-level SRAM access
// ---------------------------------------------------------------------------

#[inline(always)]
fn sram_write(offset: usize, byte: u8) {
    unsafe {
        ((SRAM_BASE + offset) as *mut u8).write_volatile(byte);
    }
}

#[inline(always)]
fn sram_read(offset: usize) -> u8 {
    unsafe { ((SRAM_BASE + offset) as *const u8).read_volatile() }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Save game state to SRAM slot 0. Returns number of bytes written.
pub fn save_to_sram(state: &CompactGameState) -> usize {
    let mut offset = SLOT0_OFFSET;
    let size = save::serialize(state, &mut |byte| {
        sram_write(offset, byte);
        offset += 1;
    });

    // Write header: magic + save size
    sram_write(0, HEADER_MAGIC[0]);
    sram_write(1, HEADER_MAGIC[1]);
    sram_write(2, HEADER_MAGIC[2]);
    sram_write(3, HEADER_MAGIC[3]);
    sram_write(4, size as u8);
    sram_write(5, (size >> 8) as u8);
    sram_write(6, 0); // reserved
    sram_write(7, 0);

    size
}

/// Load game state from SRAM slot 0. Returns Ok(()) if valid save found.
/// FOV visible bitfield is NOT restored — caller must recompute.
pub fn load_from_sram(state: &mut CompactGameState) -> Result<(), SaveError> {
    // Check header magic
    if sram_read(0) != HEADER_MAGIC[0]
        || sram_read(1) != HEADER_MAGIC[1]
        || sram_read(2) != HEADER_MAGIC[2]
        || sram_read(3) != HEADER_MAGIC[3]
    {
        return Err(SaveError::BadMagic);
    }

    let save_size = sram_read(4) as usize | ((sram_read(5) as usize) << 8);
    if save_size == 0 || save_size > SLOT_SIZE {
        return Err(SaveError::BadData);
    }

    let mut offset = SLOT0_OFFSET;
    let end = SLOT0_OFFSET + save_size;
    save::deserialize(state, &mut || {
        if offset < end {
            let b = sram_read(offset);
            offset += 1;
            Some(b)
        } else {
            None
        }
    })
}

/// Check if SRAM contains a valid save (header magic present).
pub fn has_save() -> bool {
    sram_read(0) == HEADER_MAGIC[0]
        && sram_read(1) == HEADER_MAGIC[1]
        && sram_read(2) == HEADER_MAGIC[2]
        && sram_read(3) == HEADER_MAGIC[3]
}

/// Erase the save by clearing the header magic.
pub fn erase_save() {
    sram_write(0, 0xFF);
    sram_write(1, 0xFF);
    sram_write(2, 0xFF);
    sram_write(3, 0xFF);
}
