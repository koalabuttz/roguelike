//! Shared balance constants for all capability tiers.
//!
//! This module is the single source of truth for game balance. All tiers and
//! platforms use these constants directly. The PC's data-file loading
//! (`data.rs`, gated behind `data-files`) uses these as compiled-in defaults;
//! modders can override values via `game.toml`.
//!
//! All types are `u8` — values that fit in the smallest tier's natural width.

use super::content;

// ---------------------------------------------------------------------------
// Player defaults
// ---------------------------------------------------------------------------

pub const PLAYER_HP: u8 = content::PLAYER.hp;
pub const PLAYER_ATK: u8 = content::PLAYER.attack;
pub const PLAYER_DEF: u8 = content::PLAYER.defense;
pub const PLAYER_GLYPH: char = content::PLAYER.glyph;

// ---------------------------------------------------------------------------
// Monster stats — indexed by MonsterKind (future) or looked up by name (now)
// ---------------------------------------------------------------------------

// Goblin
pub const GOBLIN_HP: u8 = content::monster_max_hp!(super::monster_table::MonsterKind::Goblin);
pub const GOBLIN_ATK: u8 = content::monster_attack!(super::monster_table::MonsterKind::Goblin);
pub const GOBLIN_DEF: u8 = content::monster_defense!(super::monster_table::MonsterKind::Goblin);
pub const GOBLIN_SIGHT: u8 =
    content::monster_sight_radius!(super::monster_table::MonsterKind::Goblin);
pub const GOBLIN_SPAWN_WEIGHT: u8 =
    content::monster_spawn_weight!(super::monster_table::MonsterKind::Goblin);
pub const GOBLIN_GLYPH: char = content::monster_glyph!(super::monster_table::MonsterKind::Goblin);
/// Percent chance that a newly spawned Goblin is a Coward (0-100).
/// Cowards chase when healthy but flee when HP drops below 33%.
pub const GOBLIN_COWARD_CHANCE: u8 =
    content::monster_coward_chance!(super::monster_table::MonsterKind::Goblin);

/// Flee threshold: a coward flees once `hp * FLEE_THRESHOLD_RECIP < max_hp`.
/// Recip 3 ⇒ below ~33% HP.
pub const FLEE_THRESHOLD_RECIP: u8 = 3;

// Orc
pub const ORC_HP: u8 = content::monster_max_hp!(super::monster_table::MonsterKind::Orc);
pub const ORC_ATK: u8 = content::monster_attack!(super::monster_table::MonsterKind::Orc);
pub const ORC_DEF: u8 = content::monster_defense!(super::monster_table::MonsterKind::Orc);
pub const ORC_SIGHT: u8 = content::monster_sight_radius!(super::monster_table::MonsterKind::Orc);
pub const ORC_SPAWN_WEIGHT: u8 =
    content::monster_spawn_weight!(super::monster_table::MonsterKind::Orc);
pub const ORC_GLYPH: char = content::monster_glyph!(super::monster_table::MonsterKind::Orc);

// Troll
pub const TROLL_HP: u8 = content::monster_max_hp!(super::monster_table::MonsterKind::Troll);
pub const TROLL_ATK: u8 = content::monster_attack!(super::monster_table::MonsterKind::Troll);
pub const TROLL_DEF: u8 = content::monster_defense!(super::monster_table::MonsterKind::Troll);
pub const TROLL_SIGHT: u8 =
    content::monster_sight_radius!(super::monster_table::MonsterKind::Troll);
pub const TROLL_SPAWN_WEIGHT: u8 =
    content::monster_spawn_weight!(super::monster_table::MonsterKind::Troll);
pub const TROLL_GLYPH: char = content::monster_glyph!(super::monster_table::MonsterKind::Troll);

// ---------------------------------------------------------------------------
// Game config
// ---------------------------------------------------------------------------

pub const FOV_RADIUS: u8 = content::CONFIG.fov_radius;
/// Upper bound on FOV radius for quarter-square lookup table sizing.
/// Increase if adding potions/effects that expand sight beyond FOV_RADIUS.
pub const MAX_FOV_RADIUS: u8 = 16;
pub const MAX_ROOMS: u8 = content::CONFIG.max_rooms;
pub const ROOM_SIZE_MIN: u8 = content::CONFIG.room_size_min;
pub const ROOM_SIZE_MAX: u8 = content::CONFIG.room_size_max;
pub const MAX_MONSTERS_PER_ROOM: u8 = content::CONFIG.max_monsters_per_room;
pub const UI_BOTTOM_ROWS: u8 = content::CONFIG.ui_bottom_rows;
pub const MAX_AUTORUN_STEPS: u8 = content::CONFIG.max_autorun_steps;
pub const MIN_MAP_WIDTH: u8 = 20;
pub const MIN_MAP_HEIGHT: u8 = 15;
pub const REGEN_INTERVAL: u8 = content::CONFIG.regen_interval;
pub const TARGET_DEPTH: u8 = content::CONFIG.target_depth;

// ---------------------------------------------------------------------------
// Depth scaling — per-floor monster stat increases
// ---------------------------------------------------------------------------

pub const MONSTER_HP_PER_FLOOR: u8 = content::DEPTH_SCALING.monster_hp_per_floor;
pub const MONSTER_ATK_PER_FLOOR: u8 = content::DEPTH_SCALING.monster_atk_per_floor;
/// Monsters gain stat bonuses every N floors (integer division).
/// At interval 3 over 22 floors: max bonus = (22-1)/3 = 7.
pub const DEPTH_SCALE_INTERVAL: u8 = content::DEPTH_SCALING.depth_scale_interval;

// ---------------------------------------------------------------------------
// Wandering spawn config
// ---------------------------------------------------------------------------

pub const WANDERING_SPAWN_INTERVAL: u8 = content::WANDERING.spawn_interval;
pub const WANDERING_SPAWN_CHANCE: u8 = content::WANDERING.spawn_chance;
pub const WANDERING_GRACE_PERIOD: u8 = content::WANDERING.grace_period;
pub const WANDERING_MAX_ACTIVE: u8 = content::WANDERING.max_wandering;
pub const WANDERING_SOUND_FAR: u8 = content::WANDERING.sound_far;
pub const WANDERING_SOUND_MEDIUM: u8 = content::WANDERING.sound_medium;
pub const WANDERING_SOUND_NEAR: u8 = content::WANDERING.sound_near;
pub const WANDERING_IDLE_THRESHOLD: u8 = content::WANDERING.idle_threshold;
pub const WANDERING_IDLE_ACCELERATION: u8 = content::WANDERING.idle_acceleration;
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

// Compile-time: interval must be non-zero to avoid division by zero.
const _: () = assert!(DEPTH_SCALE_INTERVAL > 0, "DEPTH_SCALE_INTERVAL must be > 0");

// ---------------------------------------------------------------------------
// Item balance
// ---------------------------------------------------------------------------

// Health Potion
pub const HEALTH_POTION_HEAL: u8 = content::item_heal_amount!(super::items::ItemKind::HealthPotion);
pub const HEALTH_POTION_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::HealthPotion);
pub const HEALTH_POTION_MIN_DEPTH: u8 =
    content::item_min_depth!(super::items::ItemKind::HealthPotion);

// Short Sword
pub const SHORT_SWORD_ATK_BONUS: u8 = super::items::attack_from_bag(
    &content::item_default_properties!(super::items::ItemKind::ShortSword),
);
pub const SHORT_SWORD_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::ShortSword);
pub const SHORT_SWORD_MIN_DEPTH: u8 = content::item_min_depth!(super::items::ItemKind::ShortSword);

// Leather Armor
pub const LEATHER_ARMOR_DEF_BONUS: u8 = super::items::defense_from_bag(
    &content::item_default_properties!(super::items::ItemKind::LeatherArmor),
);
pub const LEATHER_ARMOR_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::LeatherArmor);
pub const LEATHER_ARMOR_MIN_DEPTH: u8 =
    content::item_min_depth!(super::items::ItemKind::LeatherArmor);

// Iron Mace
pub const IRON_MACE_ATK_BONUS: u8 = super::items::attack_from_bag(
    &content::item_default_properties!(super::items::ItemKind::IronMace),
);
pub const IRON_MACE_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::IronMace);
pub const IRON_MACE_MIN_DEPTH: u8 = content::item_min_depth!(super::items::ItemKind::IronMace);

// Long Sword
pub const LONG_SWORD_ATK_BONUS: u8 = super::items::attack_from_bag(
    &content::item_default_properties!(super::items::ItemKind::LongSword),
);
pub const LONG_SWORD_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::LongSword);
pub const LONG_SWORD_MIN_DEPTH: u8 = content::item_min_depth!(super::items::ItemKind::LongSword);

// Chain Mail
pub const CHAIN_MAIL_DEF_BONUS: u8 = super::items::defense_from_bag(
    &content::item_default_properties!(super::items::ItemKind::ChainMail),
);
pub const CHAIN_MAIL_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::ChainMail);
pub const CHAIN_MAIL_MIN_DEPTH: u8 = content::item_min_depth!(super::items::ItemKind::ChainMail);

// Greater Health Potion
pub const GREATER_HEALTH_POTION_HEAL: u8 =
    content::item_heal_amount!(super::items::ItemKind::GreaterHealthPotion);
pub const GREATER_HEALTH_POTION_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::GreaterHealthPotion);
pub const GREATER_HEALTH_POTION_MIN_DEPTH: u8 =
    content::item_min_depth!(super::items::ItemKind::GreaterHealthPotion);

// Potion of Strength
pub const STRENGTH_POTION_ATK_BOOST: u8 =
    content::item_strength_boost!(super::items::ItemKind::StrengthPotion);
pub const STRENGTH_POTION_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::StrengthPotion);
pub const STRENGTH_POTION_MIN_DEPTH: u8 =
    content::item_min_depth!(super::items::ItemKind::StrengthPotion);

// Potion of Toughness
pub const TOUGHNESS_POTION_DEF_BOOST: u8 =
    content::item_defense_boost!(super::items::ItemKind::ToughnessPotion);
pub const TOUGHNESS_POTION_SPAWN_WEIGHT: u8 =
    content::item_spawn_weight!(super::items::ItemKind::ToughnessPotion);
pub const TOUGHNESS_POTION_MIN_DEPTH: u8 =
    content::item_min_depth!(super::items::ItemKind::ToughnessPotion);

pub const MAX_ITEMS_PER_ROOM: u8 = content::CONFIG.max_items_per_room;
pub const MAX_INVENTORY: usize = super::items::MAX_INVENTORY;

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

// Tier compact (GBA) — matches standard map dimensions; differentiates on
// entity budget, coord width (i32), and no_std constraints, not map size.
pub const COMPACT_MAP_WIDTH: u16 = 80;
pub const COMPACT_MAP_HEIGHT: u16 = 40;
pub const COMPACT_MAX_ROOMS: u8 = 12;
pub const COMPACT_MAX_ENTITIES: u8 = 128;
pub const COMPACT_ROOM_SIZE_MIN: u8 = ROOM_SIZE_MIN;
pub const COMPACT_ROOM_SIZE_MAX: u8 = ROOM_SIZE_MAX;
pub const COMPACT_MAX_ITEMS: u8 = 48;

// Tier standard (Vita/PC) — uses the config constants above
pub const STANDARD_MAP_WIDTH: u8 = 80;
pub const STANDARD_MAP_HEIGHT: u8 = 40;

// These relationships are part of the portable profile. Keep them at compile
// time so an invalid built-in balance cannot reach any target binary.
const _: () = {
    assert!(HEALTH_POTION_SPAWN_WEIGHT > 0);
    assert!(SHORT_SWORD_SPAWN_WEIGHT > 0);
    assert!(LEATHER_ARMOR_SPAWN_WEIGHT > 0);
    assert!(IRON_MACE_SPAWN_WEIGHT > 0);
    assert!(LONG_SWORD_SPAWN_WEIGHT > 0);
    assert!(CHAIN_MAIL_SPAWN_WEIGHT > 0);
    assert!(GREATER_HEALTH_POTION_SPAWN_WEIGHT > 0);
    assert!(STRENGTH_POTION_SPAWN_WEIGHT > 0);

    assert!(ROOM_SIZE_MIN <= ROOM_SIZE_MAX);
    assert!(WANDERING_SOUND_FAR > WANDERING_SOUND_MEDIUM);
    assert!(WANDERING_SOUND_MEDIUM > WANDERING_SOUND_NEAR);
    assert!(WANDERING_SOUND_NEAR > 0);

    assert!(PLAYER_ATK > GOBLIN_DEF);
    assert!(PLAYER_ATK > ORC_DEF);
    assert!(PLAYER_ATK > TROLL_DEF);

    assert!(MICRO_MAP_WIDTH > 0);
    assert!(MICRO_MAP_HEIGHT > 0);
    assert!((MICRO_MAP_WIDTH as u32) * (MICRO_MAP_HEIGHT as u32) <= u16::MAX as u32);
    assert!(MICRO_MAX_MAP_WIDTH > 0);
    assert!(MICRO_MAX_MAP_HEIGHT > 0);
    assert!((MICRO_MAX_MAP_WIDTH as u32) * (MICRO_MAX_MAP_HEIGHT as u32) <= u16::MAX as u32);
    assert!(MICRO_MAP_WIDTH <= MICRO_MAX_MAP_WIDTH);
    assert!(MICRO_MAP_HEIGHT <= MICRO_MAX_MAP_HEIGHT);
};

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
    fn micro_balance_matches_standard() {
        assert_eq!(MICRO_FOV_RADIUS, FOV_RADIUS);
        assert_eq!(MICRO_MAX_ROOMS, MAX_ROOMS);
        assert_eq!(MICRO_ROOM_SIZE_MIN, ROOM_SIZE_MIN);
        assert_eq!(MICRO_ROOM_SIZE_MAX, ROOM_SIZE_MAX);
    }
}
