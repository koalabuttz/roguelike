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

use super::color::GameColor;
use super::content::{self, ItemCategory};
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
    content::item_glyph!(kind)
}

/// Display color for an item kind.
pub const fn color(kind: ItemKind) -> GameColor {
    content::item_color!(kind)
}

/// Human-readable name for an item kind.
pub const fn name(kind: ItemKind) -> &'static str {
    content::item_name!(kind)
}

// ---------------------------------------------------------------------------
// Stat lookups (all u8 for tier portability, all const fn, no wildcards)
// ---------------------------------------------------------------------------

/// Spawn weight — single source of truth, indexed by kind.
/// `SPAWN_TABLE` and `spawn_weight()` both read from here.
const WEIGHTS: [u8; KIND_COUNT] = [
    content::item_spawn_weight!(ItemKind::HealthPotion),
    content::item_spawn_weight!(ItemKind::ShortSword),
    content::item_spawn_weight!(ItemKind::LeatherArmor),
    content::item_spawn_weight!(ItemKind::IronMace),
    content::item_spawn_weight!(ItemKind::LongSword),
    content::item_spawn_weight!(ItemKind::ChainMail),
    content::item_spawn_weight!(ItemKind::GreaterHealthPotion),
    content::item_spawn_weight!(ItemKind::StrengthPotion),
];

/// Spawn weight for the weighted item spawn table.
/// Higher weight = more common. Returns 0 to disable spawning.
pub const fn spawn_weight(kind: ItemKind) -> u8 {
    WEIGHTS[kind as usize]
}

/// Minimum dungeon depth at which this item can spawn.
pub const fn min_depth(kind: ItemKind) -> u8 {
    content::item_min_depth!(kind)
}

/// HP restored when consumed. Returns 0 for non-consumables.
pub const fn heal_amount(kind: ItemKind) -> u8 {
    content::item_heal_amount!(kind)
}

/// Attack bonus granted by equipping this item. Returns 0 for non-weapons.
pub const fn attack_bonus(kind: ItemKind) -> u8 {
    if is_weapon(kind) {
        attack_from_bag(&content::item_default_properties!(kind))
    } else {
        0
    }
}

/// Defense bonus granted by equipping this item. Returns 0 for non-armor.
pub const fn defense_bonus(kind: ItemKind) -> u8 {
    if is_armor(kind) {
        defense_from_bag(&content::item_default_properties!(kind))
    } else {
        0
    }
}

/// Permanent ATK boost granted when consumed. Returns 0 for non-boosting items.
pub const fn strength_boost(kind: ItemKind) -> u8 {
    content::item_strength_boost!(kind)
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
// InvSlot: 1-byte kind + 1-byte count + 8-byte property bag = 10 bytes.
const _: () = assert!(size_of::<InvSlot>() == 10);

// ---------------------------------------------------------------------------
// Type queries
// ---------------------------------------------------------------------------

/// Whether this item is a consumable (used from inventory).
pub const fn is_consumable(kind: ItemKind) -> bool {
    matches!(content::item_category!(kind), ItemCategory::Consumable)
}

/// Whether this item is a weapon (occupies weapon slot).
pub const fn is_weapon(kind: ItemKind) -> bool {
    matches!(content::item_category!(kind), ItemCategory::Weapon)
}

/// Whether this item is armor (occupies armor slot).
pub const fn is_armor(kind: ItemKind) -> bool {
    matches!(content::item_category!(kind), ItemCategory::Armor)
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

/// Look up an `ItemKind` by snake_case name (e.g., "health_potion", "iron_mace").
/// Case-insensitive. Matches display name with spaces→underscores. `no_std` compatible.
pub const fn from_snake_case(s: &str) -> Option<ItemKind> {
    let input = s.as_bytes();
    let mut ki = 0;
    while ki < KIND_COUNT {
        let kind = ALL_KINDS[ki];
        let display = name(kind).as_bytes();
        if input.len() == display.len() {
            let mut ok = true;
            let mut ci = 0;
            while ci < input.len() {
                let a = if input[ci] >= b'A' && input[ci] <= b'Z' {
                    input[ci] + 32
                } else {
                    input[ci]
                };
                let b = if display[ci] == b' ' {
                    b'_'
                } else if display[ci] >= b'A' && display[ci] <= b'Z' {
                    display[ci] + 32
                } else {
                    display[ci]
                };
                if a != b {
                    ok = false;
                    break;
                }
                ci += 1;
            }
            if ok {
                return Some(kind);
            }
        }
        ki += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Property profiles
// ---------------------------------------------------------------------------

/// Default property bag for an item kind. This is the starting state for
/// the emergent property system. Properties are nibble-packed (16 × 4-bit
/// values in 8 bytes).
///
/// These profiles define what each item IS in property space. Combat reads
/// stats via `attack_from_bag()` / `defense_from_bag()` (Step 3). The
/// interaction engine modifies bags at runtime via `GameCommand::Combine`.
///
/// **Balance invariant:** Equipment (non-consumable) items should NOT have
/// elemental source properties (HOT, COLD, CORROSIVE, CURSED). These
/// properties trigger interaction rules that modify the target item. Since
/// non-consumables aren't consumed on combine, a player could repeatedly
/// combine to max out target properties in ~4 turns (bounded only by the
/// nibble cap of 15). Consumables are naturally rate-limited by scarcity.
/// If adding non-consumable elemental items, consider a per-item cooldown
/// or diminishing returns on repeated combines.
pub const fn default_properties(kind: ItemKind) -> PropertyBag {
    content::item_default_properties!(kind)
}

// ---------------------------------------------------------------------------
// Item death (material property destruction)
// ---------------------------------------------------------------------------

/// Check if an item's structural material has been destroyed by interaction.
///
/// Returns true if METAL reached 0 (for items that start with METAL > 0)
/// or ORGANIC reached 0 (for items that start with ORGANIC > 0). Items
/// without material properties (potions) cannot die.
pub fn is_material_dead(kind: ItemKind, props: &PropertyBag) -> bool {
    let default = default_properties(kind);
    let had_metal = properties::get(&default, properties::Property::Metal) > 0;
    let had_organic = properties::get(&default, properties::Property::Organic) > 0;
    (had_metal && properties::get(props, properties::Property::Metal) == 0)
        || (had_organic && properties::get(props, properties::Property::Organic) == 0)
}

// ---------------------------------------------------------------------------
// Qualitative property descriptors
// ---------------------------------------------------------------------------

/// Maximum buffer size for a described item name.
/// Worst realistic case: "razor-edged, smoldering Long Sword" = 34 chars.
pub const DESCRIBED_NAME_MAX: usize = 64;

/// Maximum number of adjectives prepended to an item name.
const MAX_ADJECTIVES: usize = 2;

/// Return (low_adjective, high_adjective) for a property, or None for
/// properties that don't get descriptors (Metal, Organic — structural,
/// not evocative).
const fn adjectives(prop: properties::Property) -> Option<(&'static str, &'static str)> {
    use properties::Property;
    match prop {
        Property::Sharp => Some(("keen", "razor-edged")),
        Property::Hard => Some(("sturdy", "unyielding")),
        Property::Heavy => Some(("weighty", "ponderous")),
        Property::Swift => Some(("nimble", "flickering")),
        Property::Hot => Some(("warm", "smoldering")),
        Property::Cold => Some(("chilled", "frozen")),
        Property::Wet => Some(("damp", "dripping")),
        Property::Metal => None,
        Property::Organic => None,
        Property::Venomous => Some(("tainted", "venomous")),
        Property::Magical => Some(("enchanted", "arcane")),
        Property::Volatile => Some(("unstable", "volatile")),
        Property::Bright => Some(("glowing", "luminous")),
        Property::Corrosive => Some(("acrid", "corrosive")),
        Property::Binding => Some(("clinging", "binding")),
        Property::Cursed => Some(("eerie", "cursed")),
    }
}

/// Build a descriptive item name with qualitative adjectives for properties
/// that exceed the item's defaults. Writes to `buf`, returns bytes written.
///
/// - If `props == default_properties(kind)`, writes just the base name.
/// - Otherwise, prepends up to 2 adjectives for the properties with the
///   largest positive delta from default, e.g. "smoldering, luminous Short Sword".
/// - Delta 1-3: low-tier adjective. Delta 4+: high-tier adjective.
/// - Metal and Organic are skipped (structural, not evocative).
pub fn describe_name(kind: ItemKind, props: &PropertyBag, buf: &mut [u8]) -> usize {
    let default = default_properties(kind);

    // Find properties where current > default, track top-2 by delta.
    // Each entry: (delta, property_index).
    let mut top: [(u8, u8); MAX_ADJECTIVES] = [(0, 0); MAX_ADJECTIVES];
    let mut count: usize = 0;

    let mut idx: u8 = 0;
    while idx < 16 {
        let cur = properties::get_by_index(props, idx);
        let def = properties::get_by_index(&default, idx);
        if cur > def {
            // Skip Metal (7) and Organic (8).
            if idx != 7 && idx != 8 {
                let delta = cur - def;
                // Insert into top-2 sorted by delta descending, then by index ascending.
                if count < MAX_ADJECTIVES {
                    top[count] = (delta, idx);
                    count += 1;
                    // Bubble up if needed.
                    if count == 2
                        && (top[1].0 > top[0].0 || (top[1].0 == top[0].0 && top[1].1 < top[0].1))
                    {
                        top.swap(0, 1);
                    }
                } else if delta > top[1].0 || (delta == top[1].0 && idx < top[1].1) {
                    // Replace the weaker entry.
                    top[1] = (delta, idx);
                    // Re-sort.
                    if top[1].0 > top[0].0 || (top[1].0 == top[0].0 && top[1].1 < top[0].1) {
                        top.swap(0, 1);
                    }
                }
            }
        }
        idx += 1;
    }

    // Write adjectives then base name into buffer.
    let mut pos: usize = 0;

    if count > 0 {
        // Sort output by property index for stable display order.
        if count == 2 && top[0].1 > top[1].1 {
            top.swap(0, 1);
        }

        for (i, &(delta, pidx)) in top[..count].iter().enumerate() {
            // Safety: pidx is 0-15, ALL_PROPERTIES has 16 entries.
            let prop = properties::ALL_PROPERTIES[pidx as usize];
            if let Some((low, high)) = adjectives(prop) {
                let adj = if delta >= 4 { high } else { low };
                let adj_bytes = adj.as_bytes();
                let mut j = 0;
                while j < adj_bytes.len() && pos < buf.len() {
                    buf[pos] = adj_bytes[j];
                    pos += 1;
                    j += 1;
                }
                // Add separator: ", " between adjectives, " " before name.
                if i + 1 < count && pos + 2 <= buf.len() {
                    buf[pos] = b',';
                    pos += 1;
                    buf[pos] = b' ';
                    pos += 1;
                } else if pos < buf.len() {
                    buf[pos] = b' ';
                    pos += 1;
                }
            }
        }
    }

    // Append base item name.
    let base = name(kind).as_bytes();
    let mut j = 0;
    while j < base.len() && pos < buf.len() {
        buf[pos] = base[j];
        pos += 1;
        j += 1;
    }

    pos
}

// ---------------------------------------------------------------------------
// Property-based stat derivation
// ---------------------------------------------------------------------------

/// Compute attack bonus from a property bag.
///
/// Primary: max(SHARP, (HARD + HEAVY) / 2), halved with round-up.
/// Elemental: max(HOT, COLD, VENOMOUS, CORROSIVE) / 4.
///
/// For default property bags this reproduces the hardcoded `attack_bonus(kind)`
/// values exactly: ShortSword→3, IronMace→4, LongSword→5.
pub const fn attack_from_bag(bag: &PropertyBag) -> u8 {
    let sharp = properties::get(bag, properties::Property::Sharp);
    let hard = properties::get(bag, properties::Property::Hard);
    let heavy = properties::get(bag, properties::Property::Heavy);

    // Primary: SHARP for edged weapons, (HARD+HEAVY)/2 for blunt
    let blunt = (hard + heavy) / 2; // max 15, fits u8
    let primary = if sharp >= blunt { sharp } else { blunt };
    let base = (primary + 1) >> 1;

    // Elemental bonus: strongest elemental property / 4
    let hot = properties::get(bag, properties::Property::Hot);
    let cold = properties::get(bag, properties::Property::Cold);
    let venomous = properties::get(bag, properties::Property::Venomous);
    let corrosive = properties::get(bag, properties::Property::Corrosive);
    let ab = if hot > cold { hot } else { cold };
    let cd = if venomous > corrosive {
        venomous
    } else {
        corrosive
    };
    let elem = (if ab > cd { ab } else { cd }) / 4;

    base + elem
}

/// Compute defense bonus from a property bag.
///
/// HARD / 2, plain integer division.
///
/// For default property bags this reproduces the hardcoded `defense_bonus(kind)`
/// values exactly: LeatherArmor→2, ChainMail→4.
pub const fn defense_from_bag(bag: &PropertyBag) -> u8 {
    properties::get(bag, properties::Property::Hard) / 2
}

// ---------------------------------------------------------------------------
// Equipment (shared across all tiers)
// ---------------------------------------------------------------------------

/// Tracked equipment slots for the player.
///
/// Pure data + bonus lookups — no coordinates, no allocation, `no_std`.
/// Combat reads effective stats from equipped property bags (Step 3).
/// The `weapon_props`/`armor_props` fields carry the per-instance property
/// bag that was on the item when it was equipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
pub struct Equipment {
    pub weapon: Option<ItemKind>,
    pub armor: Option<ItemKind>,
    pub weapon_props: PropertyBag,
    pub armor_props: PropertyBag,
}

// ---------------------------------------------------------------------------
// Inventory (shared across all tiers)
// ---------------------------------------------------------------------------

/// Maximum inventory slots (a-z). Shared across all tiers.
pub const MAX_INVENTORY: usize = 26;

/// A single inventory slot — item kind, stack count, and per-instance property bag.
///
/// Properties are initialized from `default_properties(kind)` on pickup and can
/// diverge through interactions. Two items stack only if they share the same kind
/// AND the same property bag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InvSlot {
    pub kind: ItemKind,
    pub count: u8,
    pub props: PropertyBag,
}

/// Brogue-style 26-slot inventory (a-z). Consumables stack (same kind + same
/// property bag), equipment takes individual slots.
///
/// 26 × `Option<InvSlot>` = 286 bytes with property bags.
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

    /// Add an item with default properties. Consumables stack in existing
    /// slots if they share the same kind AND same property bag; equipment
    /// takes a new slot. Returns `false` if inventory is full.
    pub fn add(&mut self, kind: ItemKind) -> bool {
        let props = default_properties(kind);
        self.add_with_props(kind, props)
    }

    /// Add an item with specific properties. Used when picking up items
    /// that already have modified property bags (e.g., from the ground
    /// after environmental interactions).
    pub fn add_with_props(&mut self, kind: ItemKind, props: PropertyBag) -> bool {
        self.add_with_props_and_stackable(kind, props, is_consumable(kind))
    }

    /// Add an item using a caller-provided stacking policy. The Standard tier
    /// uses this for validated runtime catalogs; constrained tiers call the
    /// static `add_with_props` path above.
    pub fn add_with_props_and_stackable(
        &mut self,
        kind: ItemKind,
        props: PropertyBag,
        stackable: bool,
    ) -> bool {
        // Try to stack consumables in an existing slot with matching props.
        if stackable {
            for slot in self.slots.iter_mut().flatten() {
                if slot.kind == kind && slot.props == props {
                    slot.count = slot.count.saturating_add(1);
                    return true;
                }
            }
        }

        // Find first empty slot.
        for slot in &mut self.slots {
            if slot.is_none() {
                *slot = Some(InvSlot {
                    kind,
                    count: 1,
                    props,
                });
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
        let mut n = 0;
        let mut i = 0;
        while i < MAX_INVENTORY {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Whether the inventory is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether all 26 slots are occupied.
    pub fn is_full(&self) -> bool {
        let mut i = 0;
        while i < MAX_INVENTORY {
            if self.slots[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
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

    /// Update the property bag of an occupied slot. Used by the combine
    /// system to write back modified properties after an interaction.
    #[allow(clippy::collapsible_if)]
    pub fn set_props(&mut self, idx: usize, props: PropertyBag) {
        if idx < MAX_INVENTORY {
            if let Some(slot) = &mut self.slots[idx] {
                slot.props = props;
            }
        }
    }

    /// Replace untouched default property bags while preserving instances
    /// changed by environmental or item interactions.
    pub fn reconcile_default_properties(
        &mut self,
        old_defaults: &[PropertyBag; KIND_COUNT],
        new_defaults: &[PropertyBag; KIND_COUNT],
    ) {
        for slot in self.slots.iter_mut().flatten() {
            let index = slot.kind as usize;
            if slot.props == old_defaults[index] {
                slot.props = new_defaults[index];
            }
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

impl Default for Equipment {
    fn default() -> Self {
        Self {
            weapon: None,
            armor: None,
            weapon_props: properties::EMPTY,
            armor_props: properties::EMPTY,
        }
    }
}

impl Equipment {
    /// Attack bonus from equipped weapon's property bag.
    pub fn attack_bonus(&self) -> u8 {
        if self.weapon.is_some() {
            attack_from_bag(&self.weapon_props)
        } else {
            0
        }
    }

    /// Defense bonus from equipped armor's property bag.
    pub fn defense_bonus(&self) -> u8 {
        if self.armor.is_some() {
            defense_from_bag(&self.armor_props)
        } else {
            0
        }
    }

    /// Populate empty property bags from default_properties for each occupied
    /// slot. Called after deserialization to migrate saves that predate the
    /// property system (bags default to EMPTY via serde/binary format).
    #[allow(clippy::collapsible_if)]
    pub fn fixup_empty_bags(&mut self) {
        if let Some(kind) = self.weapon {
            if self.weapon_props == properties::EMPTY {
                self.weapon_props = default_properties(kind);
            }
        }
        if let Some(kind) = self.armor {
            if self.armor_props == properties::EMPTY {
                self.armor_props = default_properties(kind);
            }
        }
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
            weapon_props: default_properties(ItemKind::ShortSword),
            armor: None,
            armor_props: properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 3);
        assert_eq!(eq.defense_bonus(), 0);
    }

    #[test]
    fn equipment_armor_bonus() {
        let eq = Equipment {
            weapon: None,
            weapon_props: properties::EMPTY,
            armor: Some(ItemKind::LeatherArmor),
            armor_props: default_properties(ItemKind::LeatherArmor),
        };
        assert_eq!(eq.attack_bonus(), 0);
        assert_eq!(eq.defense_bonus(), 2);
    }

    #[test]
    fn equipment_both_slots() {
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: default_properties(ItemKind::ShortSword),
            armor: Some(ItemKind::LeatherArmor),
            armor_props: default_properties(ItemKind::LeatherArmor),
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
        assert_eq!(min_depth(ItemKind::IronMace), 3);
    }

    #[test]
    fn long_sword_properties() {
        assert_eq!(name(ItemKind::LongSword), "Long Sword");
        assert_eq!(attack_bonus(ItemKind::LongSword), 5);
        assert!(is_weapon(ItemKind::LongSword));
        assert_eq!(min_depth(ItemKind::LongSword), 7);
    }

    #[test]
    fn chain_mail_properties() {
        assert_eq!(name(ItemKind::ChainMail), "Chain Mail");
        assert_eq!(defense_bonus(ItemKind::ChainMail), 4);
        assert!(is_armor(ItemKind::ChainMail));
        assert_eq!(min_depth(ItemKind::ChainMail), 9);
    }

    #[test]
    fn greater_health_potion_properties() {
        assert_eq!(name(ItemKind::GreaterHealthPotion), "Greater Health Potion");
        assert_eq!(heal_amount(ItemKind::GreaterHealthPotion), 20);
        assert!(is_consumable(ItemKind::GreaterHealthPotion));
        assert_eq!(min_depth(ItemKind::GreaterHealthPotion), 11);
    }

    #[test]
    fn strength_potion_properties() {
        assert_eq!(name(ItemKind::StrengthPotion), "Potion of Strength");
        assert_eq!(strength_boost(ItemKind::StrengthPotion), 1);
        assert_eq!(heal_amount(ItemKind::StrengthPotion), 0);
        assert!(is_consumable(ItemKind::StrengthPotion));
        assert_eq!(min_depth(ItemKind::StrengthPotion), 5);
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
            weapon_props: default_properties(ItemKind::LongSword),
            armor: None,
            armor_props: properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 5);
    }

    #[test]
    fn equipment_chain_mail_bonus() {
        let eq = Equipment {
            weapon: None,
            weapon_props: properties::EMPTY,
            armor: Some(ItemKind::ChainMail),
            armor_props: default_properties(ItemKind::ChainMail),
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

    // ── Property-based stat derivation tests ─────────────────────────

    #[test]
    fn attack_from_bag_matches_hardcoded_for_all_weapons() {
        // The formula must reproduce the hardcoded attack_bonus values
        // for default property bags — this anchors Step 3 to Step 1.
        assert_eq!(
            attack_from_bag(&default_properties(ItemKind::ShortSword)),
            attack_bonus(ItemKind::ShortSword)
        );
        assert_eq!(
            attack_from_bag(&default_properties(ItemKind::IronMace)),
            attack_bonus(ItemKind::IronMace)
        );
        assert_eq!(
            attack_from_bag(&default_properties(ItemKind::LongSword)),
            attack_bonus(ItemKind::LongSword)
        );
    }

    #[test]
    fn defense_from_bag_matches_hardcoded_for_all_armors() {
        assert_eq!(
            defense_from_bag(&default_properties(ItemKind::LeatherArmor)),
            defense_bonus(ItemKind::LeatherArmor)
        );
        assert_eq!(
            defense_from_bag(&default_properties(ItemKind::ChainMail)),
            defense_bonus(ItemKind::ChainMail)
        );
    }

    #[test]
    fn attack_from_bag_zero_for_empty_bag() {
        assert_eq!(attack_from_bag(&properties::EMPTY), 0);
    }

    #[test]
    fn defense_from_bag_zero_for_empty_bag() {
        assert_eq!(defense_from_bag(&properties::EMPTY), 0);
    }

    #[test]
    fn attack_from_bag_includes_elemental_bonus() {
        // Fire-dipped ShortSword: base ATK 3 + HOT:7 → elemental 7/4=1 → total 4.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hot, 7);
        assert_eq!(attack_from_bag(&bag), 4);
    }

    #[test]
    fn equipment_reads_from_property_bag() {
        // Equip with actual property bags — should use attack_from_bag.
        let bag = default_properties(ItemKind::ShortSword);
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: bag,
            armor: None,
            armor_props: properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 3);
    }

    #[test]
    fn equipment_modified_bag_changes_bonus() {
        // A fire-dipped sword should give +1 ATK from elemental bonus.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hot, 7);
        let eq = Equipment {
            weapon: Some(ItemKind::ShortSword),
            weapon_props: bag,
            armor: None,
            armor_props: properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 4); // 3 base + 1 elemental
    }

    #[test]
    fn equipment_empty_bag_gives_zero_without_fixup() {
        // EMPTY bag with occupied slot → 0 stats (no fallback).
        let eq = Equipment {
            weapon: Some(ItemKind::LongSword),
            weapon_props: properties::EMPTY,
            armor: Some(ItemKind::ChainMail),
            armor_props: properties::EMPTY,
        };
        assert_eq!(eq.attack_bonus(), 0);
        assert_eq!(eq.defense_bonus(), 0);
    }

    #[test]
    fn fixup_empty_bags_populates_defaults() {
        let mut eq = Equipment {
            weapon: Some(ItemKind::LongSword),
            weapon_props: properties::EMPTY,
            armor: Some(ItemKind::ChainMail),
            armor_props: properties::EMPTY,
        };
        eq.fixup_empty_bags();
        assert_eq!(eq.attack_bonus(), 5);
        assert_eq!(eq.defense_bonus(), 4);
    }

    #[test]
    fn floor_1_items_have_min_depth_1() {
        assert_eq!(min_depth(ItemKind::HealthPotion), 1);
        assert_eq!(min_depth(ItemKind::ShortSword), 1);
        assert_eq!(min_depth(ItemKind::LeatherArmor), 1);
    }

    // ── Property-aware stacking tests ─────────────────────────────────

    #[test]
    fn inventory_stacks_with_matching_props() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::HealthPotion);
        inv.add(ItemKind::HealthPotion);
        // Same kind + same default props → should stack
        assert_eq!(inv.len(), 1);
        assert_eq!(inv.get(0).unwrap().count, 2);
    }

    #[test]
    fn inventory_no_stack_different_props() {
        let mut inv = Inventory::new();
        inv.add(ItemKind::HealthPotion);
        // Modify the first potion's props
        let mut modified_props = default_properties(ItemKind::HealthPotion);
        properties::set(&mut modified_props, properties::Property::Hot, 5);
        inv.set_props(0, modified_props);
        // Add another default potion — different props, should NOT stack
        inv.add(ItemKind::HealthPotion);
        assert_eq!(inv.len(), 2);
        assert_eq!(inv.get(0).unwrap().count, 1);
        assert_eq!(inv.get(1).unwrap().count, 1);
    }

    #[test]
    fn inventory_stacks_matching_modified_props() {
        let mut inv = Inventory::new();
        // Add a potion with modified props directly
        let mut hot_props = default_properties(ItemKind::HealthPotion);
        properties::set(&mut hot_props, properties::Property::Hot, 5);
        inv.add_with_props(ItemKind::HealthPotion, hot_props);
        inv.add_with_props(ItemKind::HealthPotion, hot_props);
        // Same kind + same modified props → should stack
        assert_eq!(inv.len(), 1);
        assert_eq!(inv.get(0).unwrap().count, 2);
    }

    #[test]
    fn from_snake_case_roundtrips_all_kinds() {
        for &kind in &ALL_KINDS {
            let snake = name(kind).to_ascii_lowercase().replace(' ', "_");
            assert_eq!(
                from_snake_case(&snake),
                Some(kind),
                "from_snake_case({:?}) should return {:?}",
                snake,
                kind
            );
        }
    }

    #[test]
    fn from_snake_case_case_insensitive() {
        assert_eq!(from_snake_case("IRON_MACE"), Some(ItemKind::IronMace));
        assert_eq!(
            from_snake_case("Health_Potion"),
            Some(ItemKind::HealthPotion)
        );
    }

    #[test]
    fn from_snake_case_unknown() {
        assert_eq!(from_snake_case("dragon_sword"), None);
        assert_eq!(from_snake_case(""), None);
    }

    // ── describe_name tests ────────────────────────────────────────────

    fn describe(kind: ItemKind, props: &PropertyBag) -> String {
        let mut buf = [0u8; DESCRIBED_NAME_MAX];
        let len = describe_name(kind, props, &mut buf);
        core::str::from_utf8(&buf[..len]).unwrap().to_string()
    }

    #[test]
    fn describe_default_gives_plain_name() {
        for kind in ALL_KINDS {
            let bag = default_properties(kind);
            assert_eq!(
                describe(kind, &bag),
                name(kind),
                "default bag for {:?} should produce plain name",
                kind
            );
        }
    }

    #[test]
    fn describe_single_low_boost() {
        // Short Sword default HARD=7, boost to 9 (delta 2 → low tier "sturdy")
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hard, 9);
        assert_eq!(describe(ItemKind::ShortSword, &bag), "sturdy Short Sword");
    }

    #[test]
    fn describe_single_high_boost() {
        // Short Sword default HARD=7, boost to 12 (delta 5 → high tier "unyielding")
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hard, 12);
        assert_eq!(
            describe(ItemKind::ShortSword, &bag),
            "unyielding Short Sword"
        );
    }

    #[test]
    fn describe_two_adjectives_ordered_by_property_index() {
        // Short Sword: boost HARD (idx 1, delta 5) and BRIGHT (idx 12, delta 3).
        // Output order by property index: Hard before Bright.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hard, 12); // delta 5
        properties::set(&mut bag, properties::Property::Bright, 3); // delta 3 (from 0)
        assert_eq!(
            describe(ItemKind::ShortSword, &bag),
            "unyielding, glowing Short Sword"
        );
    }

    #[test]
    fn describe_max_two_adjectives() {
        // Boost 4 properties. Only top 2 by delta should appear.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hot, 8); // delta 8 (from 0)
        properties::set(&mut bag, properties::Property::Bright, 6); // delta 6 (from 0)
        properties::set(&mut bag, properties::Property::Cold, 2); // delta 2 (from 0)
        properties::set(&mut bag, properties::Property::Cursed, 1); // delta 1 (from 0)
        let result = describe(ItemKind::ShortSword, &bag);
        // Top 2 by delta: HOT (8), BRIGHT (6). Order by index: Hot(4), Bright(12).
        assert_eq!(result, "smoldering, luminous Short Sword");
    }

    #[test]
    fn describe_metal_organic_skipped() {
        // Boost only Metal (idx 7) above default. Should produce plain name.
        let mut bag = default_properties(ItemKind::ShortSword);
        // Default Metal=8, boost to 15.
        properties::set(&mut bag, properties::Property::Metal, 15);
        assert_eq!(describe(ItemKind::ShortSword, &bag), "Short Sword");
    }

    #[test]
    fn describe_ties_broken_by_property_index() {
        // Two properties with equal delta. Lower index appears first.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hot, 3); // delta 3, idx 4
        properties::set(&mut bag, properties::Property::Cold, 3); // delta 3, idx 5
        let result = describe(ItemKind::ShortSword, &bag);
        assert_eq!(result, "warm, chilled Short Sword");
    }

    #[test]
    fn describe_decreased_no_adjective() {
        // SHARP default=6 for Short Sword, decrease to 2. No adjective.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Sharp, 2);
        assert_eq!(describe(ItemKind::ShortSword, &bag), "Short Sword");
    }

    #[test]
    fn describe_buffer_truncation() {
        // Tiny buffer — should write as much as fits without panicking.
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Hot, 8);
        let mut buf = [0u8; 5];
        let len = describe_name(ItemKind::ShortSword, &bag, &mut buf);
        // Should fill the buffer without exceeding it.
        assert_eq!(len, 5);
        // First 5 bytes of "smoldering Short Sword" = "smold"
        assert_eq!(&buf, b"smold");
    }

    #[test]
    fn describe_tempered_sword() {
        // Simulate tempering: Short Sword + Strength Potion should boost
        // combat properties and add BRIGHT through chain reactions.
        // Rather than running the full interaction engine, manually set
        // the expected post-temper state.
        let mut bag = default_properties(ItemKind::ShortSword);
        // Tempering adds HOT and BRIGHT through chain reactions.
        properties::set(&mut bag, properties::Property::Hot, 2); // delta 2 → "warm"
        properties::set(&mut bag, properties::Property::Bright, 5); // delta 5 → "luminous"
        let result = describe(ItemKind::ShortSword, &bag);
        assert_eq!(result, "warm, luminous Short Sword");
    }

    #[test]
    fn describe_organic_skipped_even_with_large_delta() {
        // Leather Armor default ORGANIC=6. Boost to 15 (delta 9).
        // Organic is skipped, so no adjective from it.
        let mut bag = default_properties(ItemKind::LeatherArmor);
        properties::set(&mut bag, properties::Property::Organic, 15);
        assert_eq!(describe(ItemKind::LeatherArmor, &bag), "Leather Armor");
    }

    #[test]
    fn describe_adjectives_all_valid_utf8() {
        // Every adjective string returned by adjectives() should be valid ASCII.
        for &prop in &properties::ALL_PROPERTIES {
            if let Some((low, high)) = adjectives(prop) {
                assert!(low.is_ascii(), "low adjective for {:?} not ASCII", prop);
                assert!(high.is_ascii(), "high adjective for {:?} not ASCII", prop);
            }
        }
    }

    // ── is_material_dead tests ──────────────────────────────────────

    #[test]
    fn material_dead_metal_zero() {
        let mut bag = default_properties(ItemKind::ShortSword);
        assert!(!is_material_dead(ItemKind::ShortSword, &bag));
        properties::set(&mut bag, properties::Property::Metal, 0);
        assert!(is_material_dead(ItemKind::ShortSword, &bag));
    }

    #[test]
    fn material_dead_organic_zero() {
        let mut bag = default_properties(ItemKind::LeatherArmor);
        assert!(!is_material_dead(ItemKind::LeatherArmor, &bag));
        properties::set(&mut bag, properties::Property::Organic, 0);
        assert!(is_material_dead(ItemKind::LeatherArmor, &bag));
    }

    #[test]
    fn material_dead_metal_above_zero() {
        let mut bag = default_properties(ItemKind::ShortSword);
        properties::set(&mut bag, properties::Property::Metal, 1);
        assert!(!is_material_dead(ItemKind::ShortSword, &bag));
    }

    #[test]
    fn potions_cannot_die() {
        for kind in [
            ItemKind::HealthPotion,
            ItemKind::GreaterHealthPotion,
            ItemKind::StrengthPotion,
        ] {
            // Zero out everything — potions still can't die (no material)
            let bag = properties::EMPTY;
            assert!(
                !is_material_dead(kind, &bag),
                "{:?} should not be material-dead",
                kind
            );
        }
    }

    #[test]
    fn is_material_dead_all_equipment() {
        // All metal items die when METAL=0
        for kind in [
            ItemKind::ShortSword,
            ItemKind::IronMace,
            ItemKind::LongSword,
            ItemKind::ChainMail,
        ] {
            let mut bag = default_properties(kind);
            properties::set(&mut bag, properties::Property::Metal, 0);
            assert!(
                is_material_dead(kind, &bag),
                "{:?} should die when METAL=0",
                kind
            );
        }
        // Leather armor dies when ORGANIC=0
        let mut bag = default_properties(ItemKind::LeatherArmor);
        properties::set(&mut bag, properties::Property::Organic, 0);
        assert!(is_material_dead(ItemKind::LeatherArmor, &bag));
    }
}
