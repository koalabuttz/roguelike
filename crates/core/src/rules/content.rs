//! Bounded game content generated from the canonical `data/game.toml`.

use super::color::GameColor;
use super::monster_table::AiPersonality;
use super::properties::PropertyBag;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemCategory {
    Consumable,
    Weapon,
    Armor,
}

#[derive(Clone, Copy, Debug)]
pub struct PlayerContent {
    pub hp: u8,
    pub attack: u8,
    pub defense: u8,
    pub glyph: char,
    pub color: GameColor,
}

#[derive(Clone, Copy, Debug)]
pub struct GameConfigContent {
    pub fov_radius: u8,
    pub max_rooms: u8,
    pub room_size_min: u8,
    pub room_size_max: u8,
    pub max_monsters_per_room: u8,
    pub max_items_per_room: u8,
    pub ui_bottom_rows: u8,
    pub max_autorun_steps: u8,
    pub regen_interval: u8,
    pub target_depth: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct DepthScalingContent {
    pub monster_hp_per_floor: u8,
    pub monster_atk_per_floor: u8,
    pub depth_scale_interval: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct WanderingContent {
    pub spawn_interval: u8,
    pub spawn_chance: u8,
    pub grace_period: u8,
    pub max_wandering: u8,
    pub sound_far: u8,
    pub sound_medium: u8,
    pub sound_near: u8,
    pub idle_threshold: u8,
    pub idle_acceleration: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct MonsterContent {
    pub id: &'static str,
    pub name: &'static str,
    pub glyph: char,
    pub color: GameColor,
    pub max_hp: u8,
    pub attack: u8,
    pub defense: u8,
    pub ai: AiPersonality,
    pub spawn_weight: u8,
    pub sight_radius: u8,
    pub coward_chance: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ItemContent {
    pub id: &'static str,
    pub name: &'static str,
    pub glyph: char,
    pub color: GameColor,
    pub category: ItemCategory,
    pub spawn_weight: u8,
    pub min_depth: u8,
    pub heal_amount: u8,
    pub strength_boost: u8,
    pub default_properties: PropertyBag,
}

include!(concat!(env!("OUT_DIR"), "/game_content.rs"));

const _: () = {
    assert!(MONSTER_COUNT == super::monster_table::KIND_COUNT);
    assert!(ITEM_COUNT == super::items::KIND_COUNT);
    assert!(CONFIG.room_size_min <= CONFIG.room_size_max);
    assert!(WANDERING.idle_acceleration.is_power_of_two());
    assert!(DEPTH_SCALING.depth_scale_interval > 0);
};

#[cfg(feature = "data-files")]
const _: () = {
    assert!(MONSTERS.len() == MONSTER_COUNT);
    assert!(ITEMS.len() == ITEM_COUNT);
};
