//! Structured binary save/load for the micro tier.
//!
//! Defines a versioned binary format with magic bytes, explicit field
//! ordering, and CRC-16 checksum. The format is independent of Rust
//! struct layout — saves survive recompilation.
//!
//! Serialization uses streaming callbacks (`FnMut`) so the C64 can
//! write directly to disk via KERNAL CHROUT without a RAM buffer.
//!
//! # Format (v1)
//!
//! ```text
//! Header (8B): magic "RG" | version | width | height | seed (LE u16) | depth
//! Scalars (12B): turn_count (LE u16) | kills | flags | counters | rng (LE u16)
//! Map: room_count | rooms × {x,y,w,h} | packed tiles
//! Explored: bitfield bytes (visible is skipped — recomputed on load)
//! Entities: count | 10 parallel arrays × count
//! Items: count | 4 parallel arrays × count
//! Equipment: weapon | armor (0xFF = None)
//! Inventory: 26 × {kind, count, props[8]} (0xFF = empty slot, 10 bytes each)
//! CRC-16 (2B): CCITT over all preceding bytes
//! ```

use super::entity::EntityStore;
use super::fov::MicroFov;
use super::game::MicroGameState;
use super::item_store::{ItemStore, MAX_ITEMS};
use super::map::{MicroMap, Room};
use super::msglog::MicroMessageLog;
use super::prng::LfsrRng16;
use super::types::*;
use crate::rules::items::{Equipment, InvSlot, Inventory, ItemKind, MAX_INVENTORY};
use crate::rules::monster_table::{AiBehavior, MonsterKind};

// ---------------------------------------------------------------------------
// Format constants
// ---------------------------------------------------------------------------

pub const SAVE_MAGIC: [u8; 2] = *b"RG";
pub const SAVE_VERSION: u8 = 2;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    /// Magic bytes don't match — not a save file.
    BadMagic,
    /// Format version is newer than this build supports.
    BadVersion,
    /// CRC-16 mismatch — data is corrupted.
    BadChecksum,
    /// File ended before all fields were read.
    UnexpectedEof,
    /// A field value is out of the valid range (e.g. entity count > max).
    BadData,
}

// ---------------------------------------------------------------------------
// CRC-16-CCITT (polynomial 0x1021, initial 0xFFFF)
// ---------------------------------------------------------------------------

/// Update CRC-16-CCITT with one byte. Bit-by-bit computation is tiny
/// on 6502 (~30 bytes of machine code).
pub fn crc16_update(crc: u16, byte: u8) -> u16 {
    let mut c = crc ^ ((byte as u16) << 8);
    let mut i: u8 = 0;
    while i < 8 {
        if c & 0x8000 != 0 {
            c = (c << 1) ^ 0x1021;
        } else {
            c <<= 1;
        }
        i += 1;
    }
    c
}

/// Compute CRC-16-CCITT over a byte slice.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut i = 0;
    while i < data.len() {
        crc = crc16_update(crc, data[i]);
        i += 1;
    }
    crc
}

// ---------------------------------------------------------------------------
// Enum encode/decode — explicit matches, independent of repr ordering
// ---------------------------------------------------------------------------

fn encode_opt_monster_kind(k: Option<MonsterKind>) -> u8 {
    match k {
        None => 0xFF,
        Some(MonsterKind::Goblin) => 0,
        Some(MonsterKind::Orc) => 1,
        Some(MonsterKind::Troll) => 2,
    }
}

fn decode_opt_monster_kind(b: u8) -> Option<MonsterKind> {
    match b {
        0 => Some(MonsterKind::Goblin),
        1 => Some(MonsterKind::Orc),
        2 => Some(MonsterKind::Troll),
        _ => None,
    }
}

fn encode_ai_behavior(ai: AiBehavior) -> u8 {
    match ai {
        AiBehavior::None => 0,
        AiBehavior::Chase => 1,
        AiBehavior::Wander => 2,
    }
}

fn decode_ai_behavior(b: u8) -> AiBehavior {
    match b {
        1 => AiBehavior::Chase,
        2 => AiBehavior::Wander,
        _ => AiBehavior::None,
    }
}

fn encode_opt_item_kind(k: Option<ItemKind>) -> u8 {
    match k {
        None => 0xFF,
        Some(ItemKind::HealthPotion) => 0,
        Some(ItemKind::ShortSword) => 1,
        Some(ItemKind::LeatherArmor) => 2,
        Some(ItemKind::IronMace) => 3,
        Some(ItemKind::LongSword) => 4,
        Some(ItemKind::ChainMail) => 5,
        Some(ItemKind::GreaterHealthPotion) => 6,
        Some(ItemKind::StrengthPotion) => 7,
    }
}

fn decode_opt_item_kind(b: u8) -> Option<ItemKind> {
    match b {
        0 => Some(ItemKind::HealthPotion),
        1 => Some(ItemKind::ShortSword),
        2 => Some(ItemKind::LeatherArmor),
        3 => Some(ItemKind::IronMace),
        4 => Some(ItemKind::LongSword),
        5 => Some(ItemKind::ChainMail),
        6 => Some(ItemKind::GreaterHealthPotion),
        7 => Some(ItemKind::StrengthPotion),
        _ => None,
    }
}

fn encode_item_kind(k: ItemKind) -> u8 {
    encode_opt_item_kind(Some(k))
}

fn decode_item_kind(b: u8) -> ItemKind {
    decode_opt_item_kind(b).unwrap_or(ItemKind::HealthPotion)
}

// ---------------------------------------------------------------------------
// Size helpers
// ---------------------------------------------------------------------------

/// Number of bytes in the packed tile array for the given dimensions.
fn packed_tile_count(width: u8, height: u8) -> usize {
    let tiles = (width as usize) * (height as usize);
    tiles.div_ceil(2)
}

/// Number of bytes in the explored bitfield for the given dimensions.
fn bitfield_byte_count(width: u8, height: u8) -> usize {
    let tiles = (width as usize) * (height as usize);
    tiles.div_ceil(8)
}

// ---------------------------------------------------------------------------
// Serialize
// ---------------------------------------------------------------------------

/// Serialize `MicroGameState` to a byte stream via the `emit` callback.
///
/// Appends a CRC-16 checksum at the end (not included in the CRC itself).
/// Returns the total number of bytes emitted (including CRC).
pub fn serialize<F: FnMut(u8)>(state: &MicroGameState, emit: &mut F) -> usize {
    let mut crc: u16 = 0xFFFF;
    let mut n: usize = 0;

    macro_rules! wb {
        ($b:expr) => {{
            let byte: u8 = $b;
            crc = crc16_update(crc, byte);
            emit(byte);
            n += 1;
        }};
    }

    macro_rules! wb_u16 {
        ($v:expr) => {{
            let val: u16 = $v;
            wb!(val as u8);
            wb!((val >> 8) as u8);
        }};
    }

    // --- Header (8 bytes) ---
    wb!(SAVE_MAGIC[0]);
    wb!(SAVE_MAGIC[1]);
    wb!(SAVE_VERSION);
    wb!(state.map.width);
    wb!(state.map.height);
    wb_u16!(state.seed);
    wb!(state.depth);

    // --- Scalars (12 bytes) ---
    wb_u16!(state.turn_count);
    wb!(state.kills);
    wb!(state.game_over as u8);
    wb!(state.game_won as u8);
    wb!(state.idle_count);
    wb!(state.wandering_spawned);
    wb!(state.regen_counter);
    wb!(state.wandering_counter);
    wb!(state.ambient_sound_counter);
    wb_u16!(state.rng.state());

    // --- Map ---
    wb!(state.map.room_count);
    let rc = state.map.room_count as usize;
    let mut i = 0;
    while i < rc {
        wb!(state.map.rooms[i].x);
        wb!(state.map.rooms[i].y);
        wb!(state.map.rooms[i].w);
        wb!(state.map.rooms[i].h);
        i += 1;
    }
    let packed = packed_tile_count(state.map.width, state.map.height);
    i = 0;
    while i < packed {
        wb!(state.map.tiles[i]);
        i += 1;
    }

    // --- Explored bitfield (visible is skipped — recomputed on load) ---
    let bf_size = bitfield_byte_count(state.map.width, state.map.height);
    let explored = state.fov.explored_bytes();
    i = 0;
    while i < bf_size {
        wb!(explored[i]);
        i += 1;
    }

    // --- Entities ---
    let ec = state.entities.count as usize;
    wb!(state.entities.count);
    i = 0;
    while i < ec {
        wb!(state.entities.x[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.y[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.hp[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.max_hp[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.atk[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.def[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(encode_opt_monster_kind(state.entities.kind[i]));
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(encode_ai_behavior(state.entities.ai[i]));
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.alive[i] as u8);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb!(state.entities.sight[i]);
        i += 1;
    }

    // --- Items ---
    let ic = state.items.count as usize;
    wb!(state.items.count);
    i = 0;
    while i < ic {
        wb!(state.items.x[i]);
        i += 1;
    }
    i = 0;
    while i < ic {
        wb!(state.items.y[i]);
        i += 1;
    }
    i = 0;
    while i < ic {
        wb!(encode_item_kind(state.items.kind[i]));
        i += 1;
    }
    i = 0;
    while i < ic {
        wb!(state.items.alive[i] as u8);
        i += 1;
    }

    // --- Equipment (kind + 8 bytes props per slot) ---
    wb!(encode_opt_item_kind(state.equipment.weapon));
    {
        let mut pi = 0;
        while pi < 8 {
            wb!(state.equipment.weapon_props[pi]);
            pi += 1;
        }
    }
    wb!(encode_opt_item_kind(state.equipment.armor));
    {
        let mut pi = 0;
        while pi < 8 {
            wb!(state.equipment.armor_props[pi]);
            pi += 1;
        }
    }

    // --- Inventory (26 fixed slots, 10 bytes each: kind + count + 8 props) ---
    i = 0;
    while i < MAX_INVENTORY {
        match state.inventory.get(i) {
            Some(slot) => {
                wb!(encode_item_kind(slot.kind));
                wb!(slot.count);
                let mut pi = 0;
                while pi < 8 {
                    wb!(slot.props[pi]);
                    pi += 1;
                }
            }
            None => {
                wb!(0xFF);
                wb!(0);
                let mut pi = 0;
                while pi < 8 {
                    wb!(0);
                    pi += 1;
                }
            }
        }
        i += 1;
    }

    // --- CRC-16 (NOT included in CRC) ---
    emit(crc as u8);
    emit((crc >> 8) as u8);
    n += 2;

    n
}

// ---------------------------------------------------------------------------
// Deserialize
// ---------------------------------------------------------------------------

/// Deserialize a byte stream into `MicroGameState`.
///
/// The `read` callback must return `Some(byte)` for each byte, or `None`
/// on EOF/error. All fields of `state` are overwritten — including the
/// message log, which is reset. FOV visible bitfield is NOT restored;
/// the caller must call `compute_fov()` after a successful load.
pub fn deserialize<F: FnMut() -> Option<u8>>(
    state: &mut MicroGameState,
    read: &mut F,
) -> Result<(), SaveError> {
    let mut crc: u16 = 0xFFFF;

    macro_rules! rb {
        () => {{
            let b = read().ok_or(SaveError::UnexpectedEof)?;
            crc = crc16_update(crc, b);
            b
        }};
    }

    macro_rules! rb_u16 {
        () => {{
            let lo = rb!() as u16;
            let hi = rb!() as u16;
            lo | (hi << 8)
        }};
    }

    // --- Header ---
    if rb!() != SAVE_MAGIC[0] || rb!() != SAVE_MAGIC[1] {
        return Err(SaveError::BadMagic);
    }
    if rb!() != SAVE_VERSION {
        return Err(SaveError::BadVersion);
    }
    let width = rb!();
    let height = rb!();
    if width > MAX_MAP_WIDTH || height > MAX_MAP_HEIGHT || width == 0 || height == 0 {
        return Err(SaveError::BadData);
    }
    let seed = rb_u16!();
    let depth = rb!();

    // --- Scalars ---
    let turn_count = rb_u16!();
    let kills = rb!();
    let game_over = rb!() != 0;
    let game_won = rb!() != 0;
    let idle_count = rb!();
    let wandering_spawned = rb!();
    let regen_counter = rb!();
    let wandering_counter = rb!();
    let ambient_sound_counter = rb!();
    let rng_state = rb_u16!();

    // --- Map ---
    let room_count = rb!();
    if room_count as usize > MAX_ROOMS {
        return Err(SaveError::BadData);
    }
    state.map = MicroMap::new(width, height);
    state.map.room_count = room_count;
    let mut i: usize = 0;
    let rc = room_count as usize;
    while i < rc {
        state.map.rooms[i] = Room {
            x: rb!(),
            y: rb!(),
            w: rb!(),
            h: rb!(),
        };
        i += 1;
    }
    let packed = packed_tile_count(width, height);
    i = 0;
    while i < packed {
        state.map.tiles[i] = rb!();
        i += 1;
    }

    // --- Explored bitfield ---
    state.fov = MicroFov::new(width, height);
    let bf_size = bitfield_byte_count(width, height);
    let explored = state.fov.explored_bytes_mut();
    i = 0;
    while i < bf_size {
        explored[i] = rb!();
        i += 1;
    }

    // --- Entities ---
    let ec = rb!() as usize;
    if ec > MAX_ENTITIES {
        return Err(SaveError::BadData);
    }
    state.entities = EntityStore::new();
    state.entities.count = ec as u8;
    i = 0;
    while i < ec {
        state.entities.x[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.y[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.hp[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.max_hp[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.atk[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.def[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.kind[i] = decode_opt_monster_kind(rb!());
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.ai[i] = decode_ai_behavior(rb!());
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.alive[i] = rb!() != 0;
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.sight[i] = rb!();
        i += 1;
    }

    // --- Items ---
    let ic = rb!() as usize;
    if ic > MAX_ITEMS {
        return Err(SaveError::BadData);
    }
    state.items = ItemStore::new();
    state.items.count = ic as u8;
    i = 0;
    while i < ic {
        state.items.x[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ic {
        state.items.y[i] = rb!();
        i += 1;
    }
    i = 0;
    while i < ic {
        state.items.kind[i] = decode_item_kind(rb!());
        i += 1;
    }
    i = 0;
    while i < ic {
        state.items.alive[i] = rb!() != 0;
        i += 1;
    }

    // --- Equipment (kind + 8 bytes props per slot) ---
    let weapon = decode_opt_item_kind(rb!());
    let mut weapon_props = crate::rules::properties::EMPTY;
    {
        let mut pi = 0;
        while pi < 8 {
            weapon_props[pi] = rb!();
            pi += 1;
        }
    }
    let armor = decode_opt_item_kind(rb!());
    let mut armor_props = crate::rules::properties::EMPTY;
    {
        let mut pi = 0;
        while pi < 8 {
            armor_props[pi] = rb!();
            pi += 1;
        }
    }
    state.equipment = Equipment {
        weapon,
        armor,
        weapon_props,
        armor_props,
    };

    // --- Inventory (10 bytes per slot: kind + count + 8 props) ---
    state.inventory = Inventory::new();
    i = 0;
    while i < MAX_INVENTORY {
        let kind_byte = rb!();
        let count_byte = rb!();
        let mut props = [0u8; 8];
        let mut pi = 0;
        while pi < 8 {
            props[pi] = rb!();
            pi += 1;
        }
        if let Some(kind) = decode_opt_item_kind(kind_byte) {
            state.inventory.set_slot(
                i,
                Some(InvSlot {
                    kind,
                    count: count_byte,
                    props,
                }),
            );
        }
        i += 1;
    }

    // --- Write remaining scalars ---
    state.seed = seed;
    state.depth = depth;
    state.turn_count = turn_count;
    state.kills = kills;
    state.game_over = game_over;
    state.game_won = game_won;
    state.idle_count = idle_count;
    state.wandering_spawned = wandering_spawned;
    state.regen_counter = regen_counter;
    state.wandering_counter = wandering_counter;
    state.ambient_sound_counter = ambient_sound_counter;
    state.rng = LfsrRng16::from_raw_state(rng_state);
    state.log = MicroMessageLog::new();

    // --- CRC verification ---
    let stored_lo = read().ok_or(SaveError::UnexpectedEof)?;
    let stored_hi = read().ok_or(SaveError::UnexpectedEof)?;
    let stored_crc = stored_lo as u16 | ((stored_hi as u16) << 8);
    if crc != stored_crc {
        return Err(SaveError::BadChecksum);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier_micro::game::MicroGameState;

    /// Serialize a game state to a Vec<u8>.
    fn serialize_to_vec(state: &MicroGameState) -> Vec<u8> {
        let mut buf = Vec::new();
        serialize(state, &mut |b| buf.push(b));
        buf
    }

    /// Deserialize from a byte slice into a MicroGameState.
    fn deserialize_from_slice(state: &mut MicroGameState, data: &[u8]) -> Result<(), SaveError> {
        let mut pos = 0;
        deserialize(state, &mut || {
            if pos < data.len() {
                let b = data[pos];
                pos += 1;
                Some(b)
            } else {
                None
            }
        })
    }

    #[test]
    fn round_trip_default_game() {
        let original = MicroGameState::new_default(42);
        let bytes = serialize_to_vec(&original);

        let mut loaded = MicroGameState::new_default(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        // Verify key fields match
        assert_eq!(loaded.seed, original.seed);
        assert_eq!(loaded.depth, original.depth);
        assert_eq!(loaded.turn_count, original.turn_count);
        assert_eq!(loaded.kills, original.kills);
        assert_eq!(loaded.game_over, original.game_over);
        assert_eq!(loaded.game_won, original.game_won);
        assert_eq!(loaded.rng.state(), original.rng.state());
        assert_eq!(loaded.map.width, original.map.width);
        assert_eq!(loaded.map.height, original.map.height);
        assert_eq!(loaded.map.room_count, original.map.room_count);
        assert_eq!(loaded.entities.count, original.entities.count);
        assert_eq!(loaded.items.count, original.items.count);
        assert_eq!(loaded.equipment, original.equipment);

        // Verify entity arrays match for alive entities
        let ec = original.entities.count as usize;
        assert_eq!(&loaded.entities.x[..ec], &original.entities.x[..ec]);
        assert_eq!(&loaded.entities.y[..ec], &original.entities.y[..ec]);
        assert_eq!(&loaded.entities.hp[..ec], &original.entities.hp[..ec]);
        assert_eq!(&loaded.entities.alive[..ec], &original.entities.alive[..ec]);
        assert_eq!(&loaded.entities.kind[..ec], &original.entities.kind[..ec]);

        // Verify map tiles match
        let packed = packed_tile_count(original.map.width, original.map.height);
        assert_eq!(&loaded.map.tiles[..packed], &original.map.tiles[..packed]);

        // Verify explored bitfield matches
        let bf = bitfield_byte_count(original.map.width, original.map.height);
        assert_eq!(
            &loaded.fov.explored_bytes()[..bf],
            &original.fov.explored_bytes()[..bf]
        );
    }

    #[test]
    fn round_trip_byte_stream_identical() {
        // Serialize twice via round-trip and compare byte streams.
        // This catches any field that serialize writes but deserialize
        // doesn't restore (or vice versa).
        let original = MicroGameState::new_default(12345);
        let bytes1 = serialize_to_vec(&original);

        let mut loaded = MicroGameState::new_default(0);
        deserialize_from_slice(&mut loaded, &bytes1).unwrap();
        let bytes2 = serialize_to_vec(&loaded);

        assert_eq!(
            bytes1, bytes2,
            "re-serialization should produce identical bytes"
        );
    }

    #[test]
    fn round_trip_with_inventory() {
        let mut state = MicroGameState::new_default(99);
        state.inventory.add(ItemKind::HealthPotion);
        state.inventory.add(ItemKind::HealthPotion);
        state.inventory.add(ItemKind::ShortSword);
        state.equipment = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: crate::rules::items::default_properties(ItemKind::ShortSword),
            armor: Some(ItemKind::LeatherArmor),
            armor_props: crate::rules::items::default_properties(ItemKind::LeatherArmor),
        };

        let bytes = serialize_to_vec(&state);
        let mut loaded = MicroGameState::new_default(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert_eq!(loaded.equipment, state.equipment);
        assert_eq!(loaded.inventory.len(), state.inventory.len());
        // Potions should be stacked (count=2)
        assert_eq!(loaded.inventory.get(0).unwrap().count, 2);
        assert_eq!(
            loaded.inventory.get(0).unwrap().kind,
            ItemKind::HealthPotion
        );
        assert_eq!(loaded.inventory.get(1).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn bad_magic_rejected() {
        let state = MicroGameState::new_default(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[0] = b'X'; // corrupt magic
        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadMagic)
        );
    }

    #[test]
    fn bad_version_rejected() {
        let state = MicroGameState::new_default(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[2] = 99; // future version
        // Fix CRC for the corrupted data so we test version check, not CRC
        let data_len = bytes.len() - 2;
        let new_crc = crc16(&bytes[..data_len]);
        bytes[data_len] = new_crc as u8;
        bytes[data_len + 1] = (new_crc >> 8) as u8;

        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadVersion)
        );
    }

    #[test]
    fn bad_checksum_rejected() {
        let state = MicroGameState::new_default(1);
        let mut bytes = serialize_to_vec(&state);
        // Corrupt a data byte (not the CRC bytes)
        bytes[10] ^= 0xFF;
        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadChecksum)
        );
    }

    #[test]
    fn truncated_file_rejected() {
        let state = MicroGameState::new_default(1);
        let bytes = serialize_to_vec(&state);
        // Cut off halfway
        let truncated = &bytes[..bytes.len() / 2];
        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, truncated),
            Err(SaveError::UnexpectedEof)
        );
    }

    #[test]
    fn empty_file_rejected() {
        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &[]),
            Err(SaveError::UnexpectedEof)
        );
    }

    #[test]
    fn crc16_known_vectors() {
        // "123456789" should produce 0x29B1 for CRC-16-CCITT-FALSE
        assert_eq!(crc16(b"123456789"), 0x29B1);
        assert_eq!(crc16(b""), 0xFFFF); // empty input = initial value
    }

    #[test]
    fn save_size_reasonable() {
        let state = MicroGameState::new_default(42);
        let bytes = serialize_to_vec(&state);
        // 64×48 map: ~2000-2500 bytes typical
        assert!(
            bytes.len() > 1500 && bytes.len() < 4000,
            "save size {} outside expected range",
            bytes.len()
        );
    }

    #[test]
    fn enum_encode_decode_round_trip() {
        // MonsterKind
        for &mk in &[MonsterKind::Goblin, MonsterKind::Orc, MonsterKind::Troll] {
            let encoded = encode_opt_monster_kind(Some(mk));
            assert_eq!(decode_opt_monster_kind(encoded), Some(mk));
        }
        assert_eq!(decode_opt_monster_kind(encode_opt_monster_kind(None)), None);
        assert_eq!(decode_opt_monster_kind(0xFE), None); // unknown

        // AiBehavior
        for &ai in &[AiBehavior::None, AiBehavior::Chase, AiBehavior::Wander] {
            let encoded = encode_ai_behavior(ai);
            assert_eq!(decode_ai_behavior(encoded), ai);
        }

        // ItemKind
        for &ik in &[
            ItemKind::HealthPotion,
            ItemKind::ShortSword,
            ItemKind::LeatherArmor,
        ] {
            let encoded = encode_opt_item_kind(Some(ik));
            assert_eq!(decode_opt_item_kind(encoded), Some(ik));
            assert_eq!(decode_item_kind(encode_item_kind(ik)), ik);
        }
        assert_eq!(decode_opt_item_kind(encode_opt_item_kind(None)), None);
    }

    #[test]
    fn multiple_seeds_round_trip() {
        for seed in [1, 100, 1000, 0xFFFF, 0xACE1] {
            let original = MicroGameState::new_default(seed);
            let bytes = serialize_to_vec(&original);
            let mut loaded = MicroGameState::new_default(0);
            deserialize_from_slice(&mut loaded, &bytes).unwrap();
            assert_eq!(loaded.seed, original.seed);
            assert_eq!(loaded.rng.state(), original.rng.state());

            // Re-serialize and verify identical
            let bytes2 = serialize_to_vec(&loaded);
            assert_eq!(bytes, bytes2);
        }
    }

    #[test]
    fn round_trip_custom_dimensions() {
        let original = MicroGameState::new(42, 32, 24);
        let bytes = serialize_to_vec(&original);
        let mut loaded = MicroGameState::new_default(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert_eq!(loaded.map.width, 32);
        assert_eq!(loaded.map.height, 24);
        let bytes2 = serialize_to_vec(&loaded);
        assert_eq!(bytes, bytes2);
    }

    #[test]
    fn bad_dimensions_rejected() {
        let state = MicroGameState::new_default(1);
        let mut bytes = serialize_to_vec(&state);
        // Set width to 0 (invalid)
        bytes[3] = 0;
        // Fix CRC
        let data_len = bytes.len() - 2;
        let new_crc = crc16(&bytes[..data_len]);
        bytes[data_len] = new_crc as u8;
        bytes[data_len + 1] = (new_crc >> 8) as u8;

        let mut loaded = MicroGameState::new_default(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadData)
        );
    }

    #[test]
    fn private_counters_preserved() {
        let mut original = MicroGameState::new_default(42);
        // Advance the game a few turns to change counter values
        use crate::command::GameCommand;
        for _ in 0..5 {
            original.step(GameCommand::Wait);
        }

        let bytes = serialize_to_vec(&original);
        let mut loaded = MicroGameState::new_default(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert_eq!(loaded.regen_counter, original.regen_counter);
        assert_eq!(loaded.wandering_counter, original.wandering_counter);
        assert_eq!(loaded.ambient_sound_counter, original.ambient_sound_counter);
    }
}
