//! GBA SRAM save/load wrapper.
//!
//! Cartridge SRAM is 32 KB at `0x0E00_0000`, 8-bit bus — all accesses must
//! be byte-by-byte volatile reads/writes. Supports both compact and micro
//! tier saves via the tier byte in the save envelope.

use roguelike_core::rules::save_common::SaveError;
use roguelike_core::rules::seed_code::Tier;
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_compact::save as compact_save;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::tier_micro::save as micro_save;

const SRAM_BASE: usize = 0x0E00_0000;

/// Maximum bytes for one save slot.
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

fn write_header(size: usize) {
    sram_write(0, HEADER_MAGIC[0]);
    sram_write(1, HEADER_MAGIC[1]);
    sram_write(2, HEADER_MAGIC[2]);
    sram_write(3, HEADER_MAGIC[3]);
    sram_write(4, size as u8);
    sram_write(5, (size >> 8) as u8);
    sram_write(6, 0);
    sram_write(7, 0);
}

// ---------------------------------------------------------------------------
// Compact tier save/load
// ---------------------------------------------------------------------------

fn save_compact_to_sram(state: &CompactGameState) -> usize {
    let mut offset = SLOT0_OFFSET;
    let size = compact_save::serialize(state, &mut |byte| {
        sram_write(offset, byte);
        offset += 1;
    });
    write_header(size);
    size
}

fn load_compact_from_sram(state: &mut CompactGameState) -> Result<(), SaveError> {
    let save_size = sram_read(4) as usize | ((sram_read(5) as usize) << 8);
    if save_size == 0 || save_size > SLOT_SIZE {
        return Err(SaveError::BadData);
    }
    let mut offset = SLOT0_OFFSET;
    let end = SLOT0_OFFSET + save_size;
    compact_save::deserialize(state, &mut || {
        if offset < end {
            let b = sram_read(offset);
            offset += 1;
            Some(b)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Micro tier save/load
// ---------------------------------------------------------------------------

fn save_micro_to_sram(state: &MicroGameState) -> usize {
    let mut offset = SLOT0_OFFSET;
    let size = micro_save::serialize(state, &mut |byte| {
        sram_write(offset, byte);
        offset += 1;
    });
    write_header(size);
    size
}

fn load_micro_from_sram(state: &mut MicroGameState) -> Result<(), SaveError> {
    let save_size = sram_read(4) as usize | ((sram_read(5) as usize) << 8);
    if save_size == 0 || save_size > SLOT_SIZE {
        return Err(SaveError::BadData);
    }
    let mut offset = SLOT0_OFFSET;
    let end = SLOT0_OFFSET + save_size;
    micro_save::deserialize(state, &mut || {
        if offset < end {
            let b = sram_read(offset);
            offset += 1;
            Some(b)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Tier-aware dispatch
// ---------------------------------------------------------------------------

/// Save the active game to SRAM. Dispatches based on IS_MICRO flag.
pub fn save_dispatch() -> usize {
    if super::game_loop::is_micro() {
        save_micro_to_sram(super::game_loop::game_micro())
    } else {
        save_compact_to_sram(super::game_loop::game_compact())
    }
}

/// Load a game from SRAM. Auto-detects tier from the save envelope.
/// Sets IS_MICRO and loads into the correct union variant.
/// Returns true on success.
pub fn load_dispatch() -> bool {
    if !has_save() {
        return false;
    }

    // Peek at the tier byte in the save envelope (byte 3 of the save data).
    let tier_byte = sram_read(SLOT0_OFFSET + 3);
    match tier_byte {
        t if t == Tier::Micro as u8 => {
            unsafe { super::game_loop::set_micro(true) };
            load_micro_from_sram(super::game_loop::game_micro()).is_ok()
        }
        t if t == Tier::Compact as u8 => {
            unsafe { super::game_loop::set_micro(false) };
            load_compact_from_sram(super::game_loop::game_compact()).is_ok()
        }
        _ => false,
    }
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

// ---------------------------------------------------------------------------
// Persistent settings (independent of game saves)
// ---------------------------------------------------------------------------

/// Settings live at a fixed SRAM offset well past the game save area.
const SETTINGS_OFFSET: usize = 8192;
/// 4-byte magic to validate the settings block.
const SETTINGS_MAGIC: [u8; 4] = *b"RGST";

/// GBA-specific persistent settings. Stored in SRAM independently of game saves
/// so they survive character death and console restarts.
#[derive(Clone, Copy)]
pub struct GbaSettings {
    pub auto_pickup: bool,
}

impl GbaSettings {
    const fn default() -> Self {
        Self { auto_pickup: false }
    }
}

static mut SETTINGS: GbaSettings = GbaSettings::default();

/// Read the current settings (from the in-memory cache).
pub fn settings() -> GbaSettings {
    unsafe { SETTINGS }
}

/// Update a setting, save to SRAM, and sync to the active game state.
pub fn update_settings(s: GbaSettings) {
    unsafe { SETTINGS = s };
    save_settings_to_sram();
    apply_settings_to_game();
}

/// Load settings from SRAM into the in-memory cache. Call once at boot.
pub fn load_settings() {
    let ok = sram_read(SETTINGS_OFFSET) == SETTINGS_MAGIC[0]
        && sram_read(SETTINGS_OFFSET + 1) == SETTINGS_MAGIC[1]
        && sram_read(SETTINGS_OFFSET + 2) == SETTINGS_MAGIC[2]
        && sram_read(SETTINGS_OFFSET + 3) == SETTINGS_MAGIC[3];
    if ok {
        let flags = sram_read(SETTINGS_OFFSET + 4);
        unsafe {
            SETTINGS.auto_pickup = flags & 1 != 0;
        }
    }
    // If no valid settings block, keep defaults.
}

/// Apply cached settings to whichever game state is currently active.
pub fn apply_settings_to_game() {
    let s = unsafe { SETTINGS };
    if super::game_loop::is_micro() {
        super::game_loop::game_micro().auto_pickup = s.auto_pickup;
    } else {
        super::game_loop::game_compact().auto_pickup = s.auto_pickup;
    }
}

fn save_settings_to_sram() {
    let s = unsafe { SETTINGS };
    sram_write(SETTINGS_OFFSET, SETTINGS_MAGIC[0]);
    sram_write(SETTINGS_OFFSET + 1, SETTINGS_MAGIC[1]);
    sram_write(SETTINGS_OFFSET + 2, SETTINGS_MAGIC[2]);
    sram_write(SETTINGS_OFFSET + 3, SETTINGS_MAGIC[3]);
    sram_write(SETTINGS_OFFSET + 4, s.auto_pickup as u8);
}
