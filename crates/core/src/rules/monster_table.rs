//! Pure monster type definitions and lookup functions for all capability tiers.
//!
//! This module defines `MonsterKind`, `AiBehavior`, and all tier-portable
//! monster queries. Every lookup is a `const fn` with explicit exhaustive
//! matches — no wildcard arms — so the compiler forces coverage when variants
//! are added.
//!
//! **Stat contract:** All stat return types are `u8` — the smallest tier's
//! natural width. The standard-tier `Entity` struct uses signed `Stat` (`i32`)
//! for combat math; callers widen `u8 → Stat` via lossless `as` casts.

use super::balance;
use crate::types::GameColor;

/// Monster AI behavior. Shared by all tiers — the C64 maps discriminants to
/// `u8` constants, the standard tier uses enum variants directly.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AiBehavior {
    None = 0,   // Player — no automatic AI
    Chase = 1,  // Greedy chase toward player
    Wander = 2, // Random walk; switches to Chase when player enters LOS
}

/// The type of monster. Each variant is a `u8` discriminant — no 16-bit bloat
/// on constrained platforms (C64, GBA).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MonsterKind {
    Goblin = 0,
    Orc = 1,
    Troll = 2,
}

/// All monster kinds, for iteration. Adding a variant to `MonsterKind` without
/// adding it here will cause exhaustive-match compile errors in `glyph()`
/// et al., plus the `all_kinds_covers_every_variant` test catches desync.
pub const ALL_KINDS: [MonsterKind; 3] = [MonsterKind::Goblin, MonsterKind::Orc, MonsterKind::Troll];

/// Number of monster kinds, derived from `ALL_KINDS` — never manually synced.
pub const KIND_COUNT: usize = ALL_KINDS.len();

// ---------------------------------------------------------------------------
// Display lookups
// ---------------------------------------------------------------------------

/// Display glyph for a monster kind.
pub const fn glyph(kind: MonsterKind) -> char {
    match kind {
        MonsterKind::Goblin => balance::GOBLIN_GLYPH,
        MonsterKind::Orc => balance::ORC_GLYPH,
        MonsterKind::Troll => balance::TROLL_GLYPH,
    }
}

/// Display color for a monster kind.
pub const fn color(kind: MonsterKind) -> GameColor {
    match kind {
        MonsterKind::Goblin => GameColor::Green,
        MonsterKind::Orc => GameColor::Red,
        MonsterKind::Troll => GameColor::DarkGreen,
    }
}

/// Human-readable name for a monster kind.
pub const fn name(kind: MonsterKind) -> &'static str {
    match kind {
        MonsterKind::Goblin => "Goblin",
        MonsterKind::Orc => "Orc",
        MonsterKind::Troll => "Troll",
    }
}

// ---------------------------------------------------------------------------
// Stat lookups (all u8 for tier portability, all const fn, no wildcards)
// ---------------------------------------------------------------------------

/// Spawn weight — single source of truth, indexed by kind.
/// `SPAWN_TABLE` and `spawn_weight()` both read from here.
const WEIGHTS: [u8; KIND_COUNT] = [
    balance::GOBLIN_SPAWN_WEIGHT,
    balance::ORC_SPAWN_WEIGHT,
    balance::TROLL_SPAWN_WEIGHT,
];

/// Maximum hit points for a monster kind.
pub const fn max_hp(kind: MonsterKind) -> u8 {
    match kind {
        MonsterKind::Goblin => balance::GOBLIN_HP,
        MonsterKind::Orc => balance::ORC_HP,
        MonsterKind::Troll => balance::TROLL_HP,
    }
}

/// Attack stat for a monster kind.
pub const fn attack(kind: MonsterKind) -> u8 {
    match kind {
        MonsterKind::Goblin => balance::GOBLIN_ATK,
        MonsterKind::Orc => balance::ORC_ATK,
        MonsterKind::Troll => balance::TROLL_ATK,
    }
}

/// Defense stat for a monster kind.
pub const fn defense(kind: MonsterKind) -> u8 {
    match kind {
        MonsterKind::Goblin => balance::GOBLIN_DEF,
        MonsterKind::Orc => balance::ORC_DEF,
        MonsterKind::Troll => balance::TROLL_DEF,
    }
}

/// Sight radius for a monster kind (how far it can detect the player).
pub const fn sight_radius(kind: MonsterKind) -> u8 {
    match kind {
        MonsterKind::Goblin => balance::GOBLIN_SIGHT,
        MonsterKind::Orc => balance::ORC_SIGHT,
        MonsterKind::Troll => balance::TROLL_SIGHT,
    }
}

/// Spawn weight for the weighted monster spawn table.
/// Higher weight = more common. Returns 0 to disable spawning.
pub const fn spawn_weight(kind: MonsterKind) -> u8 {
    WEIGHTS[kind as usize]
}

/// Default AI behavior for a monster kind.
pub const fn ai_behavior(kind: MonsterKind) -> AiBehavior {
    match kind {
        MonsterKind::Goblin => AiBehavior::Chase,
        MonsterKind::Orc => AiBehavior::Chase,
        MonsterKind::Troll => AiBehavior::Chase,
    }
}

// ---------------------------------------------------------------------------
// Spawn table (fixed-size, no allocation)
// ---------------------------------------------------------------------------

/// Parallel-array spawn table: kinds and weights in separate arrays.
/// On the 6502, indexing a `[u8; N]` is a single LDA abs,X — no multiply.
/// Keeping kinds and weights separate avoids struct-of-arrays index math
/// even if either array's element size changes in the future.
pub const SPAWN_KINDS: [MonsterKind; KIND_COUNT] =
    [MonsterKind::Goblin, MonsterKind::Orc, MonsterKind::Troll];

/// Spawn weights corresponding 1:1 with `SPAWN_KINDS`.
pub const SPAWN_WEIGHTS: [u8; KIND_COUNT] = WEIGHTS;

// ---------------------------------------------------------------------------
// Name → MonsterKind lookup (for bridging TOML/data.rs to MonsterKind)
// ---------------------------------------------------------------------------

/// Look up a `MonsterKind` by name (case-sensitive, matching `name()` output).
/// Returns `None` for unknown names.
pub const fn from_name(s: &str) -> Option<MonsterKind> {
    // const fn cannot use iterators, so manual matching on bytes.
    if str_eq(s, "Goblin") {
        Some(MonsterKind::Goblin)
    } else if str_eq(s, "Orc") {
        Some(MonsterKind::Orc)
    } else if str_eq(s, "Troll") {
        Some(MonsterKind::Troll)
    } else {
        None
    }
}

/// Const-compatible string equality (byte-by-byte).
const fn str_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goblin_properties() {
        assert_eq!(glyph(MonsterKind::Goblin), 'g');
        assert_eq!(color(MonsterKind::Goblin), GameColor::Green);
        assert_eq!(name(MonsterKind::Goblin), "Goblin");
        assert_eq!(max_hp(MonsterKind::Goblin), 6);
        assert_eq!(attack(MonsterKind::Goblin), 3);
        assert_eq!(defense(MonsterKind::Goblin), 0);
        assert_eq!(sight_radius(MonsterKind::Goblin), 6);
        assert_eq!(spawn_weight(MonsterKind::Goblin), 60);
        assert_eq!(ai_behavior(MonsterKind::Goblin), AiBehavior::Chase);
    }

    #[test]
    fn orc_properties() {
        assert_eq!(glyph(MonsterKind::Orc), 'o');
        assert_eq!(color(MonsterKind::Orc), GameColor::Red);
        assert_eq!(name(MonsterKind::Orc), "Orc");
        assert_eq!(max_hp(MonsterKind::Orc), 12);
        assert_eq!(attack(MonsterKind::Orc), 4);
        assert_eq!(defense(MonsterKind::Orc), 1);
        assert_eq!(sight_radius(MonsterKind::Orc), 7);
        assert_eq!(spawn_weight(MonsterKind::Orc), 30);
        assert_eq!(ai_behavior(MonsterKind::Orc), AiBehavior::Chase);
    }

    #[test]
    fn troll_properties() {
        assert_eq!(glyph(MonsterKind::Troll), 'T');
        assert_eq!(color(MonsterKind::Troll), GameColor::DarkGreen);
        assert_eq!(name(MonsterKind::Troll), "Troll");
        assert_eq!(max_hp(MonsterKind::Troll), 20);
        assert_eq!(attack(MonsterKind::Troll), 6);
        assert_eq!(defense(MonsterKind::Troll), 3);
        assert_eq!(sight_radius(MonsterKind::Troll), 5);
        assert_eq!(spawn_weight(MonsterKind::Troll), 10);
        assert_eq!(ai_behavior(MonsterKind::Troll), AiBehavior::Chase);
    }

    #[test]
    fn all_kinds_covers_every_variant() {
        let mut count = 0usize;
        for &kind in &ALL_KINDS {
            match kind {
                MonsterKind::Goblin | MonsterKind::Orc | MonsterKind::Troll => {}
            }
            assert!(!name(kind).is_empty());
            assert!(spawn_weight(kind) > 0);
            count += 1;
        }
        assert_eq!(count, KIND_COUNT);
    }

    #[test]
    fn repr_u8_discriminants() {
        assert_eq!(MonsterKind::Goblin as u8, 0);
        assert_eq!(MonsterKind::Orc as u8, 1);
        assert_eq!(MonsterKind::Troll as u8, 2);
    }

    #[test]
    fn ai_behavior_repr_u8() {
        assert_eq!(AiBehavior::None as u8, 0);
        assert_eq!(AiBehavior::Chase as u8, 1);
        assert_eq!(AiBehavior::Wander as u8, 2);
    }

    #[test]
    fn spawn_table_matches_weights() {
        for i in 0..KIND_COUNT {
            assert_eq!(SPAWN_WEIGHTS[i], spawn_weight(SPAWN_KINDS[i]));
        }
    }

    #[test]
    fn const_fn_usable_at_compile_time() {
        const GOBLIN_HP: u8 = max_hp(MonsterKind::Goblin);
        const ORC_ATK: u8 = attack(MonsterKind::Orc);
        const TROLL_DEF: u8 = defense(MonsterKind::Troll);
        assert_eq!(GOBLIN_HP, 6);
        assert_eq!(ORC_ATK, 4);
        assert_eq!(TROLL_DEF, 3);
    }

    #[test]
    fn from_name_known() {
        assert_eq!(from_name("Goblin"), Some(MonsterKind::Goblin));
        assert_eq!(from_name("Orc"), Some(MonsterKind::Orc));
        assert_eq!(from_name("Troll"), Some(MonsterKind::Troll));
    }

    #[test]
    fn from_name_unknown() {
        assert_eq!(from_name("goblin"), None); // case-sensitive
        assert_eq!(from_name("Dragon"), None);
        assert_eq!(from_name(""), None);
    }

    #[test]
    fn from_name_roundtrips_with_name() {
        for &kind in &ALL_KINDS {
            assert_eq!(from_name(name(kind)), Some(kind));
        }
    }
}
