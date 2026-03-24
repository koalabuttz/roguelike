//! Structured game events shared by all capability tiers.
//!
//! `GameEvent` is `Copy` and `no_std` compatible — constrained platforms
//! (C64, GBA) map events directly to fixed-width display strings without
//! heap allocation. The standard tier converts events to `String` via
//! `format_event()` in `message_log.rs`.

use core::mem::size_of;

use super::health::HealthTier;
use super::items::ItemKind;
use super::monster_table::MonsterKind;

/// Why autorun stopped — mirrors `MicroAutorunStop` but lives in the
/// shared `rules` layer so all tiers can produce messages from it.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutorunStopCause {
    WallReached = 0,
    MonsterSpotted = 1,
    DamageTaken = 2,
    GameOver = 3,
    CorridorBranches = 4,
    MaxSteps = 5,
    PathComplete = 6,
    StairsFound = 7,
}

/// An actor in a game event — the player, a known monster type, or an
/// unrecognized monster (custom/modded entities without a `MonsterKind`).
///
/// 2 bytes: 1-byte discriminant + 1-byte `MonsterKind` payload (or padding
/// for unit variants). Small enough for all tiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combatant {
    Player,
    Monster(MonsterKind),
    /// A monster entity without a `MonsterKind` (custom/modded).
    UnknownMonster,
}

impl Combatant {
    /// Display name for this combatant. Returns `&'static str` — no allocation.
    pub const fn name(self) -> &'static str {
        match self {
            Combatant::Player => "Player",
            Combatant::Monster(kind) => super::monster_table::name(kind),
            Combatant::UnknownMonster => "Something",
        }
    }
}

/// Distance category for sound cues from wandering monsters.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundDistance {
    /// Manhattan distance <= sound_near threshold.
    Near = 0,
    /// Manhattan distance <= sound_medium threshold.
    Medium = 1,
    /// Manhattan distance <= sound_far threshold.
    Far = 2,
}

/// A structured game event, `Copy` and `no_std` compatible.
///
/// Standard tier converts these to `String` via `format_event()` in
/// `message_log.rs`. Constrained tiers (C64, GBA) convert to fixed-width
/// display formats without heap allocation.
///
/// Every production `log.add(format!(...))` call should be replaced with
/// `log.add_event(GameEvent::...)` to keep messages structured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameEvent {
    /// Melee attack that dealt damage.
    Attack {
        attacker: Combatant,
        defender: Combatant,
        damage: u8,
    },
    /// Melee attack that dealt zero damage (defense >= attack).
    NoDamage {
        attacker: Combatant,
        defender: Combatant,
    },
    /// An entity was killed.
    Kill {
        attacker: Combatant,
        victim: Combatant,
    },
    /// A combatant crossed a health tier boundary (emitted after Attack,
    /// only when the defender is still alive and tier changed).
    HealthStatus { who: Combatant, tier: HealthTier },
    /// An entity noticed the player and switched to chase AI.
    EntityNotice { who: Combatant },
    /// Player consumed a potion.
    DrinkPotion { kind: ItemKind, healed: u8 },
    /// Player equipped a weapon.
    EquipWeapon { kind: ItemKind, bonus: u8 },
    /// Player equipped armor.
    EquipArmor { kind: ItemKind, bonus: u8 },
    /// Player unequipped a weapon.
    UnequipWeapon { kind: ItemKind },
    /// Player unequipped armor.
    UnequipArmor { kind: ItemKind },
    /// Tried to descend but no stairs on this tile.
    NoStairs,
    /// Player descended to a new dungeon depth.
    Descend { depth: u8, target: u8 },
    /// Player won the game by ascending from the final depth.
    Victory { depth: u8 },
    /// Welcome message at game start.
    Welcome,
    /// Distance-based sound cue from wandering monsters.
    SoundCue { distance: SoundDistance },
    /// The player died.
    PlayerDeath,
    /// Player picked up an item from the ground.
    PickupItem { kind: ItemKind },
    /// Player dropped an item on the ground.
    DropItem { kind: ItemKind },
    /// Pickup failed — inventory is full.
    InventoryFull,
    /// Notification: item(s) on the ground at player position.
    ItemsHere { kind: ItemKind, count: u8 },
    /// Autorun started.
    Autorun,
    /// Autorun finished — carries the stop reason.
    AutorunStop { cause: AutorunStopCause },
    /// Player consumed a stat-boosting potion.
    UseStrengthPotion { bonus: u8 },
    /// Player combined two items (applied source properties to target).
    CombineItems { target: ItemKind, source: ItemKind },
    /// Combine attempt produced no property changes.
    CombineNoEffect,
}

// Compile-time size checks — keep these small for constrained tiers.
const _: () = assert!(size_of::<Combatant>() <= 2);
const _: () = assert!(size_of::<SoundDistance>() == 1);
const _: () = assert!(size_of::<GameEvent>() <= 8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combatant_player_name() {
        assert_eq!(Combatant::Player.name(), "Player");
    }

    #[test]
    fn combatant_monster_name() {
        assert_eq!(Combatant::Monster(MonsterKind::Goblin).name(), "Goblin");
        assert_eq!(Combatant::Monster(MonsterKind::Orc).name(), "Orc");
        assert_eq!(Combatant::Monster(MonsterKind::Troll).name(), "Troll");
    }

    #[test]
    fn game_event_is_copy() {
        let event = GameEvent::Attack {
            attacker: Combatant::Player,
            defender: Combatant::Monster(MonsterKind::Goblin),
            damage: 5,
        };
        let copy = event;
        assert_eq!(event, copy);
    }

    #[test]
    fn sound_distance_repr() {
        assert_eq!(SoundDistance::Near as u8, 0);
        assert_eq!(SoundDistance::Medium as u8, 1);
        assert_eq!(SoundDistance::Far as u8, 2);
    }
}
