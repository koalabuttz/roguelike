//! Structured binary save/load for the compact tier (GBA).
//!
//! Follows the same streaming pattern as `tier_micro/save.rs`: versioned
//! binary format with magic bytes, explicit field ordering, CRC-16 checksum,
//! and a tier byte in the envelope so any platform can detect the save type.
//!
//! Key differences from micro format:
//! - Tier byte (1) after version in envelope
//! - i32 coords (4 bytes LE) for entities, items, and rooms
//! - Unpacked tiles (one byte per tile, no nibble packing)
//! - u32 seed and RNG state (4 bytes LE each)
//! - Fixed map dimensions (MAP_WIDTH × MAP_HEIGHT) — not stored per-save
//!
//! # Format (v1)
//!
//! ```text
//! Envelope (4B): magic "RG" | version(1) | tier(1)
//! Header (8B):   seed (LE u32) | depth | turn_count (LE u16) | kills
//! Scalars (4B):  flags(game_over,game_won,auto_pickup) | idle | wander_spawned | wander_counter
//! RNG (4B):      rng state (LE u32)
//! Map:           room_count | rooms[count × 16B] | tiles[MAP_SIZE]
//! Explored:      bitfield[BITFIELD_SIZE]
//! Entities:      count | parallel arrays (i32 coords = 4B LE each)
//! Items:         count | parallel arrays (i32 coords = 4B LE each)
//! Equipment:     weapon(1) | weapon_props(8) | armor(1) | armor_props(8)
//! Inventory:     26 × {kind(1) | count(1) | props(8)}
//! CRC-16 (2B):   CCITT over all preceding bytes
//! ```

use super::entity::EntityStore;
use super::fov::CompactFov;
use super::game::CompactGameState;
use super::item_store::ItemStore;
use super::map::{CompactMap, Room};
use super::msglog::CompactMessageLog;
use super::prng::LfsrRng32;
use super::types::*;
use crate::rules::items::{Equipment, InvSlot, Inventory, MAX_INVENTORY};
// ItemKind used in tests via `use super::*` + save_common glob.
#[cfg(test)]
use crate::rules::items::ItemKind;
use crate::rules::save_common::*;
use crate::rules::seed_code::Tier;

pub use crate::rules::save_common::{crc16, crc16_update, SaveError, SAVE_MAGIC};

/// Format version for compact tier saves.
pub const SAVE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Serialize
// ---------------------------------------------------------------------------

/// Serialize `CompactGameState` to a byte stream via the `emit` callback.
///
/// Appends a CRC-16 checksum at the end. Returns total bytes emitted.
pub fn serialize<F: FnMut(u8)>(state: &CompactGameState, emit: &mut F) -> usize {
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

    macro_rules! wb_i32 {
        ($v:expr) => {{
            let val: u32 = $v as u32;
            wb!(val as u8);
            wb!((val >> 8) as u8);
            wb!((val >> 16) as u8);
            wb!((val >> 24) as u8);
        }};
    }

    macro_rules! wb_u32 {
        ($v:expr) => {{
            let val: u32 = $v;
            wb!(val as u8);
            wb!((val >> 8) as u8);
            wb!((val >> 16) as u8);
            wb!((val >> 24) as u8);
        }};
    }

    // --- Envelope (4 bytes) ---
    wb!(SAVE_MAGIC[0]);
    wb!(SAVE_MAGIC[1]);
    wb!(SAVE_VERSION);
    wb!(Tier::Compact as u8);

    // --- Header (8 bytes) ---
    wb_u32!(state.seed);
    wb!(state.depth);
    wb_u16!(state.turn_count);
    wb!(state.kills);

    // --- Scalars (4 bytes) ---
    let flags: u8 = (state.game_over as u8)
        | ((state.game_won as u8) << 1)
        | ((state.auto_pickup as u8) << 2);
    wb!(flags);
    wb!(state.idle_count);
    wb!(state.wandering_spawned);
    wb!(state.wandering_counter);

    // --- RNG (4 bytes) ---
    wb_u32!(state.rng.state());

    // --- Map ---
    wb!(state.map.room_count);
    let rc = state.map.room_count as usize;
    let mut i = 0;
    while i < rc {
        wb_i32!(state.map.rooms[i].x);
        wb_i32!(state.map.rooms[i].y);
        wb_i32!(state.map.rooms[i].w);
        wb_i32!(state.map.rooms[i].h);
        i += 1;
    }
    // Unpacked tiles — one byte per tile
    let tile_count = (state.map.width as usize) * (state.map.height as usize);
    i = 0;
    while i < tile_count {
        wb!(state.map.tiles[i]);
        i += 1;
    }

    // --- Explored bitfield (visible is skipped — recomputed on load) ---
    let explored = state.fov.explored_bytes();
    i = 0;
    while i < BITFIELD_SIZE {
        wb!(explored[i]);
        i += 1;
    }

    // --- Entities ---
    let ec = state.entities.count as usize;
    wb!(state.entities.count);
    i = 0;
    while i < ec {
        wb_i32!(state.entities.x[i]);
        i += 1;
    }
    i = 0;
    while i < ec {
        wb_i32!(state.entities.y[i]);
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
        wb_i32!(state.items.x[i]);
        i += 1;
    }
    i = 0;
    while i < ic {
        wb_i32!(state.items.y[i]);
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

/// Deserialize a byte stream into `CompactGameState`.
///
/// The `read` callback must return `Some(byte)` for each byte, or `None`
/// on EOF/error. All fields of `state` are overwritten — including the
/// message log, which is reset. FOV visible bitfield is NOT restored;
/// the caller must call `compute_fov()` after a successful load.
pub fn deserialize<F: FnMut() -> Option<u8>>(
    state: &mut CompactGameState,
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

    macro_rules! rb_u32 {
        () => {{
            let b0 = rb!() as u32;
            let b1 = rb!() as u32;
            let b2 = rb!() as u32;
            let b3 = rb!() as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        }};
    }

    macro_rules! rb_i32 {
        () => {
            rb_u32!() as i32
        };
    }

    // --- Envelope ---
    if rb!() != SAVE_MAGIC[0] || rb!() != SAVE_MAGIC[1] {
        return Err(SaveError::BadMagic);
    }
    if rb!() != SAVE_VERSION {
        return Err(SaveError::BadVersion);
    }
    let tier = rb!();
    if tier != Tier::Compact as u8 {
        return Err(SaveError::BadData);
    }

    // --- Header ---
    let seed = rb_u32!();
    let depth = rb!();
    let turn_count = rb_u16!();
    let kills = rb!();

    // --- Scalars ---
    let flags = rb!();
    let game_over = flags & 1 != 0;
    let game_won = flags & 2 != 0;
    let auto_pickup = flags & 4 != 0;
    let idle_count = rb!();
    let wandering_spawned = rb!();
    let wandering_counter = rb!();

    // --- RNG ---
    let rng_state = rb_u32!();

    // --- Map ---
    let room_count = rb!();
    if room_count as usize > MAX_ROOMS {
        return Err(SaveError::BadData);
    }
    state.map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
    state.map.room_count = room_count;
    let mut i: usize = 0;
    let rc = room_count as usize;
    while i < rc {
        state.map.rooms[i] = Room {
            x: rb_i32!(),
            y: rb_i32!(),
            w: rb_i32!(),
            h: rb_i32!(),
        };
        i += 1;
    }
    let tile_count = (MAP_WIDTH as usize) * (MAP_HEIGHT as usize);
    i = 0;
    while i < tile_count {
        state.map.tiles[i] = rb!();
        i += 1;
    }

    // --- Explored bitfield ---
    state.fov = CompactFov::new(MAP_WIDTH, MAP_HEIGHT);
    let explored = state.fov.explored_bytes_mut();
    i = 0;
    while i < BITFIELD_SIZE {
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
        state.entities.x[i] = rb_i32!();
        i += 1;
    }
    i = 0;
    while i < ec {
        state.entities.y[i] = rb_i32!();
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
        state.items.x[i] = rb_i32!();
        i += 1;
    }
    i = 0;
    while i < ic {
        state.items.y[i] = rb_i32!();
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
    state.auto_pickup = auto_pickup;
    state.idle_count = idle_count;
    state.wandering_spawned = wandering_spawned;
    state.wandering_counter = wandering_counter;
    state.rng = LfsrRng32::from_raw_state(rng_state);
    state.log = CompactMessageLog::new();

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
    use crate::rules::items;
    use crate::tier_compact::game::CompactGameState;

    fn serialize_to_vec(state: &CompactGameState) -> Vec<u8> {
        let mut buf = Vec::new();
        serialize(state, &mut |b| buf.push(b));
        buf
    }

    fn deserialize_from_slice(
        state: &mut CompactGameState,
        data: &[u8],
    ) -> Result<(), SaveError> {
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

    fn new_game(seed: u32) -> CompactGameState {
        CompactGameState::new(seed, MAP_WIDTH, MAP_HEIGHT)
    }

    #[test]
    fn round_trip_default_game() {
        let original = new_game(42);
        let bytes = serialize_to_vec(&original);

        let mut loaded = new_game(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert_eq!(loaded.seed, original.seed);
        assert_eq!(loaded.depth, original.depth);
        assert_eq!(loaded.turn_count, original.turn_count);
        assert_eq!(loaded.kills, original.kills);
        assert_eq!(loaded.game_over, original.game_over);
        assert_eq!(loaded.game_won, original.game_won);
        assert_eq!(loaded.auto_pickup, original.auto_pickup);
        assert_eq!(loaded.rng.state(), original.rng.state());
        assert_eq!(loaded.map.room_count, original.map.room_count);
        assert_eq!(loaded.entities.count, original.entities.count);
        assert_eq!(loaded.items.count, original.items.count);
        assert_eq!(loaded.equipment, original.equipment);

        // Verify entity arrays match
        let ec = original.entities.count as usize;
        assert_eq!(&loaded.entities.x[..ec], &original.entities.x[..ec]);
        assert_eq!(&loaded.entities.y[..ec], &original.entities.y[..ec]);
        assert_eq!(&loaded.entities.hp[..ec], &original.entities.hp[..ec]);
        assert_eq!(&loaded.entities.alive[..ec], &original.entities.alive[..ec]);
        assert_eq!(&loaded.entities.kind[..ec], &original.entities.kind[..ec]);

        // Verify map tiles match
        let tile_count = (original.map.width as usize) * (original.map.height as usize);
        assert_eq!(
            &loaded.map.tiles[..tile_count],
            &original.map.tiles[..tile_count]
        );

        // Verify explored bitfield matches
        assert_eq!(
            loaded.fov.explored_bytes(),
            original.fov.explored_bytes()
        );
    }

    #[test]
    fn round_trip_byte_stream_identical() {
        let original = new_game(12345);
        let bytes1 = serialize_to_vec(&original);

        let mut loaded = new_game(0);
        deserialize_from_slice(&mut loaded, &bytes1).unwrap();
        let bytes2 = serialize_to_vec(&loaded);

        assert_eq!(
            bytes1, bytes2,
            "re-serialization should produce identical bytes"
        );
    }

    #[test]
    fn round_trip_with_inventory() {
        let mut state = new_game(99);
        state.inventory.add(ItemKind::HealthPotion);
        state.inventory.add(ItemKind::HealthPotion);
        state.inventory.add(ItemKind::ShortSword);
        state.equipment = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: items::default_properties(ItemKind::ShortSword),
            armor: Some(ItemKind::LeatherArmor),
            armor_props: items::default_properties(ItemKind::LeatherArmor),
        };

        let bytes = serialize_to_vec(&state);
        let mut loaded = new_game(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert_eq!(loaded.equipment, state.equipment);
        assert_eq!(loaded.inventory.len(), state.inventory.len());
        assert_eq!(loaded.inventory.get(0).unwrap().count, 2);
        assert_eq!(
            loaded.inventory.get(0).unwrap().kind,
            ItemKind::HealthPotion
        );
        assert_eq!(loaded.inventory.get(1).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn bad_magic_rejected() {
        let state = new_game(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[0] = b'X';
        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadMagic)
        );
    }

    #[test]
    fn bad_version_rejected() {
        let state = new_game(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[2] = 99; // future version
        let data_len = bytes.len() - 2;
        let new_crc = crc16(&bytes[..data_len]);
        bytes[data_len] = new_crc as u8;
        bytes[data_len + 1] = (new_crc >> 8) as u8;

        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadVersion)
        );
    }

    #[test]
    fn wrong_tier_rejected() {
        let state = new_game(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[3] = Tier::Micro as u8; // wrong tier
        let data_len = bytes.len() - 2;
        let new_crc = crc16(&bytes[..data_len]);
        bytes[data_len] = new_crc as u8;
        bytes[data_len + 1] = (new_crc >> 8) as u8;

        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadData)
        );
    }

    #[test]
    fn bad_checksum_rejected() {
        let state = new_game(1);
        let mut bytes = serialize_to_vec(&state);
        bytes[10] ^= 0xFF;
        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &bytes),
            Err(SaveError::BadChecksum)
        );
    }

    #[test]
    fn truncated_file_rejected() {
        let state = new_game(1);
        let bytes = serialize_to_vec(&state);
        let truncated = &bytes[..bytes.len() / 2];
        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, truncated),
            Err(SaveError::UnexpectedEof)
        );
    }

    #[test]
    fn empty_file_rejected() {
        let mut loaded = new_game(0);
        assert_eq!(
            deserialize_from_slice(&mut loaded, &[]),
            Err(SaveError::UnexpectedEof)
        );
    }

    #[test]
    fn save_size_reasonable() {
        let state = new_game(42);
        let bytes = serialize_to_vec(&state);
        // 80×40 map: ~4000-6000 bytes typical
        assert!(
            bytes.len() > 3000 && bytes.len() < 8000,
            "save size {} outside expected range",
            bytes.len()
        );
    }

    #[test]
    fn multiple_seeds_round_trip() {
        for seed in [1u32, 100, 1000, 0xFFFF, 0xACE1_CAFE] {
            let original = new_game(seed);
            let bytes = serialize_to_vec(&original);
            let mut loaded = new_game(0);
            deserialize_from_slice(&mut loaded, &bytes).unwrap();
            assert_eq!(loaded.seed, original.seed);
            assert_eq!(loaded.rng.state(), original.rng.state());

            let bytes2 = serialize_to_vec(&loaded);
            assert_eq!(bytes, bytes2);
        }
    }

    #[test]
    fn flags_round_trip() {
        let mut state = new_game(42);
        state.game_over = true;
        state.game_won = true;
        state.auto_pickup = true;
        state.idle_count = 7;
        state.wandering_spawned = 3;

        let bytes = serialize_to_vec(&state);
        let mut loaded = new_game(0);
        deserialize_from_slice(&mut loaded, &bytes).unwrap();

        assert!(loaded.game_over);
        assert!(loaded.game_won);
        assert!(loaded.auto_pickup);
        assert_eq!(loaded.idle_count, 7);
        assert_eq!(loaded.wandering_spawned, 3);
    }

    #[test]
    fn envelope_has_tier_byte() {
        let state = new_game(42);
        let bytes = serialize_to_vec(&state);
        assert_eq!(bytes[0], b'R');
        assert_eq!(bytes[1], b'G');
        assert_eq!(bytes[2], SAVE_VERSION);
        assert_eq!(bytes[3], Tier::Compact as u8);
    }
}
