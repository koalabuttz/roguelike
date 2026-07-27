//! Host-side parser, validator, and Rust emitter for portable game content.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

pub const MONSTER_IDS: [&str; 3] = ["goblin", "orc", "troll"];
pub const ITEM_IDS: [&str; 9] = [
    "health_potion",
    "short_sword",
    "leather_armor",
    "iron_mace",
    "long_sword",
    "chain_mail",
    "greater_health_potion",
    "strength_potion",
    "toughness_potion",
];

const COLORS: [&str; 11] = [
    "Black",
    "White",
    "Grey",
    "DarkGrey",
    "Red",
    "DarkRed",
    "Green",
    "DarkGreen",
    "Yellow",
    "DarkBlue",
    "Cyan",
];
const AIS: [&str; 4] = ["aggressive", "patrol", "coward", "chase"];
const CATEGORIES: [&str; 3] = ["consumable", "weapon", "armor"];
const PROPERTIES: [&str; 16] = [
    "sharp",
    "hard",
    "heavy",
    "swift",
    "hot",
    "cold",
    "wet",
    "metal",
    "organic",
    "venomous",
    "magical",
    "volatile",
    "bright",
    "corrosive",
    "binding",
    "cursed",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameData {
    pub player: PlayerDef,
    pub config: GameConfig,
    #[serde(default)]
    pub depth_scaling: DepthScaling,
    #[serde(default)]
    pub wandering: WanderingConfig,
    pub monsters: Vec<MonsterDef>,
    pub items: Vec<ItemDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerDef {
    pub hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub glyph: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameConfig {
    pub fov_radius: i32,
    pub max_rooms: i32,
    pub room_size_min: i32,
    pub room_size_max: i32,
    pub max_monsters_per_room: i32,
    #[serde(default = "default_max_items_per_room")]
    pub max_items_per_room: i32,
    pub ui_bottom_rows: i32,
    pub max_autorun_steps: i32,
    pub regen_interval: i32,
    #[serde(default = "default_target_depth")]
    pub target_depth: i32,
}

const fn default_max_items_per_room() -> i32 {
    1
}
const fn default_target_depth() -> i32 {
    22
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepthScaling {
    #[serde(default = "one")]
    pub monster_hp_per_floor: i32,
    #[serde(default = "one")]
    pub monster_atk_per_floor: i32,
    #[serde(default = "three")]
    pub depth_scale_interval: i32,
}

impl Default for DepthScaling {
    fn default() -> Self {
        Self {
            monster_hp_per_floor: 1,
            monster_atk_per_floor: 1,
            depth_scale_interval: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WanderingConfig {
    #[serde(default = "thirty")]
    pub spawn_interval: i32,
    #[serde(default = "fifty")]
    pub spawn_chance: i32,
    #[serde(default = "fifty")]
    pub grace_period: i32,
    #[serde(default = "five")]
    pub max_wandering: i32,
    #[serde(default = "twenty")]
    pub sound_far: i32,
    #[serde(default = "ten")]
    pub sound_medium: i32,
    #[serde(default = "five")]
    pub sound_near: i32,
    #[serde(default = "five")]
    pub idle_threshold: i32,
    #[serde(default = "two")]
    pub idle_acceleration: i32,
}

impl Default for WanderingConfig {
    fn default() -> Self {
        Self {
            spawn_interval: 30,
            spawn_chance: 50,
            grace_period: 50,
            max_wandering: 5,
            sound_far: 20,
            sound_medium: 10,
            sound_near: 5,
            idle_threshold: 5,
            idle_acceleration: 2,
        }
    }
}

const fn one() -> i32 {
    1
}
const fn two() -> i32 {
    2
}
const fn three() -> i32 {
    3
}
const fn five() -> i32 {
    5
}
const fn ten() -> i32 {
    10
}
const fn twenty() -> i32 {
    20
}
const fn thirty() -> i32 {
    30
}
const fn fifty() -> i32 {
    50
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonsterDef {
    pub id: String,
    pub name: String,
    pub glyph: String,
    pub color: String,
    pub hp: i32,
    pub attack: i32,
    pub defense: i32,
    pub ai: String,
    pub spawn_weight: u32,
    pub sight_radius: i32,
    #[serde(default)]
    pub coward_chance: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub glyph: String,
    pub color: String,
    pub category: String,
    pub spawn_weight: u32,
    pub min_depth: u8,
    #[serde(default)]
    pub heal_amount: u8,
    #[serde(default)]
    pub strength_boost: u8,
    #[serde(default)]
    pub defense_boost: u8,
    #[serde(default)]
    pub properties: BTreeMap<String, u8>,
}

pub fn parse_game_data(source: &str) -> Result<GameData, String> {
    let data: GameData = toml::from_str(source).map_err(|e| format!("TOML parse error: {e}"))?;
    let errors = validate_game_data(&data);
    if errors.is_empty() {
        Ok(data)
    } else {
        Err(errors.join("; "))
    }
}

pub fn validate_game_data(data: &GameData) -> Vec<String> {
    let mut errors = Vec::new();
    positive_u8("player.hp", data.player.hp, &mut errors);
    bounded_u8("player.attack", data.player.attack, &mut errors);
    bounded_u8("player.defense", data.player.defense, &mut errors);
    glyph("player", &data.player.glyph, &mut errors);
    known("player.color", &data.player.color, &COLORS, &mut errors);

    positive_u8("config.fov_radius", data.config.fov_radius, &mut errors);
    positive_u8("config.max_rooms", data.config.max_rooms, &mut errors);
    positive_u8(
        "config.room_size_min",
        data.config.room_size_min,
        &mut errors,
    );
    positive_u8(
        "config.room_size_max",
        data.config.room_size_max,
        &mut errors,
    );
    bounded_u8(
        "config.max_monsters_per_room",
        data.config.max_monsters_per_room,
        &mut errors,
    );
    bounded_u8(
        "config.max_items_per_room",
        data.config.max_items_per_room,
        &mut errors,
    );
    bounded_u8(
        "config.ui_bottom_rows",
        data.config.ui_bottom_rows,
        &mut errors,
    );
    positive_u8(
        "config.max_autorun_steps",
        data.config.max_autorun_steps,
        &mut errors,
    );
    positive_u8(
        "config.regen_interval",
        data.config.regen_interval,
        &mut errors,
    );
    positive_u8("config.target_depth", data.config.target_depth, &mut errors);
    if data.config.room_size_min > data.config.room_size_max {
        errors.push("config.room_size_min must be <= room_size_max".into());
    }

    bounded_u8(
        "depth_scaling.monster_hp_per_floor",
        data.depth_scaling.monster_hp_per_floor,
        &mut errors,
    );
    bounded_u8(
        "depth_scaling.monster_atk_per_floor",
        data.depth_scaling.monster_atk_per_floor,
        &mut errors,
    );
    positive_u8(
        "depth_scaling.depth_scale_interval",
        data.depth_scaling.depth_scale_interval,
        &mut errors,
    );

    positive_u8(
        "wandering.spawn_interval",
        data.wandering.spawn_interval,
        &mut errors,
    );
    percent(
        "wandering.spawn_chance",
        data.wandering.spawn_chance,
        &mut errors,
    );
    bounded_u8(
        "wandering.grace_period",
        data.wandering.grace_period,
        &mut errors,
    );
    bounded_u8(
        "wandering.max_wandering",
        data.wandering.max_wandering,
        &mut errors,
    );
    positive_u8("wandering.sound_far", data.wandering.sound_far, &mut errors);
    positive_u8(
        "wandering.sound_medium",
        data.wandering.sound_medium,
        &mut errors,
    );
    positive_u8(
        "wandering.sound_near",
        data.wandering.sound_near,
        &mut errors,
    );
    bounded_u8(
        "wandering.idle_threshold",
        data.wandering.idle_threshold,
        &mut errors,
    );
    positive_u8(
        "wandering.idle_acceleration",
        data.wandering.idle_acceleration,
        &mut errors,
    );
    if data.wandering.idle_acceleration <= 0
        || !(data.wandering.idle_acceleration as u32).is_power_of_two()
    {
        errors.push("wandering.idle_acceleration must be a power of two".into());
    }
    if !(data.wandering.sound_far > data.wandering.sound_medium
        && data.wandering.sound_medium > data.wandering.sound_near)
    {
        errors.push("wandering sound distances must be strictly far > medium > near".into());
    }

    validate_ids(
        "monster",
        data.monsters.iter().map(|v| v.id.as_str()),
        &MONSTER_IDS,
        &mut errors,
    );
    for monster in &data.monsters {
        glyph(
            &format!("monster {}", monster.id),
            &monster.glyph,
            &mut errors,
        );
        known(
            &format!("monster {} color", monster.id),
            &monster.color,
            &COLORS,
            &mut errors,
        );
        known_lower(
            &format!("monster {} ai", monster.id),
            &monster.ai,
            &AIS,
            &mut errors,
        );
        positive_u8(
            &format!("monster {} hp", monster.id),
            monster.hp,
            &mut errors,
        );
        bounded_u8(
            &format!("monster {} attack", monster.id),
            monster.attack,
            &mut errors,
        );
        bounded_u8(
            &format!("monster {} defense", monster.id),
            monster.defense,
            &mut errors,
        );
        positive_u8(
            &format!("monster {} sight_radius", monster.id),
            monster.sight_radius,
            &mut errors,
        );
        if monster.spawn_weight == 0 || monster.spawn_weight > u8::MAX as u32 {
            errors.push(format!(
                "monster {} spawn_weight must be 1..=255",
                monster.id
            ));
        }
        if monster.coward_chance > 100 {
            errors.push(format!(
                "monster {} coward_chance must be 0..=100",
                monster.id
            ));
        }
    }

    validate_ids(
        "item",
        data.items.iter().map(|v| v.id.as_str()),
        &ITEM_IDS,
        &mut errors,
    );
    for item in &data.items {
        glyph(&format!("item {}", item.id), &item.glyph, &mut errors);
        known(
            &format!("item {} color", item.id),
            &item.color,
            &COLORS,
            &mut errors,
        );
        known_lower(
            &format!("item {} category", item.id),
            &item.category,
            &CATEGORIES,
            &mut errors,
        );
        if item.spawn_weight == 0 || item.spawn_weight > u8::MAX as u32 {
            errors.push(format!("item {} spawn_weight must be 1..=255", item.id));
        }
        if item.min_depth == 0 {
            errors.push(format!("item {} min_depth must be > 0", item.id));
        }
        if item.category != "consumable"
            && (item.heal_amount > 0 || item.strength_boost > 0 || item.defense_boost > 0)
        {
            errors.push(format!(
                "item {} non-consumable cannot have consumable effects",
                item.id
            ));
        }
        for (property, value) in &item.properties {
            if !PROPERTIES.contains(&property.as_str()) {
                errors.push(format!("item {} has unknown property {property}", item.id));
            }
            if *value > 15 {
                errors.push(format!(
                    "item {} property {property} must be 0..=15",
                    item.id
                ));
            }
        }
    }
    errors
}

fn validate_ids<'a>(
    label: &str,
    ids: impl Iterator<Item = &'a str>,
    expected: &[&str],
    errors: &mut Vec<String>,
) {
    let actual: Vec<&str> = ids.collect();
    let set: BTreeSet<&str> = actual.iter().copied().collect();
    if set.len() != actual.len() {
        errors.push(format!("duplicate {label} id"));
    }
    for id in expected {
        if !set.contains(id) {
            errors.push(format!("missing {label} id {id}"));
        }
    }
    for id in set {
        if !expected.contains(&id) {
            errors.push(format!("unknown {label} id {id}"));
        }
    }
}

fn glyph(label: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 1 || !value.is_ascii() {
        errors.push(format!("{label} glyph must be one ASCII byte"));
    }
}
fn known(label: &str, value: &str, known: &[&str], errors: &mut Vec<String>) {
    if !known.contains(&value) {
        errors.push(format!("{label} has unknown value {value}"));
    }
}
fn known_lower(label: &str, value: &str, known: &[&str], errors: &mut Vec<String>) {
    if !known.contains(&value.to_ascii_lowercase().as_str()) {
        errors.push(format!("{label} has unknown value {value}"));
    }
}
fn bounded_u8(label: &str, value: i32, errors: &mut Vec<String>) {
    if !(0..=u8::MAX as i32).contains(&value) {
        errors.push(format!("{label} must be 0..=255"));
    }
}
fn positive_u8(label: &str, value: i32, errors: &mut Vec<String>) {
    if !(1..=u8::MAX as i32).contains(&value) {
        errors.push(format!("{label} must be 1..=255"));
    }
}
fn percent(label: &str, value: i32, errors: &mut Vec<String>) {
    if !(0..=100).contains(&value) {
        errors.push(format!("{label} must be 0..=100"));
    }
}

fn ordered<'a, T>(values: &'a [T], ids: &[&str], id: impl Fn(&T) -> &str) -> Vec<&'a T> {
    ids.iter()
        .map(|wanted| values.iter().find(|v| id(v) == *wanted).unwrap())
        .collect()
}

fn emit_lookup(
    out: &mut String,
    name: &str,
    _return_type: &str,
    values: impl IntoIterator<Item = String>,
    entity: &str,
) {
    let variants: &[&str] = match entity {
        "monster" => &["Goblin", "Orc", "Troll"],
        "item" => &[
            "HealthPotion",
            "ShortSword",
            "LeatherArmor",
            "IronMace",
            "LongSword",
            "ChainMail",
            "GreaterHealthPotion",
            "StrengthPotion",
            "ToughnessPotion",
        ],
        _ => unreachable!(),
    };
    let enum_path = match entity {
        "monster" => "crate::rules::monster_table::MonsterKind",
        "item" => "crate::rules::items::ItemKind",
        _ => unreachable!(),
    };
    writeln!(out, "macro_rules! {name} {{\n    ($kind:expr) => {{ {{").unwrap();
    out.push_str("        let value = match $kind {\n");
    for (variant, value) in variants.iter().zip(values) {
        writeln!(out, "            {enum_path}::{variant} => {value},").unwrap();
    }
    writeln!(
        out,
        "        }};\n        value\n    }} }};\n}}\npub(crate) use {name};"
    )
    .unwrap();
}

pub fn emit_rust(data: &GameData) -> String {
    assert!(validate_game_data(data).is_empty());
    let monsters = ordered(&data.monsters, &MONSTER_IDS, |v| &v.id);
    let items = ordered(&data.items, &ITEM_IDS, |v| &v.id);
    let mut out = String::from("// @generated by roguelike-content; do not edit.\n");
    writeln!(out, "pub const PLAYER: PlayerContent = PlayerContent {{ hp: {}, attack: {}, defense: {}, glyph: {:?}, color: GameColor::{} }};", data.player.hp, data.player.attack, data.player.defense, data.player.glyph.chars().next().unwrap(), data.player.color).unwrap();
    writeln!(out, "pub const CONFIG: GameConfigContent = GameConfigContent {{ fov_radius: {}, max_rooms: {}, room_size_min: {}, room_size_max: {}, max_monsters_per_room: {}, max_items_per_room: {}, ui_bottom_rows: {}, max_autorun_steps: {}, regen_interval: {}, target_depth: {} }};", data.config.fov_radius, data.config.max_rooms, data.config.room_size_min, data.config.room_size_max, data.config.max_monsters_per_room, data.config.max_items_per_room, data.config.ui_bottom_rows, data.config.max_autorun_steps, data.config.regen_interval, data.config.target_depth).unwrap();
    writeln!(out, "pub const DEPTH_SCALING: DepthScalingContent = DepthScalingContent {{ monster_hp_per_floor: {}, monster_atk_per_floor: {}, depth_scale_interval: {} }};", data.depth_scaling.monster_hp_per_floor, data.depth_scaling.monster_atk_per_floor, data.depth_scaling.depth_scale_interval).unwrap();
    writeln!(out, "pub const WANDERING: WanderingContent = WanderingContent {{ spawn_interval: {}, spawn_chance: {}, grace_period: {}, max_wandering: {}, sound_far: {}, sound_medium: {}, sound_near: {}, idle_threshold: {}, idle_acceleration: {} }};", data.wandering.spawn_interval, data.wandering.spawn_chance, data.wandering.grace_period, data.wandering.max_wandering, data.wandering.sound_far, data.wandering.sound_medium, data.wandering.sound_near, data.wandering.idle_threshold, data.wandering.idle_acceleration).unwrap();
    let monster_literals: Vec<_> = monsters
        .iter()
        .map(|v| {
            let ai = match v.ai.to_ascii_lowercase().as_str() {
                "patrol" => "Patrol",
                "coward" => "Coward",
                _ => "Aggressive",
            };
            format!("MonsterContent {{ id: {:?}, name: {:?}, glyph: {:?}, color: GameColor::{}, max_hp: {}, attack: {}, defense: {}, ai: AiPersonality::{}, spawn_weight: {}, sight_radius: {}, coward_chance: {} }}", v.id, v.name, v.glyph.chars().next().unwrap(), v.color, v.hp, v.attack, v.defense, ai, v.spawn_weight, v.sight_radius, v.coward_chance)
        })
        .collect();
    let item_literals: Vec<_> = items
        .iter()
        .map(|v| {
            let category = match v.category.as_str() {
                "weapon" => "Weapon",
                "armor" => "Armor",
                _ => "Consumable",
            };
            let bag = property_bag(&v.properties);
            format!("ItemContent {{ id: {:?}, name: {:?}, glyph: {:?}, color: GameColor::{}, category: ItemCategory::{}, spawn_weight: {}, min_depth: {}, heal_amount: {}, strength_boost: {}, defense_boost: {}, default_properties: {:?} }}", v.id, v.name, v.glyph.chars().next().unwrap(), v.color, category, v.spawn_weight, v.min_depth, v.heal_amount, v.strength_boost, v.defense_boost, bag)
        })
        .collect();

    out.push_str("pub const MONSTER_COUNT: usize = 3;\npub const ITEM_COUNT: usize = 9;\n");
    out.push_str("#[cfg(feature = \"data-files\")]\npub const MONSTERS: [MonsterContent; MONSTER_COUNT] = [\n");
    for literal in &monster_literals {
        writeln!(out, "    {literal},").unwrap();
    }
    out.push_str("];");
    out.push_str(
        "\n#[cfg(feature = \"data-files\")]\npub const ITEMS: [ItemContent; ITEM_COUNT] = [\n",
    );
    for literal in &item_literals {
        writeln!(out, "    {literal},").unwrap();
    }
    out.push_str("];\n");

    emit_lookup(
        &mut out,
        "monster_name",
        "&'static str",
        monsters.iter().map(|v| format!("{:?}", v.name)),
        "monster",
    );
    emit_lookup(
        &mut out,
        "monster_glyph",
        "char",
        monsters
            .iter()
            .map(|v| format!("{:?}", v.glyph.chars().next().unwrap())),
        "monster",
    );
    emit_lookup(
        &mut out,
        "monster_color",
        "GameColor",
        monsters.iter().map(|v| format!("GameColor::{}", v.color)),
        "monster",
    );
    for (name, values) in [
        (
            "monster_max_hp",
            monsters
                .iter()
                .map(|v| v.hp.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "monster_attack",
            monsters
                .iter()
                .map(|v| v.attack.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "monster_defense",
            monsters
                .iter()
                .map(|v| v.defense.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "monster_spawn_weight",
            monsters
                .iter()
                .map(|v| v.spawn_weight.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "monster_sight_radius",
            monsters
                .iter()
                .map(|v| v.sight_radius.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "monster_coward_chance",
            monsters
                .iter()
                .map(|v| v.coward_chance.to_string())
                .collect::<Vec<_>>(),
        ),
    ] {
        emit_lookup(&mut out, name, "u8", values, "monster");
    }
    emit_lookup(
        &mut out,
        "monster_ai",
        "AiPersonality",
        monsters.iter().map(|v| {
            let ai = match v.ai.to_ascii_lowercase().as_str() {
                "patrol" => "Patrol",
                "coward" => "Coward",
                _ => "Aggressive",
            };
            format!("AiPersonality::{ai}")
        }),
        "monster",
    );

    emit_lookup(
        &mut out,
        "item_name",
        "&'static str",
        items.iter().map(|v| format!("{:?}", v.name)),
        "item",
    );
    emit_lookup(
        &mut out,
        "item_glyph",
        "char",
        items
            .iter()
            .map(|v| format!("{:?}", v.glyph.chars().next().unwrap())),
        "item",
    );
    emit_lookup(
        &mut out,
        "item_color",
        "GameColor",
        items.iter().map(|v| format!("GameColor::{}", v.color)),
        "item",
    );
    emit_lookup(
        &mut out,
        "item_category",
        "ItemCategory",
        items.iter().map(|v| {
            let category = match v.category.as_str() {
                "weapon" => "Weapon",
                "armor" => "Armor",
                _ => "Consumable",
            };
            format!("ItemCategory::{category}")
        }),
        "item",
    );
    for (name, values) in [
        (
            "item_spawn_weight",
            items
                .iter()
                .map(|v| v.spawn_weight.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "item_min_depth",
            items
                .iter()
                .map(|v| v.min_depth.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "item_heal_amount",
            items
                .iter()
                .map(|v| v.heal_amount.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "item_strength_boost",
            items
                .iter()
                .map(|v| v.strength_boost.to_string())
                .collect::<Vec<_>>(),
        ),
        (
            "item_defense_boost",
            items
                .iter()
                .map(|v| v.defense_boost.to_string())
                .collect::<Vec<_>>(),
        ),
    ] {
        emit_lookup(&mut out, name, "u8", values, "item");
    }
    emit_lookup(
        &mut out,
        "item_consumable_effect",
        "Option<ConsumableEffect>",
        items.iter().map(|item| {
            let effect = if item.heal_amount > 0 {
                Some(("Heal", item.heal_amount))
            } else if item.strength_boost > 0 {
                Some(("BoostAttack", item.strength_boost))
            } else if item.defense_boost > 0 {
                Some(("BoostDefense", item.defense_boost))
            } else {
                None
            };
            match effect {
                Some((kind, amount)) => format!(
                    "Some(crate::rules::items::ConsumableEffect {{ kind: \
                     crate::rules::items::ConsumableEffectKind::{kind}, amount: {amount} }})"
                ),
                None => "None".to_owned(),
            }
        }),
        "item",
    );
    emit_lookup(
        &mut out,
        "item_default_properties",
        "PropertyBag",
        items
            .iter()
            .map(|v| format!("{:?}", property_bag(&v.properties))),
        "item",
    );
    out
}

pub fn property_bag(properties: &BTreeMap<String, u8>) -> [u8; 8] {
    let mut bag = [0; 8];
    for (name, value) in properties {
        if let Some(index) = PROPERTIES.iter().position(|candidate| candidate == name) {
            if index & 1 == 0 {
                bag[index / 2] |= value << 4;
            } else {
                bag[index / 2] |= value;
            }
        }
    }
    bag
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = include_str!("../../core/data/game.toml");

    #[test]
    fn canonical_file_validates_and_emits() {
        let data = parse_game_data(DEFAULT).unwrap();
        let generated = emit_rust(&data);
        assert!(generated.contains("pub const ITEMS: [ItemContent; ITEM_COUNT]"));
        assert!(generated.contains("macro_rules! item_default_properties"));
        assert!(generated.contains("macro_rules! item_consumable_effect"));
        assert!(generated.contains("macro_rules! item_defense_boost"));
        assert!(generated.contains("ItemKind::ToughnessPotion => 1"));
    }

    #[test]
    fn rejects_unknown_and_missing_ids() {
        let mut data = parse_game_data(DEFAULT).unwrap();
        data.items[0].id = "desktop_only".into();
        let errors = validate_game_data(&data).join("; ");
        assert!(errors.contains("missing item id health_potion"));
        assert!(errors.contains("unknown item id desktop_only"));
    }

    #[test]
    fn packs_named_properties() {
        let mut props = BTreeMap::new();
        props.insert("sharp".into(), 6);
        props.insert("hard".into(), 7);
        props.insert("metal".into(), 8);
        assert_eq!(property_bag(&props), [0x67, 0, 0, 0x08, 0, 0, 0, 0]);
    }

    #[test]
    fn rejects_consumable_effect_on_equipment() {
        let mut data = parse_game_data(DEFAULT).unwrap();
        let sword = data
            .items
            .iter_mut()
            .find(|item| item.id == "short_sword")
            .unwrap();
        sword.defense_boost = 1;
        let errors = validate_game_data(&data).join("; ");
        assert!(errors.contains("short_sword non-consumable cannot have consumable effects"));
    }
}
