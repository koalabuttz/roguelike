use serde::{Deserialize, Serialize};

use crate::rules::balance;
use crate::types::{Coord, GameColor, Stat};

/// Maximum number of items on the ground the engine supports.
///
/// Constrained platforms override this: C64 = 8.
pub const MAX_GROUND_ITEMS: usize = 256;

/// Maximum items spawned per room during map generation.
pub const MAX_ITEMS_PER_ROOM: Stat = balance::MAX_ITEMS_PER_ROOM as Stat;

/// All item spawn weights for the weighted spawn table.
const ALL_KINDS: [ItemKind; 3] = [
    ItemKind::HealthPotion,
    ItemKind::ShortSword,
    ItemKind::LeatherArmor,
];

/// The type of item. Each variant maps to a `u8` discriminant, making it
/// portable to constrained platforms (C64, GBA).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    HealthPotion,
    ShortSword,
    LeatherArmor,
}

/// An item on the ground at a specific position.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub x: Coord,
    pub y: Coord,
    pub kind: ItemKind,
}

/// Tracked equipment slots for the player.
///
/// Combat reads effective stats (base + equipment bonus) from these slots.
/// Structured so that adding inventory later is a small diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipment {
    pub weapon: Option<ItemKind>,
    pub armor: Option<ItemKind>,
}

// ---------------------------------------------------------------------------
// Pure functions — tier-portable, will lift into rules/ module later
// ---------------------------------------------------------------------------

/// Display glyph for an item kind.
pub fn item_glyph(kind: ItemKind) -> char {
    match kind {
        ItemKind::HealthPotion => '!',
        ItemKind::ShortSword => '/',
        ItemKind::LeatherArmor => '[',
    }
}

/// Display color for an item kind.
pub fn item_color(kind: ItemKind) -> GameColor {
    match kind {
        ItemKind::HealthPotion => GameColor::Red,
        ItemKind::ShortSword => GameColor::Cyan,
        ItemKind::LeatherArmor => GameColor::Yellow,
    }
}

/// Human-readable name for an item kind.
pub fn item_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::HealthPotion => "Health Potion",
        ItemKind::ShortSword => "Short Sword",
        ItemKind::LeatherArmor => "Leather Armor",
    }
}

/// Spawn weight for the weighted item spawn table.
/// Higher weight = more common. Set to 0 to disable spawning.
pub fn item_spawn_weight(kind: ItemKind) -> u32 {
    match kind {
        ItemKind::HealthPotion => balance::HEALTH_POTION_SPAWN_WEIGHT as u32,
        ItemKind::ShortSword => balance::SHORT_SWORD_SPAWN_WEIGHT as u32,
        ItemKind::LeatherArmor => balance::LEATHER_ARMOR_SPAWN_WEIGHT as u32,
    }
}

/// HP restored when a health potion is consumed. Returns 0 for non-consumables.
pub fn item_heal_amount(kind: ItemKind) -> Stat {
    match kind {
        ItemKind::HealthPotion => balance::HEALTH_POTION_HEAL as Stat,
        _ => 0,
    }
}

/// Attack bonus granted by equipping this item. Returns 0 for non-weapons.
pub fn item_attack_bonus(kind: ItemKind) -> Stat {
    match kind {
        ItemKind::ShortSword => balance::SHORT_SWORD_ATK_BONUS as Stat,
        _ => 0,
    }
}

/// Defense bonus granted by equipping this item. Returns 0 for non-armor.
pub fn item_defense_bonus(kind: ItemKind) -> Stat {
    match kind {
        ItemKind::LeatherArmor => balance::LEATHER_ARMOR_DEF_BONUS as Stat,
        _ => 0,
    }
}

/// Whether this item is a consumable (used immediately on pickup).
pub fn is_consumable(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::HealthPotion)
}

/// Whether this item is a weapon (occupies weapon slot).
pub fn is_weapon(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::ShortSword)
}

/// Whether this item is armor (occupies armor slot).
pub fn is_armor(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::LeatherArmor)
}

/// Returns true if `new` is strictly better than `current` for the weapon slot.
pub fn is_better_weapon(new: ItemKind, current: Option<ItemKind>) -> bool {
    match current {
        None => item_attack_bonus(new) > 0,
        Some(cur) => item_attack_bonus(new) > item_attack_bonus(cur),
    }
}

/// Returns true if `new` is strictly better than `current` for the armor slot.
pub fn is_better_armor(new: ItemKind, current: Option<ItemKind>) -> bool {
    match current {
        None => item_defense_bonus(new) > 0,
        Some(cur) => item_defense_bonus(new) > item_defense_bonus(cur),
    }
}

/// All item kinds with positive spawn weight, for the spawn table.
pub fn spawn_table() -> Vec<(ItemKind, u32)> {
    ALL_KINDS
        .iter()
        .filter_map(|&kind| {
            let w = item_spawn_weight(kind);
            if w > 0 { Some((kind, w)) } else { None }
        })
        .collect()
}

impl Equipment {
    /// Attack bonus from equipped weapon.
    pub fn attack_bonus(&self) -> Stat {
        self.weapon.map_or(0, item_attack_bonus)
    }

    /// Defense bonus from equipped armor.
    pub fn defense_bonus(&self) -> Stat {
        self.armor.map_or(0, item_defense_bonus)
    }
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
        assert_eq!(table.len(), 3);
        let total: u32 = table.iter().map(|(_, w)| w).sum();
        assert_eq!(total, 100); // 50 + 30 + 20
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
            armor: None,
        };
        assert_eq!(eq.attack_bonus(), 3);
        assert_eq!(eq.defense_bonus(), 0);
    }

    #[test]
    fn equipment_armor_bonus() {
        let eq = Equipment {
            weapon: None,
            armor: Some(ItemKind::LeatherArmor),
        };
        assert_eq!(eq.attack_bonus(), 0);
        assert_eq!(eq.defense_bonus(), 2);
    }

    #[test]
    fn equipment_both_slots() {
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            armor: Some(ItemKind::LeatherArmor),
        };
        assert_eq!(eq.attack_bonus(), 3);
        assert_eq!(eq.defense_bonus(), 2);
    }
}
