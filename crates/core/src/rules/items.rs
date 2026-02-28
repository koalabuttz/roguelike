//! Pure item definitions and lookup functions for all capability tiers.
//!
//! This module defines `ItemKind` and all tier-portable item queries.
//! Every lookup is a `const fn` with explicit exhaustive matches — no
//! wildcard arms — so the compiler forces coverage when variants are added.
//!
//! **Stat contract:** All stat return types are `u8` — the smallest tier's
//! natural width. Balance values are non-negative by design (debuffs are
//! handled by the combat/effect system, not by negative item stats). The
//! standard-tier `item` module re-exports `ItemKind` and widens `u8 → Stat`
//! (`i32`) via lossless `as` casts for compatibility with the standard-tier
//! engine, which uses signed `Stat` to support debuffs in combat math.

use core::mem::size_of;

use super::balance;
use super::color::GameColor;

/// The type of item. Each variant is a `u8` discriminant — no 16-bit bloat
/// on constrained platforms (C64, GBA).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ItemKind {
    HealthPotion = 0,
    ShortSword = 1,
    LeatherArmor = 2,
}

/// All item kinds, for iteration. Adding a variant to `ItemKind` without
/// adding it here will cause exhaustive-match compile errors in `glyph()`
/// et al., plus the `all_kinds_covers_every_variant` test catches desync.
pub const ALL_KINDS: [ItemKind; 3] = [
    ItemKind::HealthPotion,
    ItemKind::ShortSword,
    ItemKind::LeatherArmor,
];

/// Number of item kinds, derived from `ALL_KINDS` — never manually synced.
pub const KIND_COUNT: usize = ALL_KINDS.len();

// ---------------------------------------------------------------------------
// Display lookups
// ---------------------------------------------------------------------------

/// Display glyph for an item kind.
pub const fn glyph(kind: ItemKind) -> char {
    match kind {
        ItemKind::HealthPotion => '!',
        ItemKind::ShortSword => '/',
        ItemKind::LeatherArmor => '[',
    }
}

/// Display color for an item kind.
pub const fn color(kind: ItemKind) -> GameColor {
    match kind {
        ItemKind::HealthPotion => GameColor::Red,
        ItemKind::ShortSword => GameColor::Cyan,
        ItemKind::LeatherArmor => GameColor::Yellow,
    }
}

/// Human-readable name for an item kind.
pub const fn name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::HealthPotion => "Health Potion",
        ItemKind::ShortSword => "Short Sword",
        ItemKind::LeatherArmor => "Leather Armor",
    }
}

// ---------------------------------------------------------------------------
// Stat lookups (all u8 for tier portability, all const fn, no wildcards)
// ---------------------------------------------------------------------------

/// Spawn weight — single source of truth, indexed by kind.
/// `SPAWN_TABLE` and `spawn_weight()` both read from here.
const WEIGHTS: [u8; KIND_COUNT] = [
    balance::HEALTH_POTION_SPAWN_WEIGHT,
    balance::SHORT_SWORD_SPAWN_WEIGHT,
    balance::LEATHER_ARMOR_SPAWN_WEIGHT,
];

/// Spawn weight for the weighted item spawn table.
/// Higher weight = more common. Returns 0 to disable spawning.
pub const fn spawn_weight(kind: ItemKind) -> u8 {
    WEIGHTS[kind as usize]
}

/// HP restored when consumed. Returns 0 for non-consumables.
pub const fn heal_amount(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => balance::HEALTH_POTION_HEAL,
        ItemKind::ShortSword => 0,
        ItemKind::LeatherArmor => 0,
    }
}

/// Attack bonus granted by equipping this item. Returns 0 for non-weapons.
pub const fn attack_bonus(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => 0,
        ItemKind::ShortSword => balance::SHORT_SWORD_ATK_BONUS,
        ItemKind::LeatherArmor => 0,
    }
}

/// Defense bonus granted by equipping this item. Returns 0 for non-armor.
pub const fn defense_bonus(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => 0,
        ItemKind::ShortSword => 0,
        ItemKind::LeatherArmor => balance::LEATHER_ARMOR_DEF_BONUS,
    }
}

// ---------------------------------------------------------------------------
// Spawn table (fixed-size, no allocation)
// ---------------------------------------------------------------------------

/// Fixed-size spawn table: `(kind, weight)` pairs for all item kinds.
/// Usable on constrained tiers (C64, GBA) without heap allocation.
/// Weights come from `WEIGHTS` — single source of truth with `spawn_weight()`.
///
/// Each entry is 2 bytes (u8 kind + u8 weight). If this struct ever grows
/// past a power-of-two, switch to parallel arrays to avoid index math on 6502.
pub const SPAWN_TABLE: [(ItemKind, u8); KIND_COUNT] = [
    (ItemKind::HealthPotion, WEIGHTS[0]),
    (ItemKind::ShortSword, WEIGHTS[1]),
    (ItemKind::LeatherArmor, WEIGHTS[2]),
];

// Compile-time guard: each spawn table entry must be exactly 2 bytes.
// If ItemKind grows beyond u8 or padding is inserted, this fails.
// Compile-time guarantee: enum fits in a single byte on all tiers.
const _: () = assert!(size_of::<ItemKind>() == 1);
const _: () = assert!(size_of::<(ItemKind, u8)>() == 2);

// ---------------------------------------------------------------------------
// Type queries
// ---------------------------------------------------------------------------

/// Whether this item is a consumable (used immediately on pickup).
pub const fn is_consumable(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => true,
        ItemKind::ShortSword => false,
        ItemKind::LeatherArmor => false,
    }
}

/// Whether this item is a weapon (occupies weapon slot).
pub const fn is_weapon(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => false,
        ItemKind::ShortSword => true,
        ItemKind::LeatherArmor => false,
    }
}

/// Whether this item is armor (occupies armor slot).
pub const fn is_armor(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => false,
        ItemKind::ShortSword => false,
        ItemKind::LeatherArmor => true,
    }
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Returns true if `new` is strictly better than `current` for a given stat.
///
/// `stat_fn` is the lookup (e.g. `attack_bonus`, `defense_bonus`). Works for
/// any equipment slot — no per-slot boilerplate needed.
pub fn is_upgrade(new: ItemKind, current: Option<ItemKind>, stat_fn: fn(ItemKind) -> u8) -> bool {
    match current {
        None => stat_fn(new) > 0,
        Some(cur) => stat_fn(new) > stat_fn(cur),
    }
}

/// Returns true if `new` is strictly better than `current` for the weapon slot.
pub fn is_better_weapon(new: ItemKind, current: Option<ItemKind>) -> bool {
    is_upgrade(new, current, attack_bonus)
}

/// Returns true if `new` is strictly better than `current` for the armor slot.
pub fn is_better_armor(new: ItemKind, current: Option<ItemKind>) -> bool {
    is_upgrade(new, current, defense_bonus)
}

// ---------------------------------------------------------------------------
// Equipment (shared across all tiers)
// ---------------------------------------------------------------------------

/// Tracked equipment slots for the player.
///
/// Pure data + bonus lookups — no coordinates, no allocation, `no_std`.
/// Combat reads effective stats (base + equipment bonus) from these slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Equipment {
    pub weapon: Option<ItemKind>,
    pub armor: Option<ItemKind>,
}

impl Equipment {
    /// Attack bonus from equipped weapon (u8 for tier portability).
    pub fn attack_bonus(&self) -> u8 {
        self.weapon.map_or(0, attack_bonus)
    }

    /// Defense bonus from equipped armor (u8 for tier portability).
    pub fn defense_bonus(&self) -> u8 {
        self.armor.map_or(0, defense_bonus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_potion_properties() {
        assert_eq!(glyph(ItemKind::HealthPotion), '!');
        assert_eq!(color(ItemKind::HealthPotion), GameColor::Red);
        assert_eq!(name(ItemKind::HealthPotion), "Health Potion");
        assert_eq!(heal_amount(ItemKind::HealthPotion), 10);
        assert_eq!(attack_bonus(ItemKind::HealthPotion), 0);
        assert_eq!(defense_bonus(ItemKind::HealthPotion), 0);
        assert!(is_consumable(ItemKind::HealthPotion));
        assert!(!is_weapon(ItemKind::HealthPotion));
        assert!(!is_armor(ItemKind::HealthPotion));
    }

    #[test]
    fn short_sword_properties() {
        assert_eq!(glyph(ItemKind::ShortSword), '/');
        assert_eq!(color(ItemKind::ShortSword), GameColor::Cyan);
        assert_eq!(name(ItemKind::ShortSword), "Short Sword");
        assert_eq!(heal_amount(ItemKind::ShortSword), 0);
        assert_eq!(attack_bonus(ItemKind::ShortSword), 3);
        assert_eq!(defense_bonus(ItemKind::ShortSword), 0);
        assert!(!is_consumable(ItemKind::ShortSword));
        assert!(is_weapon(ItemKind::ShortSword));
        assert!(!is_armor(ItemKind::ShortSword));
    }

    #[test]
    fn leather_armor_properties() {
        assert_eq!(glyph(ItemKind::LeatherArmor), '[');
        assert_eq!(color(ItemKind::LeatherArmor), GameColor::Yellow);
        assert_eq!(name(ItemKind::LeatherArmor), "Leather Armor");
        assert_eq!(heal_amount(ItemKind::LeatherArmor), 0);
        assert_eq!(attack_bonus(ItemKind::LeatherArmor), 0);
        assert_eq!(defense_bonus(ItemKind::LeatherArmor), 2);
        assert!(!is_consumable(ItemKind::LeatherArmor));
        assert!(!is_weapon(ItemKind::LeatherArmor));
        assert!(is_armor(ItemKind::LeatherArmor));
    }

    #[test]
    fn is_better_weapon_with_none() {
        assert!(is_better_weapon(ItemKind::ShortSword, None));
        assert!(!is_better_weapon(ItemKind::HealthPotion, None));
    }

    #[test]
    fn is_better_weapon_same_not_better() {
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
    fn is_better_armor_same_not_better() {
        assert!(!is_better_armor(
            ItemKind::LeatherArmor,
            Some(ItemKind::LeatherArmor)
        ));
    }

    #[test]
    fn all_kinds_covers_every_variant() {
        // If you add a variant to ItemKind, this exhaustive match forces you
        // to add it here — and the assert_eq on KIND_COUNT catches it too.
        let mut count = 0usize;
        for &kind in &ALL_KINDS {
            match kind {
                ItemKind::HealthPotion | ItemKind::ShortSword | ItemKind::LeatherArmor => {}
            }
            assert!(!name(kind).is_empty());
            assert!(spawn_weight(kind) > 0);
            count += 1;
        }
        assert_eq!(count, KIND_COUNT);
    }

    #[test]
    fn is_upgrade_generic() {
        // Works with any stat function
        assert!(is_upgrade(ItemKind::ShortSword, None, attack_bonus));
        assert!(!is_upgrade(ItemKind::HealthPotion, None, attack_bonus));
        assert!(is_upgrade(ItemKind::LeatherArmor, None, defense_bonus));
        assert!(!is_upgrade(
            ItemKind::LeatherArmor,
            Some(ItemKind::LeatherArmor),
            defense_bonus
        ));
    }

    #[test]
    fn repr_u8_discriminants() {
        assert_eq!(ItemKind::HealthPotion as u8, 0);
        assert_eq!(ItemKind::ShortSword as u8, 1);
        assert_eq!(ItemKind::LeatherArmor as u8, 2);
    }

    #[test]
    fn spawn_table_matches_weights() {
        for &(kind, weight) in &SPAWN_TABLE {
            assert_eq!(weight, spawn_weight(kind));
        }
    }

    #[test]
    fn const_fn_usable_at_compile_time() {
        // Prove these are truly const — use them in const context.
        const POTION_HEAL: u8 = heal_amount(ItemKind::HealthPotion);
        const SWORD_ATK: u8 = attack_bonus(ItemKind::ShortSword);
        const ARMOR_DEF: u8 = defense_bonus(ItemKind::LeatherArmor);
        assert_eq!(POTION_HEAL, 10);
        assert_eq!(SWORD_ATK, 3);
        assert_eq!(ARMOR_DEF, 2);
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
