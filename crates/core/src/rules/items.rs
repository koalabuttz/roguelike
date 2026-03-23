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
use super::properties::{self, PropertyBag};

/// The type of item. Each variant is a `u8` discriminant — no 16-bit bloat
/// on constrained platforms (C64, GBA).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ItemKind {
    HealthPotion = 0,
    ShortSword = 1,
    LeatherArmor = 2,
    IronMace = 3,
    LongSword = 4,
    ChainMail = 5,
    GreaterHealthPotion = 6,
    StrengthPotion = 7,
}

/// All item kinds, for iteration. Adding a variant to `ItemKind` without
/// adding it here will cause exhaustive-match compile errors in `glyph()`
/// et al., plus the `all_kinds_covers_every_variant` test catches desync.
pub const ALL_KINDS: [ItemKind; 8] = [
    ItemKind::HealthPotion,
    ItemKind::ShortSword,
    ItemKind::LeatherArmor,
    ItemKind::IronMace,
    ItemKind::LongSword,
    ItemKind::ChainMail,
    ItemKind::GreaterHealthPotion,
    ItemKind::StrengthPotion,
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
        ItemKind::IronMace => '/',
        ItemKind::LongSword => '/',
        ItemKind::ChainMail => '[',
        ItemKind::GreaterHealthPotion => '!',
        ItemKind::StrengthPotion => '!',
    }
}

/// Display color for an item kind.
pub const fn color(kind: ItemKind) -> GameColor {
    match kind {
        ItemKind::HealthPotion => GameColor::Red,
        ItemKind::ShortSword => GameColor::Cyan,
        ItemKind::LeatherArmor => GameColor::Yellow,
        ItemKind::IronMace => GameColor::White,
        ItemKind::LongSword => GameColor::White,
        ItemKind::ChainMail => GameColor::Grey,
        ItemKind::GreaterHealthPotion => GameColor::DarkRed,
        ItemKind::StrengthPotion => GameColor::Green,
    }
}

/// Human-readable name for an item kind.
pub const fn name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::HealthPotion => "Health Potion",
        ItemKind::ShortSword => "Short Sword",
        ItemKind::LeatherArmor => "Leather Armor",
        ItemKind::IronMace => "Iron Mace",
        ItemKind::LongSword => "Long Sword",
        ItemKind::ChainMail => "Chain Mail",
        ItemKind::GreaterHealthPotion => "Greater Health Potion",
        ItemKind::StrengthPotion => "Potion of Strength",
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
    balance::IRON_MACE_SPAWN_WEIGHT,
    balance::LONG_SWORD_SPAWN_WEIGHT,
    balance::CHAIN_MAIL_SPAWN_WEIGHT,
    balance::GREATER_HEALTH_POTION_SPAWN_WEIGHT,
    balance::STRENGTH_POTION_SPAWN_WEIGHT,
];

/// Spawn weight for the weighted item spawn table.
/// Higher weight = more common. Returns 0 to disable spawning.
pub const fn spawn_weight(kind: ItemKind) -> u8 {
    WEIGHTS[kind as usize]
}

/// Minimum dungeon depth at which this item can spawn.
pub const fn min_depth(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => balance::HEALTH_POTION_MIN_DEPTH,
        ItemKind::ShortSword => balance::SHORT_SWORD_MIN_DEPTH,
        ItemKind::LeatherArmor => balance::LEATHER_ARMOR_MIN_DEPTH,
        ItemKind::IronMace => balance::IRON_MACE_MIN_DEPTH,
        ItemKind::LongSword => balance::LONG_SWORD_MIN_DEPTH,
        ItemKind::ChainMail => balance::CHAIN_MAIL_MIN_DEPTH,
        ItemKind::GreaterHealthPotion => balance::GREATER_HEALTH_POTION_MIN_DEPTH,
        ItemKind::StrengthPotion => balance::STRENGTH_POTION_MIN_DEPTH,
    }
}

/// HP restored when consumed. Returns 0 for non-consumables.
pub const fn heal_amount(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => balance::HEALTH_POTION_HEAL,
        ItemKind::ShortSword => 0,
        ItemKind::LeatherArmor => 0,
        ItemKind::IronMace => 0,
        ItemKind::LongSword => 0,
        ItemKind::ChainMail => 0,
        ItemKind::GreaterHealthPotion => balance::GREATER_HEALTH_POTION_HEAL,
        ItemKind::StrengthPotion => 0,
    }
}

/// Attack bonus granted by equipping this item. Returns 0 for non-weapons.
pub const fn attack_bonus(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => 0,
        ItemKind::ShortSword => balance::SHORT_SWORD_ATK_BONUS,
        ItemKind::LeatherArmor => 0,
        ItemKind::IronMace => balance::IRON_MACE_ATK_BONUS,
        ItemKind::LongSword => balance::LONG_SWORD_ATK_BONUS,
        ItemKind::ChainMail => 0,
        ItemKind::GreaterHealthPotion => 0,
        ItemKind::StrengthPotion => 0,
    }
}

/// Defense bonus granted by equipping this item. Returns 0 for non-armor.
pub const fn defense_bonus(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::HealthPotion => 0,
        ItemKind::ShortSword => 0,
        ItemKind::LeatherArmor => balance::LEATHER_ARMOR_DEF_BONUS,
        ItemKind::IronMace => 0,
        ItemKind::LongSword => 0,
        ItemKind::ChainMail => balance::CHAIN_MAIL_DEF_BONUS,
        ItemKind::GreaterHealthPotion => 0,
        ItemKind::StrengthPotion => 0,
    }
}

/// Permanent ATK boost granted when consumed. Returns 0 for non-boosting items.
pub const fn strength_boost(kind: ItemKind) -> u8 {
    match kind {
        ItemKind::StrengthPotion => balance::STRENGTH_POTION_ATK_BOOST,
        ItemKind::HealthPotion => 0,
        ItemKind::ShortSword => 0,
        ItemKind::LeatherArmor => 0,
        ItemKind::IronMace => 0,
        ItemKind::LongSword => 0,
        ItemKind::ChainMail => 0,
        ItemKind::GreaterHealthPotion => 0,
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
    (ItemKind::IronMace, WEIGHTS[3]),
    (ItemKind::LongSword, WEIGHTS[4]),
    (ItemKind::ChainMail, WEIGHTS[5]),
    (ItemKind::GreaterHealthPotion, WEIGHTS[6]),
    (ItemKind::StrengthPotion, WEIGHTS[7]),
];

// Compile-time guard: each spawn table entry must be exactly 2 bytes.
// If ItemKind grows beyond u8 or padding is inserted, this fails.
// Compile-time guarantee: enum fits in a single byte on all tiers.
const _: () = assert!(size_of::<ItemKind>() == 1);
const _: () = assert!(size_of::<(ItemKind, u8)>() == 2);
// InvSlot: 1-byte kind + 1-byte count = 2 bytes.
const _: () = assert!(size_of::<InvSlot>() == 2);

// ---------------------------------------------------------------------------
// Type queries
// ---------------------------------------------------------------------------

/// Whether this item is a consumable (used from inventory).
pub const fn is_consumable(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => true,
        ItemKind::ShortSword => false,
        ItemKind::LeatherArmor => false,
        ItemKind::IronMace => false,
        ItemKind::LongSword => false,
        ItemKind::ChainMail => false,
        ItemKind::GreaterHealthPotion => true,
        ItemKind::StrengthPotion => true,
    }
}

/// Whether this item is a weapon (occupies weapon slot).
pub const fn is_weapon(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => false,
        ItemKind::ShortSword => true,
        ItemKind::LeatherArmor => false,
        ItemKind::IronMace => true,
        ItemKind::LongSword => true,
        ItemKind::ChainMail => false,
        ItemKind::GreaterHealthPotion => false,
        ItemKind::StrengthPotion => false,
    }
}

/// Whether this item is armor (occupies armor slot).
pub const fn is_armor(kind: ItemKind) -> bool {
    match kind {
        ItemKind::HealthPotion => false,
        ItemKind::ShortSword => false,
        ItemKind::LeatherArmor => true,
        ItemKind::IronMace => false,
        ItemKind::LongSword => false,
        ItemKind::ChainMail => true,
        ItemKind::GreaterHealthPotion => false,
        ItemKind::StrengthPotion => false,
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
// Property profiles (dead code in step 1 — nothing reads these at runtime)
// ---------------------------------------------------------------------------

/// Default property bag for an item kind. This is the starting state for
/// the emergent property system. Properties are nibble-packed (16 × 4-bit
/// values in 8 bytes).
///
/// **Step 1:** These profiles exist as validated dead code. Combat still
/// reads from `attack_bonus(kind)` etc. The profiles define what each item
/// IS in property space, forcing the design decisions before the interaction
/// engine exists.
pub const fn default_properties(kind: ItemKind) -> PropertyBag {
    let mut bag = properties::EMPTY;
    match kind {
        ItemKind::HealthPotion => {
            // WET:5, MAG:4 — magical healing liquid
            bag[3] = 0x50; // WET:5 (high nibble of byte 3)
            bag[5] = 0x40; // MAG:4 (high nibble of byte 5)
        }
        ItemKind::ShortSword => {
            // SHP:6, HRD:7, HVY:4, MTL:8
            bag[0] = 0x67; // SHP:6 | HRD:7
            bag[1] = 0x40; // HVY:4 | SWF:0
            bag[3] = 0x08; // WET:0 | MTL:8
        }
        ItemKind::LeatherArmor => {
            // HRD:5, SWF:3, ORG:6
            bag[0] = 0x05; // SHP:0 | HRD:5
            bag[1] = 0x03; // HVY:0 | SWF:3
            bag[4] = 0x60; // ORG:6 | VNM:0
        }
        ItemKind::IronMace => {
            // HRD:9, HVY:8, MTL:8 — blunt weapon, no SHARP
            bag[0] = 0x09; // SHP:0 | HRD:9
            bag[1] = 0x80; // HVY:8 | SWF:0
            bag[3] = 0x08; // WET:0 | MTL:8
        }
        ItemKind::LongSword => {
            // SHP:9, HRD:8, HVY:7, MTL:9
            bag[0] = 0x98; // SHP:9 | HRD:8
            bag[1] = 0x70; // HVY:7 | SWF:0
            bag[3] = 0x09; // WET:0 | MTL:9
        }
        ItemKind::ChainMail => {
            // HRD:8, HVY:6, MTL:7
            bag[0] = 0x08; // SHP:0 | HRD:8
            bag[1] = 0x60; // HVY:6 | SWF:0
            bag[3] = 0x07; // WET:0 | MTL:7
        }
        ItemKind::GreaterHealthPotion => {
            // WET:7, MAG:7 — stronger magical healing
            bag[3] = 0x70; // WET:7 (high nibble of byte 3)
            bag[5] = 0x70; // MAG:7 (high nibble of byte 5)
        }
        ItemKind::StrengthPotion => {
            // HOT:4, HVY:3, MAG:6 — burning power concentrate
            bag[1] = 0x30; // HVY:3 | SWF:0
            bag[2] = 0x40; // HOT:4 | CLD:0
            bag[5] = 0x60; // MAG:6 | VOL:0
        }
    }
    bag
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

// ---------------------------------------------------------------------------
// Inventory (shared across all tiers)
// ---------------------------------------------------------------------------

/// Maximum inventory slots (a-z). Shared across all tiers.
pub const MAX_INVENTORY: usize = 26;

/// A single inventory slot — one item kind with a count (stacks for consumables).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvSlot {
    pub kind: ItemKind,
    pub count: u8,
}

/// Brogue-style 26-slot inventory (a-z). Consumables stack, equipment doesn't.
///
/// 26 × `Option<InvSlot>` = 78 bytes — fits comfortably in micro tier hiram.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Inventory {
    slots: [Option<InvSlot>; MAX_INVENTORY],
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    /// Create an empty inventory.
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_INVENTORY],
        }
    }

    /// Add an item. Consumables stack in existing slots; equipment takes
    /// a new slot. Returns `false` if inventory is full.
    pub fn add(&mut self, kind: ItemKind) -> bool {
        // Try to stack consumables in an existing slot.
        if is_consumable(kind) {
            for slot in self.slots.iter_mut().flatten() {
                if slot.kind == kind {
                    slot.count = slot.count.saturating_add(1);
                    return true;
                }
            }
        }

        // Find first empty slot.
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(InvSlot { kind, count: 1 });
                return true;
            }
        }

        false // full
    }

    /// Remove one item from a slot. Returns the kind removed, or `None` if
    /// the slot is empty or out of range.
    pub fn remove_one(&mut self, slot: usize) -> Option<ItemKind> {
        if slot >= MAX_INVENTORY {
            return None;
        }
        let entry = self.slots[slot].as_mut()?;
        let kind = entry.kind;
        if entry.count <= 1 {
            self.slots[slot] = None;
        } else {
            entry.count -= 1;
        }
        Some(kind)
    }

    /// Read a slot by index.
    pub fn get(&self, slot: usize) -> Option<&InvSlot> {
        if slot >= MAX_INVENTORY {
            return None;
        }
        self.slots[slot].as_ref()
    }

    /// Number of occupied slots.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Whether the inventory is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether all 26 slots are occupied.
    pub fn is_full(&self) -> bool {
        self.slots.iter().all(|s| s.is_some())
    }

    /// Iterate over occupied slots as `(index, &InvSlot)`.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &InvSlot)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|slot| (i, slot)))
    }

    /// Set a slot directly by index. Used by save/load to restore
    /// inventory state without going through the stacking logic of `add()`.
    pub(crate) fn set_slot(&mut self, idx: usize, slot: Option<InvSlot>) {
        if idx < MAX_INVENTORY {
            self.slots[idx] = slot;
        }
    }

    /// Get the n-th occupied slot (0-indexed among occupied slots).
    /// Avoids a 473-byte `FilterMap::nth` monomorphization on 6502.
    pub fn nth_occupied(&self, n: usize) -> Option<(usize, &InvSlot)> {
        let mut count = 0usize;
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(s) = slot {
                if count == n {
                    return Some((i, s));
                }
                count += 1;
            }
        }
        None
    }
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
                ItemKind::HealthPotion
                | ItemKind::ShortSword
                | ItemKind::LeatherArmor
                | ItemKind::IronMace
                | ItemKind::LongSword
                | ItemKind::ChainMail
                | ItemKind::GreaterHealthPotion
                | ItemKind::StrengthPotion => {}
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
        assert_eq!(ItemKind::IronMace as u8, 3);
        assert_eq!(ItemKind::LongSword as u8, 4);
        assert_eq!(ItemKind::ChainMail as u8, 5);
        assert_eq!(ItemKind::GreaterHealthPotion as u8, 6);
        assert_eq!(ItemKind::StrengthPotion as u8, 7);
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

    // ── Inventory tests ─────────────────────────────────────────────

    #[test]
    fn inventory_add_to_empty() {
        let mut inv = Inventory::new();
        assert!(inv.add(ItemKind::HealthPotion));
        assert_eq!(inv.get(0).unwrap().kind, ItemKind::HealthPotion);
        assert_eq!(inv.get(0).unwrap().count, 1);
    }

    #[test]
    fn inventory_consumable_stacks() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::HealthPotion);
        inv.add(ItemKind::HealthPotion);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv.get(0).unwrap().count, 2);
    }

    #[test]
    fn inventory_equipment_no_stack() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::ShortSword);
        inv.add(ItemKind::ShortSword);
        assert_eq!(inv.len(), 2);
        assert_eq!(inv.get(0).unwrap().count, 1);
        assert_eq!(inv.get(1).unwrap().count, 1);
    }

    #[test]
    fn inventory_remove_one_decrements() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::HealthPotion);
        inv.add(ItemKind::HealthPotion);
        let removed = inv.remove_one(0);
        assert_eq!(removed, Some(ItemKind::HealthPotion));
        assert_eq!(inv.get(0).unwrap().count, 1);
    }

    #[test]
    fn inventory_remove_last_clears_slot() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::ShortSword);
        let removed = inv.remove_one(0);
        assert_eq!(removed, Some(ItemKind::ShortSword));
        assert!(inv.get(0).is_none());
        assert_eq!(inv.len(), 0);
    }

    #[test]
    fn inventory_full_returns_false() {
        let mut inv = Inventory::new();
        for _ in 0..MAX_INVENTORY {
            assert!(inv.add(ItemKind::ShortSword));
        }
        assert!(inv.is_full());
        assert!(!inv.add(ItemKind::ShortSword));
    }

    #[test]
    fn inventory_full_consumable_still_stacks() {
        let mut inv = Inventory::new();
        // Fill slot 0 with a potion, rest with swords.
        inv.add(ItemKind::HealthPotion);
        for _ in 1..MAX_INVENTORY {
            inv.add(ItemKind::ShortSword);
        }
        assert!(inv.is_full());
        // Adding another potion should stack into slot 0.
        assert!(inv.add(ItemKind::HealthPotion));
        assert_eq!(inv.get(0).unwrap().count, 2);
    }

    #[test]
    fn inventory_len_counts_occupied() {
        let mut inv = Inventory::new();
        assert_eq!(inv.len(), 0);
        assert!(inv.is_empty());
        inv.add(ItemKind::HealthPotion);
        assert_eq!(inv.len(), 1);
        inv.add(ItemKind::ShortSword);
        assert_eq!(inv.len(), 2);
    }

    #[test]
    fn inventory_iter_yields_occupied() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::HealthPotion);
        inv.add(ItemKind::ShortSword);
        let items: Vec<_> = inv.iter().collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, 0); // slot index
        assert_eq!(items[0].1.kind, ItemKind::HealthPotion);
        assert_eq!(items[1].0, 1);
        assert_eq!(items[1].1.kind, ItemKind::ShortSword);
    }

    #[test]
    fn inventory_remove_out_of_range() {
        let mut inv = Inventory::new();
        assert_eq!(inv.remove_one(30), None);
    }

    #[test]
    fn inventory_remove_empty_slot() {
        let mut inv = Inventory::new();
        assert_eq!(inv.remove_one(0), None);
    }

    // ── New item tests ────────────────────────────────────────────────

    #[test]
    fn iron_mace_properties() {
        assert_eq!(name(ItemKind::IronMace), "Iron Mace");
        assert_eq!(attack_bonus(ItemKind::IronMace), 4);
        assert_eq!(defense_bonus(ItemKind::IronMace), 0);
        assert_eq!(heal_amount(ItemKind::IronMace), 0);
        assert!(is_weapon(ItemKind::IronMace));
        assert!(!is_consumable(ItemKind::IronMace));
        assert!(!is_armor(ItemKind::IronMace));
        assert_eq!(min_depth(ItemKind::IronMace), 2);
    }

    #[test]
    fn long_sword_properties() {
        assert_eq!(name(ItemKind::LongSword), "Long Sword");
        assert_eq!(attack_bonus(ItemKind::LongSword), 5);
        assert!(is_weapon(ItemKind::LongSword));
        assert_eq!(min_depth(ItemKind::LongSword), 3);
    }

    #[test]
    fn chain_mail_properties() {
        assert_eq!(name(ItemKind::ChainMail), "Chain Mail");
        assert_eq!(defense_bonus(ItemKind::ChainMail), 4);
        assert!(is_armor(ItemKind::ChainMail));
        assert_eq!(min_depth(ItemKind::ChainMail), 3);
    }

    #[test]
    fn greater_health_potion_properties() {
        assert_eq!(name(ItemKind::GreaterHealthPotion), "Greater Health Potion");
        assert_eq!(heal_amount(ItemKind::GreaterHealthPotion), 20);
        assert!(is_consumable(ItemKind::GreaterHealthPotion));
        assert_eq!(min_depth(ItemKind::GreaterHealthPotion), 3);
    }

    #[test]
    fn strength_potion_properties() {
        assert_eq!(name(ItemKind::StrengthPotion), "Potion of Strength");
        assert_eq!(strength_boost(ItemKind::StrengthPotion), 1);
        assert_eq!(heal_amount(ItemKind::StrengthPotion), 0);
        assert!(is_consumable(ItemKind::StrengthPotion));
        assert_eq!(min_depth(ItemKind::StrengthPotion), 2);
    }

    #[test]
    fn weapon_upgrade_chain() {
        // Short Sword < Iron Mace < Long Sword
        assert!(is_better_weapon(
            ItemKind::IronMace,
            Some(ItemKind::ShortSword)
        ));
        assert!(is_better_weapon(
            ItemKind::LongSword,
            Some(ItemKind::IronMace)
        ));
        assert!(!is_better_weapon(
            ItemKind::ShortSword,
            Some(ItemKind::IronMace)
        ));
    }

    #[test]
    fn armor_upgrade_chain() {
        // Leather Armor < Chain Mail
        assert!(is_better_armor(
            ItemKind::ChainMail,
            Some(ItemKind::LeatherArmor)
        ));
        assert!(!is_better_armor(
            ItemKind::LeatherArmor,
            Some(ItemKind::ChainMail)
        ));
    }

    #[test]
    fn equipment_long_sword_bonus() {
        let eq = Equipment {
            weapon: Some(ItemKind::LongSword),
            armor: None,
        };
        assert_eq!(eq.attack_bonus(), 5);
    }

    #[test]
    fn equipment_chain_mail_bonus() {
        let eq = Equipment {
            weapon: None,
            armor: Some(ItemKind::ChainMail),
        };
        assert_eq!(eq.defense_bonus(), 4);
    }

    // ── Property profile tests ────────────────────────────────────────

    #[test]
    fn all_items_have_at_least_two_properties() {
        for &kind in &ALL_KINDS {
            let bag = default_properties(kind);
            assert!(
                properties::count_nonzero(&bag) >= 2,
                "{:?} has fewer than 2 non-zero properties",
                kind
            );
        }
    }

    #[test]
    fn weapons_have_sharp_or_hard() {
        use properties::Property;
        for &kind in &ALL_KINDS {
            if is_weapon(kind) {
                let bag = default_properties(kind);
                let sharp = properties::get(&bag, Property::Sharp);
                let hard = properties::get(&bag, Property::Hard);
                assert!(
                    sharp > 0 || hard > 0,
                    "weapon {:?} has neither SHARP nor HARD",
                    kind
                );
            }
        }
    }

    #[test]
    fn armors_have_hard() {
        use properties::Property;
        for &kind in &ALL_KINDS {
            if is_armor(kind) {
                let bag = default_properties(kind);
                assert!(
                    properties::get(&bag, Property::Hard) > 0,
                    "armor {:?} has no HARD",
                    kind
                );
            }
        }
    }

    #[test]
    fn chain_mail_and_leather_differ_on_multiple_axes() {
        use properties::Property;
        let chain = default_properties(ItemKind::ChainMail);
        let leather = default_properties(ItemKind::LeatherArmor);
        // Chain Mail has METAL, Leather does not
        assert!(properties::get(&chain, Property::Metal) > 0);
        assert_eq!(properties::get(&leather, Property::Metal), 0);
        // Leather has ORGANIC, Chain Mail does not
        assert!(properties::get(&leather, Property::Organic) > 0);
        assert_eq!(properties::get(&chain, Property::Organic), 0);
        // Leather has SWIFT, Chain Mail does not
        assert!(properties::get(&leather, Property::Swift) > 0);
        assert_eq!(properties::get(&chain, Property::Swift), 0);
    }

    #[test]
    fn iron_mace_has_no_sharp() {
        use properties::Property;
        let bag = default_properties(ItemKind::IronMace);
        assert_eq!(properties::get(&bag, Property::Sharp), 0);
        assert!(properties::get(&bag, Property::Hard) > 0);
        assert!(properties::get(&bag, Property::Heavy) > 0);
    }

    #[test]
    fn default_properties_deterministic() {
        for &kind in &ALL_KINDS {
            assert_eq!(default_properties(kind), default_properties(kind));
        }
    }

    #[test]
    fn floor_1_items_have_min_depth_1() {
        assert_eq!(min_depth(ItemKind::HealthPotion), 1);
        assert_eq!(min_depth(ItemKind::ShortSword), 1);
        assert_eq!(min_depth(ItemKind::LeatherArmor), 1);
    }
}
