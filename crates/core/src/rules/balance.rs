//! Shared balance constants for all capability tiers.
//!
//! This module is the single source of truth for game balance. All tiers and
//! platforms use these constants directly. The PC's data-file loading
//! (`data.rs`, gated behind `data-files`) uses these as compiled-in defaults;
//! modders can override values via `game.toml`.
//!
//! All types are `u8` — values that fit in the smallest tier's natural width.

// ---------------------------------------------------------------------------
// Player defaults
// ---------------------------------------------------------------------------

pub const PLAYER_HP: u8 = 30;
pub const PLAYER_ATK: u8 = 5;
pub const PLAYER_DEF: u8 = 2;
pub const PLAYER_GLYPH: char = '@';

// ---------------------------------------------------------------------------
// Monster stats — indexed by MonsterKind (future) or looked up by name (now)
// ---------------------------------------------------------------------------

// Goblin
pub const GOBLIN_HP: u8 = 6;
pub const GOBLIN_ATK: u8 = 3;
pub const GOBLIN_DEF: u8 = 0;
pub const GOBLIN_SIGHT: u8 = 6;
pub const GOBLIN_SPAWN_WEIGHT: u8 = 60;
pub const GOBLIN_GLYPH: char = 'g';

// Orc
pub const ORC_HP: u8 = 12;
pub const ORC_ATK: u8 = 4;
pub const ORC_DEF: u8 = 1;
pub const ORC_SIGHT: u8 = 7;
pub const ORC_SPAWN_WEIGHT: u8 = 30;
pub const ORC_GLYPH: char = 'o';

// Troll
pub const TROLL_HP: u8 = 20;
pub const TROLL_ATK: u8 = 6;
pub const TROLL_DEF: u8 = 3;
pub const TROLL_SIGHT: u8 = 5;
pub const TROLL_SPAWN_WEIGHT: u8 = 10;
pub const TROLL_GLYPH: char = 'T';

// ---------------------------------------------------------------------------
// Game config
// ---------------------------------------------------------------------------

pub const FOV_RADIUS: u8 = 8;
pub const MAX_ROOMS: u8 = 30;
pub const ROOM_SIZE_MIN: u8 = 4;
pub const ROOM_SIZE_MAX: u8 = 10;
pub const MAX_MONSTERS_PER_ROOM: u8 = 2;
pub const UI_BOTTOM_ROWS: u8 = 5;
pub const MAX_AUTORUN_STEPS: u8 = 100;
pub const MIN_MAP_WIDTH: u8 = 20;
pub const MIN_MAP_HEIGHT: u8 = 15;
pub const REGEN_INTERVAL: u8 = 3;
pub const TARGET_DEPTH: u8 = 5;

// ---------------------------------------------------------------------------
// Depth scaling — per-floor monster stat increases
// ---------------------------------------------------------------------------

pub const MONSTER_HP_PER_FLOOR: u8 = 1;
pub const MONSTER_ATK_PER_FLOOR: u8 = 1;

// ---------------------------------------------------------------------------
// Wandering spawn config
// ---------------------------------------------------------------------------

pub const WANDERING_SPAWN_INTERVAL: u8 = 30;
pub const WANDERING_SPAWN_CHANCE: u8 = 50;
pub const WANDERING_GRACE_PERIOD: u8 = 50;
pub const WANDERING_MAX_ACTIVE: u8 = 5;
pub const WANDERING_SOUND_FAR: u8 = 20;
pub const WANDERING_SOUND_MEDIUM: u8 = 10;
pub const WANDERING_SOUND_NEAR: u8 = 5;
pub const WANDERING_IDLE_THRESHOLD: u8 = 5;
pub const WANDERING_IDLE_ACCELERATION: u8 = 2;
/// Right-shift amount equivalent to dividing by `WANDERING_IDLE_ACCELERATION`.
/// Derived from the acceleration value so the micro tier can use `>>` instead
/// of division (no __udivsi3 on 6502).
pub const WANDERING_IDLE_ACCEL_SHIFT: u8 = WANDERING_IDLE_ACCELERATION.trailing_zeros() as u8;
pub const WANDERING_AMBIENT_SOUND_INTERVAL: u8 = 5;

// Compile-time: acceleration must be a power of 2 so the shift is exact.
const _: () = assert!(
    WANDERING_IDLE_ACCELERATION.is_power_of_two(),
    "WANDERING_IDLE_ACCELERATION must be a power of 2 for 6502 shift optimization"
);

// ---------------------------------------------------------------------------
// Item balance
// ---------------------------------------------------------------------------

pub const HEALTH_POTION_HEAL: u8 = 10;
pub const SHORT_SWORD_ATK_BONUS: u8 = 3;
pub const LEATHER_ARMOR_DEF_BONUS: u8 = 2;

pub const HEALTH_POTION_SPAWN_WEIGHT: u8 = 50;
pub const SHORT_SWORD_SPAWN_WEIGHT: u8 = 30;
pub const LEATHER_ARMOR_SPAWN_WEIGHT: u8 = 20;

pub const MAX_ITEMS_PER_ROOM: u8 = 1;

// ---------------------------------------------------------------------------
// Per-tier map dimensions and entity caps
// ---------------------------------------------------------------------------

// Tier micro — maximum supported dimensions (array sizing)
pub const MICRO_MAX_MAP_WIDTH: u8 = 80;
pub const MICRO_MAX_MAP_HEIGHT: u8 = 60;

// Tier micro — C64 default dimensions
pub const MICRO_MAP_WIDTH: u8 = 64;
pub const MICRO_MAP_HEIGHT: u8 = 48;

// Tier micro — gameplay constants (match standard tier for parity)
pub const MICRO_MAX_ROOMS: u8 = MAX_ROOMS;
pub const MICRO_MAX_ENTITIES: u8 = 64;
pub const MICRO_MAX_ITEMS: u8 = 32;
pub const MICRO_ROOM_SIZE_MIN: u8 = ROOM_SIZE_MIN;
pub const MICRO_ROOM_SIZE_MAX: u8 = ROOM_SIZE_MAX;
pub const MICRO_FOV_RADIUS: u8 = FOV_RADIUS;

// Tier compact (GBA)
// Note: these exceed u8 range, so use u16
pub const COMPACT_MAP_WIDTH: u16 = 128;
pub const COMPACT_MAP_HEIGHT: u16 = 96;
pub const COMPACT_MAX_ROOMS: u8 = 24;
pub const COMPACT_MAX_ENTITIES: u8 = 128;

// Tier standard (Vita/PC) — uses the config constants above
// (MAX_ROOMS, 80x40 default dimensions, MAX_ENTITIES = 1024)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_weights_sum_to_100() {
        let total =
            GOBLIN_SPAWN_WEIGHT as u16 + ORC_SPAWN_WEIGHT as u16 + TROLL_SPAWN_WEIGHT as u16;
        assert_eq!(total, 100);
    }

    #[test]
    fn item_spawn_weights_sum_to_100() {
        let total = HEALTH_POTION_SPAWN_WEIGHT as u16
            + SHORT_SWORD_SPAWN_WEIGHT as u16
            + LEATHER_ARMOR_SPAWN_WEIGHT as u16;
        assert_eq!(total, 100);
    }

    #[test]
    fn room_size_range_valid() {
        assert!(ROOM_SIZE_MIN <= ROOM_SIZE_MAX);
    }

    #[test]
    fn wandering_sound_distances_ordered() {
        assert!(WANDERING_SOUND_FAR > WANDERING_SOUND_MEDIUM);
        assert!(WANDERING_SOUND_MEDIUM > WANDERING_SOUND_NEAR);
        assert!(WANDERING_SOUND_NEAR > 0);
    }

    #[test]
    fn player_can_damage_all_monsters() {
        // Player ATK must exceed every monster's DEF for combat to work
        assert!(PLAYER_ATK > GOBLIN_DEF);
        assert!(PLAYER_ATK > ORC_DEF);
        assert!(PLAYER_ATK > TROLL_DEF);
    }

    #[test]
    fn micro_tier_fits_u8() {
        assert!(MICRO_MAP_WIDTH > 0);
        assert!(MICRO_MAP_HEIGHT > 0);
        // 64 * 48 = 3072, fits in a flat [u8] array
        assert!((MICRO_MAP_WIDTH as u16 * MICRO_MAP_HEIGHT as u16) <= u16::MAX);
    }

    #[test]
    fn micro_balance_matches_standard() {
        assert_eq!(MICRO_FOV_RADIUS, FOV_RADIUS);
        assert_eq!(MICRO_MAX_ROOMS, MAX_ROOMS);
        assert_eq!(MICRO_ROOM_SIZE_MIN, ROOM_SIZE_MIN);
        assert_eq!(MICRO_ROOM_SIZE_MAX, ROOM_SIZE_MAX);
    }

    #[test]
    fn micro_max_dims_fit_u8() {
        assert!(MICRO_MAX_MAP_WIDTH > 0);
        assert!(MICRO_MAX_MAP_HEIGHT > 0);
        // 80 * 60 = 4800, fits in u16
        assert!((MICRO_MAX_MAP_WIDTH as u16 * MICRO_MAX_MAP_HEIGHT as u16) <= u16::MAX);
    }

    #[test]
    fn micro_defaults_within_max() {
        assert!(MICRO_MAP_WIDTH <= MICRO_MAX_MAP_WIDTH);
        assert!(MICRO_MAP_HEIGHT <= MICRO_MAX_MAP_HEIGHT);
    }
}
