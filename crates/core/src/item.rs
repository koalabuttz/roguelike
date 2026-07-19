use serde::{Deserialize, Serialize};

use crate::rules::{balance, items as rules_items, properties::PropertyBag};
use crate::types::{Coord, GameColor, Stat};
use crate::{data::ItemDef, rules::content::ItemCategory};

// Re-export from rules so all existing `item::ItemKind` / `item::Equipment` paths work.
pub use crate::rules::items::{Equipment, ItemKind};

/// Maximum number of items on the ground the engine supports.
///
/// Constrained platforms override this: C64 = 8.
pub const MAX_GROUND_ITEMS: usize = 256;

/// Maximum items spawned per room during map generation.
pub const MAX_ITEMS_PER_ROOM: Stat = balance::MAX_ITEMS_PER_ROOM as Stat;

/// An item on the ground at a specific position.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub x: Coord,
    pub y: Coord,
    pub kind: ItemKind,
}

// ---------------------------------------------------------------------------
// Standard-tier wrappers — delegate to rules::items, return Stat
// ---------------------------------------------------------------------------

/// Display glyph for an item kind.
pub fn item_glyph(kind: ItemKind) -> char {
    rules_items::glyph(kind)
}

pub fn item_glyph_with(catalog: &[ItemDef], kind: ItemKind) -> char {
    catalog[kind as usize].glyph_char()
}

/// Display color for an item kind.
pub fn item_color(kind: ItemKind) -> GameColor {
    rules_items::color(kind)
}

pub fn item_color_with(catalog: &[ItemDef], kind: ItemKind) -> GameColor {
    catalog[kind as usize].game_color()
}

/// Human-readable name for an item kind.
pub fn item_name(kind: ItemKind) -> &'static str {
    rules_items::name(kind)
}

pub fn item_name_with(catalog: &[ItemDef], kind: ItemKind) -> &str {
    &catalog[kind as usize].name
}

/// Item name with qualitative adjectives for non-default properties.
/// Returns the base name if properties match defaults, or a descriptive
/// name like "smoldering, luminous Short Sword" if they differ.
pub fn described_item_name(kind: ItemKind, props: &PropertyBag) -> String {
    let mut buf = [0u8; rules_items::DESCRIBED_NAME_MAX];
    let len = rules_items::describe_name(kind, props, &mut buf);
    core::str::from_utf8(&buf[..len])
        .unwrap_or(rules_items::name(kind))
        .to_string()
}

pub fn described_item_name_with(
    catalog: &[ItemDef],
    kind: ItemKind,
    props: &PropertyBag,
) -> String {
    let definition = &catalog[kind as usize];
    if *props == definition.default_properties() {
        return definition.name.clone();
    }
    let described = described_item_name(kind, props);
    described
        .strip_suffix(rules_items::name(kind))
        .map(|prefix| format!("{prefix}{}", definition.name))
        .unwrap_or_else(|| definition.name.clone())
}

pub fn is_consumable_with(catalog: &[ItemDef], kind: ItemKind) -> bool {
    catalog[kind as usize].item_category() == ItemCategory::Consumable
}

pub fn is_weapon_with(catalog: &[ItemDef], kind: ItemKind) -> bool {
    catalog[kind as usize].item_category() == ItemCategory::Weapon
}

pub fn is_armor_with(catalog: &[ItemDef], kind: ItemKind) -> bool {
    catalog[kind as usize].item_category() == ItemCategory::Armor
}

pub fn default_properties_with(catalog: &[ItemDef], kind: ItemKind) -> PropertyBag {
    catalog[kind as usize].default_properties()
}

/// Spawn weight for the weighted item spawn table.
/// Higher weight = more common. Set to 0 to disable spawning.
pub fn item_spawn_weight(kind: ItemKind) -> u32 {
    rules_items::spawn_weight(kind) as u32
}

/// HP restored when a health potion is consumed. Returns 0 for non-consumables.
pub fn item_heal_amount(kind: ItemKind) -> Stat {
    rules_items::heal_amount(kind) as Stat
}

/// Attack bonus granted by equipping this item. Returns 0 for non-weapons.
pub fn item_attack_bonus(kind: ItemKind) -> Stat {
    rules_items::attack_bonus(kind) as Stat
}

/// Defense bonus granted by equipping this item. Returns 0 for non-armor.
pub fn item_defense_bonus(kind: ItemKind) -> Stat {
    rules_items::defense_bonus(kind) as Stat
}

/// Whether this item is a consumable (used immediately on pickup).
pub fn is_consumable(kind: ItemKind) -> bool {
    rules_items::is_consumable(kind)
}

/// Whether this item is a weapon (occupies weapon slot).
pub fn is_weapon(kind: ItemKind) -> bool {
    rules_items::is_weapon(kind)
}

/// Whether this item is armor (occupies armor slot).
pub fn is_armor(kind: ItemKind) -> bool {
    rules_items::is_armor(kind)
}

/// Minimum depth at which this item can spawn.
pub fn item_min_depth(kind: ItemKind) -> u8 {
    rules_items::min_depth(kind)
}

/// Permanent ATK boost granted when consumed. Returns 0 for non-boosting items.
pub fn item_strength_boost(kind: ItemKind) -> Stat {
    rules_items::strength_boost(kind) as Stat
}

/// Permanent DEF boost granted when consumed. Returns 0 for non-boosting items.
pub fn item_defense_boost(kind: ItemKind) -> Stat {
    rules_items::defense_boost(kind) as Stat
}

/// Returns true if `new` is strictly better than `current` for the weapon slot.
pub fn is_better_weapon(new: ItemKind, current: Option<ItemKind>) -> bool {
    rules_items::is_better_weapon(new, current)
}

/// Returns true if `new` is strictly better than `current` for the armor slot.
pub fn is_better_armor(new: ItemKind, current: Option<ItemKind>) -> bool {
    rules_items::is_better_armor(new, current)
}

/// All item kinds with positive spawn weight, for the spawn table.
/// Built from the fixed-size `rules::items::SPAWN_TABLE`.
pub fn spawn_table() -> Vec<(ItemKind, u32)> {
    rules_items::SPAWN_TABLE
        .iter()
        .filter(|(_, w)| *w > 0)
        .map(|&(kind, w)| (kind, w as u32))
        .collect()
}

/// Spawn table filtered by depth — only items whose `min_depth` ≤ `depth`.
/// The spawner auto-normalizes weights, so no rebalancing needed.
pub fn spawn_table_for_depth(depth: u8) -> Vec<(ItemKind, u32)> {
    rules_items::SPAWN_TABLE
        .iter()
        .filter(|&&(kind, w)| w > 0 && rules_items::min_depth(kind) <= depth)
        .map(|&(kind, w)| (kind, w as u32))
        .collect()
}

pub fn spawn_table_for_depth_with(catalog: &[ItemDef], depth: u8) -> Vec<(ItemKind, u32)> {
    rules_items::ALL_KINDS
        .iter()
        .copied()
        .filter_map(|kind| {
            let definition = &catalog[kind as usize];
            (definition.spawn_weight > 0 && definition.min_depth <= depth)
                .then_some((kind, definition.spawn_weight))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_potion_properties() {
        assert_eq!(item_glyph(ItemKind::HealthPotion), '!');
        assert_eq!(item_color(ItemKind::HealthPotion), GameColor::Red);
        assert_eq!(item_name(ItemKind::HealthPotion), "Health Potion");
        assert_eq!(item_heal_amount(ItemKind::HealthPotion), 10);
        assert_eq!(item_attack_bonus(ItemKind::HealthPotion), 0);
        assert_eq!(item_defense_bonus(ItemKind::HealthPotion), 0);
        assert!(is_consumable(ItemKind::HealthPotion));
        assert!(!is_weapon(ItemKind::HealthPotion));
        assert!(!is_armor(ItemKind::HealthPotion));
    }

    #[test]
    fn short_sword_properties() {
        assert_eq!(item_glyph(ItemKind::ShortSword), '/');
        assert_eq!(item_color(ItemKind::ShortSword), GameColor::Cyan);
        assert_eq!(item_name(ItemKind::ShortSword), "Short Sword");
        assert_eq!(item_heal_amount(ItemKind::ShortSword), 0);
        assert_eq!(item_attack_bonus(ItemKind::ShortSword), 3);
        assert_eq!(item_defense_bonus(ItemKind::ShortSword), 0);
        assert!(!is_consumable(ItemKind::ShortSword));
        assert!(is_weapon(ItemKind::ShortSword));
        assert!(!is_armor(ItemKind::ShortSword));
    }

    #[test]
    fn leather_armor_properties() {
        assert_eq!(item_glyph(ItemKind::LeatherArmor), '[');
        assert_eq!(item_color(ItemKind::LeatherArmor), GameColor::Yellow);
        assert_eq!(item_name(ItemKind::LeatherArmor), "Leather Armor");
        assert_eq!(item_heal_amount(ItemKind::LeatherArmor), 0);
        assert_eq!(item_attack_bonus(ItemKind::LeatherArmor), 0);
        assert_eq!(item_defense_bonus(ItemKind::LeatherArmor), 2);
        assert!(!is_consumable(ItemKind::LeatherArmor));
        assert!(!is_weapon(ItemKind::LeatherArmor));
        assert!(is_armor(ItemKind::LeatherArmor));
    }

    #[test]
    fn spawn_table_has_all_items() {
        let table = spawn_table();
        assert_eq!(table.len(), 9);
        let total: u32 = table.iter().map(|(_, w)| w).sum();
        assert!(total > 0);
    }

    #[test]
    fn is_better_weapon_with_none() {
        assert!(is_better_weapon(ItemKind::ShortSword, None));
        assert!(!is_better_weapon(ItemKind::HealthPotion, None));
    }

    #[test]
    fn is_better_weapon_with_current() {
        // Same weapon is not strictly better
        assert!(!is_better_weapon(
            ItemKind::ShortSword,
            Some(ItemKind::ShortSword)
        ));
    }

    #[test]
    fn is_better_armor_with_none() {
        assert!(is_better_armor(ItemKind::LeatherArmor, None));
        assert!(!is_better_armor(ItemKind::HealthPotion, None));
    }

    #[test]
    fn is_better_armor_with_current() {
        assert!(!is_better_armor(
            ItemKind::LeatherArmor,
            Some(ItemKind::LeatherArmor)
        ));
    }

    #[test]
    fn equipment_default_no_bonuses() {
        let eq = Equipment::default();
        assert_eq!(eq.attack_bonus(), 0);
        assert_eq!(eq.defense_bonus(), 0);
    }

    #[test]
    fn equipment_weapon_bonus() {
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: rules_items::default_properties(ItemKind::ShortSword),
            armor: None,
            armor_props: crate::rules::properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 3);
        assert_eq!(eq.defense_bonus(), 0);
    }

    #[test]
    fn equipment_armor_bonus() {
        let eq = Equipment {
            weapon: None,
            weapon_props: crate::rules::properties::EMPTY,
            armor: Some(ItemKind::LeatherArmor),
            armor_props: rules_items::default_properties(ItemKind::LeatherArmor),
        };
        assert_eq!(eq.attack_bonus(), 0);
        assert_eq!(eq.defense_bonus(), 2);
    }

    #[test]
    fn equipment_both_slots() {
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: rules_items::default_properties(ItemKind::ShortSword),
            armor: Some(ItemKind::LeatherArmor),
            armor_props: rules_items::default_properties(ItemKind::LeatherArmor),
        };
        assert_eq!(eq.attack_bonus(), 3);
        assert_eq!(eq.defense_bonus(), 2);
    }
}
