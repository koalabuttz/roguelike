//! Shared save utilities used by all tier serializers.
//!
//! CRC-16-CCITT checksum, `SaveError` enum, and enum encode/decode helpers
//! live here so that micro, compact, and (future) standard saves share
//! the same wire format for common types.

use super::health::HealthTier;
use super::items::ItemKind;
use super::message::{AutorunStopCause, Combatant, GameEvent, SoundDistance};
use super::monster_table::{AiPersonality, MonsterKind};

// ---------------------------------------------------------------------------
// Format constants
// ---------------------------------------------------------------------------

pub const SAVE_MAGIC: [u8; 2] = *b"RG";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveError {
    /// Magic bytes don't match — not a save file.
    BadMagic,
    /// Format version is newer than this build supports.
    BadVersion,
    /// CRC-16 mismatch — data is corrupted.
    BadChecksum,
    /// File ended before all fields were read.
    UnexpectedEof,
    /// A field value is out of the valid range (e.g. entity count > max).
    BadData,
}

// ---------------------------------------------------------------------------
// CRC-16-CCITT (polynomial 0x1021, initial 0xFFFF)
// ---------------------------------------------------------------------------

/// Update CRC-16-CCITT with one byte. Bit-by-bit computation is tiny
/// on 6502 (~30 bytes of machine code).
pub fn crc16_update(crc: u16, byte: u8) -> u16 {
    let mut c = crc ^ ((byte as u16) << 8);
    let mut i: u8 = 0;
    while i < 8 {
        if c & 0x8000 != 0 {
            c = (c << 1) ^ 0x1021;
        } else {
            c <<= 1;
        }
        i += 1;
    }
    c
}

/// Compute CRC-16-CCITT over a byte slice.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut i = 0;
    while i < data.len() {
        crc = crc16_update(crc, data[i]);
        i += 1;
    }
    crc
}

// ---------------------------------------------------------------------------
// Enum encode/decode — explicit matches, independent of repr ordering
// ---------------------------------------------------------------------------

pub fn encode_opt_monster_kind(k: Option<MonsterKind>) -> u8 {
    match k {
        None => 0xFF,
        Some(MonsterKind::Goblin) => 0,
        Some(MonsterKind::Orc) => 1,
        Some(MonsterKind::Troll) => 2,
    }
}

pub fn decode_opt_monster_kind(b: u8) -> Option<MonsterKind> {
    match b {
        0 => Some(MonsterKind::Goblin),
        1 => Some(MonsterKind::Orc),
        2 => Some(MonsterKind::Troll),
        _ => None,
    }
}

pub fn encode_ai_personality(ai: AiPersonality) -> u8 {
    match ai {
        AiPersonality::Player => 0,
        AiPersonality::Aggressive => 1,
        AiPersonality::Patrol => 2,
        AiPersonality::Coward => 3,
    }
}

pub fn decode_ai_personality(b: u8) -> AiPersonality {
    match b {
        1 => AiPersonality::Aggressive,
        2 => AiPersonality::Patrol,
        3 => AiPersonality::Coward,
        _ => AiPersonality::Player,
    }
}

pub fn encode_opt_item_kind(k: Option<ItemKind>) -> u8 {
    match k {
        None => 0xFF,
        Some(ItemKind::HealthPotion) => 0,
        Some(ItemKind::ShortSword) => 1,
        Some(ItemKind::LeatherArmor) => 2,
        Some(ItemKind::IronMace) => 3,
        Some(ItemKind::LongSword) => 4,
        Some(ItemKind::ChainMail) => 5,
        Some(ItemKind::GreaterHealthPotion) => 6,
        Some(ItemKind::StrengthPotion) => 7,
        Some(ItemKind::ToughnessPotion) => 8,
    }
}

pub fn decode_opt_item_kind(b: u8) -> Option<ItemKind> {
    match b {
        0 => Some(ItemKind::HealthPotion),
        1 => Some(ItemKind::ShortSword),
        2 => Some(ItemKind::LeatherArmor),
        3 => Some(ItemKind::IronMace),
        4 => Some(ItemKind::LongSword),
        5 => Some(ItemKind::ChainMail),
        6 => Some(ItemKind::GreaterHealthPotion),
        7 => Some(ItemKind::StrengthPotion),
        8 => Some(ItemKind::ToughnessPotion),
        _ => None,
    }
}

pub fn encode_item_kind(k: ItemKind) -> u8 {
    encode_opt_item_kind(Some(k))
}

pub fn decode_item_kind(b: u8) -> ItemKind {
    decode_opt_item_kind(b).unwrap_or(ItemKind::HealthPotion)
}

// ---------------------------------------------------------------------------
// GameEvent encode/decode — fixed 4-byte representation per event slot.
// Byte 0: tag (0x00–0x19 = variant, 0xFF = None).
// Bytes 1–3: payload (zero-padded for shorter variants).
// ---------------------------------------------------------------------------

/// Size of one encoded event slot: 1 tag + 3 payload bytes.
pub const ENCODED_EVENT_SIZE: usize = 4;

pub fn encode_combatant(c: Combatant) -> u8 {
    match c {
        Combatant::Player => 0,
        Combatant::Monster(MonsterKind::Goblin) => 1,
        Combatant::Monster(MonsterKind::Orc) => 2,
        Combatant::Monster(MonsterKind::Troll) => 3,
        Combatant::UnknownMonster => 0xFE,
    }
}

pub fn decode_combatant(b: u8) -> Combatant {
    match b {
        0 => Combatant::Player,
        1 => Combatant::Monster(MonsterKind::Goblin),
        2 => Combatant::Monster(MonsterKind::Orc),
        3 => Combatant::Monster(MonsterKind::Troll),
        _ => Combatant::UnknownMonster,
    }
}

pub fn encode_health_tier(t: HealthTier) -> u8 {
    match t {
        HealthTier::Healthy => 0,
        HealthTier::Moderate => 1,
        HealthTier::Severe => 2,
        HealthTier::AlmostDead => 3,
    }
}

pub fn decode_health_tier(b: u8) -> HealthTier {
    match b {
        0 => HealthTier::Healthy,
        1 => HealthTier::Moderate,
        2 => HealthTier::Severe,
        _ => HealthTier::AlmostDead,
    }
}

pub fn encode_sound_distance(d: SoundDistance) -> u8 {
    match d {
        SoundDistance::Near => 0,
        SoundDistance::Medium => 1,
        SoundDistance::Far => 2,
    }
}

pub fn decode_sound_distance(b: u8) -> SoundDistance {
    match b {
        0 => SoundDistance::Near,
        1 => SoundDistance::Medium,
        _ => SoundDistance::Far,
    }
}

pub fn encode_autorun_stop(c: AutorunStopCause) -> u8 {
    match c {
        AutorunStopCause::WallReached => 0,
        AutorunStopCause::MonsterSpotted => 1,
        AutorunStopCause::DamageTaken => 2,
        AutorunStopCause::GameOver => 3,
        AutorunStopCause::CorridorBranches => 4,
        AutorunStopCause::MaxSteps => 5,
        AutorunStopCause::PathComplete => 6,
        AutorunStopCause::StairsFound => 7,
    }
}

pub fn decode_autorun_stop(b: u8) -> AutorunStopCause {
    match b {
        0 => AutorunStopCause::WallReached,
        1 => AutorunStopCause::MonsterSpotted,
        2 => AutorunStopCause::DamageTaken,
        3 => AutorunStopCause::GameOver,
        4 => AutorunStopCause::CorridorBranches,
        5 => AutorunStopCause::MaxSteps,
        6 => AutorunStopCause::PathComplete,
        _ => AutorunStopCause::StairsFound,
    }
}

/// Encode an `Option<GameEvent>` to a fixed 4-byte representation.
///
/// Returns `[tag, b1, b2, b3]` — tag 0xFF means None, tags 0–25 are variants.
/// Unused payload bytes are zero.
pub fn encode_game_event(event: Option<GameEvent>) -> [u8; ENCODED_EVENT_SIZE] {
    match event {
        None => [0xFF, 0, 0, 0],
        Some(e) => match e {
            GameEvent::Attack {
                attacker,
                defender,
                damage,
            } => [
                0,
                encode_combatant(attacker),
                encode_combatant(defender),
                damage,
            ],
            GameEvent::NoDamage { attacker, defender } => {
                [1, encode_combatant(attacker), encode_combatant(defender), 0]
            }
            GameEvent::Kill { attacker, victim } => {
                [2, encode_combatant(attacker), encode_combatant(victim), 0]
            }
            GameEvent::HealthStatus { who, tier } => {
                [3, encode_combatant(who), encode_health_tier(tier), 0]
            }
            GameEvent::EntityNotice { who } => [4, encode_combatant(who), 0, 0],
            GameEvent::DrinkPotion { kind, healed } => [5, encode_item_kind(kind), healed, 0],
            GameEvent::EquipWeapon { kind, bonus } => [6, encode_item_kind(kind), bonus, 0],
            GameEvent::EquipArmor { kind, bonus } => [7, encode_item_kind(kind), bonus, 0],
            GameEvent::UnequipWeapon { kind } => [8, encode_item_kind(kind), 0, 0],
            GameEvent::UnequipArmor { kind } => [9, encode_item_kind(kind), 0, 0],
            GameEvent::NoStairs => [10, 0, 0, 0],
            GameEvent::Descend { depth, target } => [11, depth, target, 0],
            GameEvent::Victory { depth } => [12, depth, 0, 0],
            GameEvent::Welcome => [13, 0, 0, 0],
            GameEvent::SoundCue { distance } => [14, encode_sound_distance(distance), 0, 0],
            GameEvent::PlayerDeath => [15, 0, 0, 0],
            GameEvent::PickupItem { kind } => [16, encode_item_kind(kind), 0, 0],
            GameEvent::DropItem { kind } => [17, encode_item_kind(kind), 0, 0],
            GameEvent::InventoryFull => [18, 0, 0, 0],
            GameEvent::ItemsHere { kind, count } => [19, encode_item_kind(kind), count, 0],
            GameEvent::Autorun => [20, 0, 0, 0],
            GameEvent::AutorunStop { cause } => [21, encode_autorun_stop(cause), 0, 0],
            GameEvent::UseStrengthPotion { bonus } => [22, bonus, 0, 0],
            GameEvent::CombineItems { target, source } => {
                [23, encode_item_kind(target), encode_item_kind(source), 0]
            }
            GameEvent::CombineNoEffect => [24, 0, 0, 0],
            GameEvent::ItemDestroyed { kind } => [25, encode_item_kind(kind), 0, 0],
            GameEvent::UseToughnessPotion { bonus } => [26, bonus, 0, 0],
        },
    }
}

/// Decode a 4-byte representation back to `Option<GameEvent>`.
///
/// Returns `None` for tag 0xFF or any unrecognized tag (forward compat).
pub fn decode_game_event(bytes: [u8; ENCODED_EVENT_SIZE]) -> Option<GameEvent> {
    match bytes[0] {
        0 => Some(GameEvent::Attack {
            attacker: decode_combatant(bytes[1]),
            defender: decode_combatant(bytes[2]),
            damage: bytes[3],
        }),
        1 => Some(GameEvent::NoDamage {
            attacker: decode_combatant(bytes[1]),
            defender: decode_combatant(bytes[2]),
        }),
        2 => Some(GameEvent::Kill {
            attacker: decode_combatant(bytes[1]),
            victim: decode_combatant(bytes[2]),
        }),
        3 => Some(GameEvent::HealthStatus {
            who: decode_combatant(bytes[1]),
            tier: decode_health_tier(bytes[2]),
        }),
        4 => Some(GameEvent::EntityNotice {
            who: decode_combatant(bytes[1]),
        }),
        5 => Some(GameEvent::DrinkPotion {
            kind: decode_item_kind(bytes[1]),
            healed: bytes[2],
        }),
        6 => Some(GameEvent::EquipWeapon {
            kind: decode_item_kind(bytes[1]),
            bonus: bytes[2],
        }),
        7 => Some(GameEvent::EquipArmor {
            kind: decode_item_kind(bytes[1]),
            bonus: bytes[2],
        }),
        8 => Some(GameEvent::UnequipWeapon {
            kind: decode_item_kind(bytes[1]),
        }),
        9 => Some(GameEvent::UnequipArmor {
            kind: decode_item_kind(bytes[1]),
        }),
        10 => Some(GameEvent::NoStairs),
        11 => Some(GameEvent::Descend {
            depth: bytes[1],
            target: bytes[2],
        }),
        12 => Some(GameEvent::Victory { depth: bytes[1] }),
        13 => Some(GameEvent::Welcome),
        14 => Some(GameEvent::SoundCue {
            distance: decode_sound_distance(bytes[1]),
        }),
        15 => Some(GameEvent::PlayerDeath),
        16 => Some(GameEvent::PickupItem {
            kind: decode_item_kind(bytes[1]),
        }),
        17 => Some(GameEvent::DropItem {
            kind: decode_item_kind(bytes[1]),
        }),
        18 => Some(GameEvent::InventoryFull),
        19 => Some(GameEvent::ItemsHere {
            kind: decode_item_kind(bytes[1]),
            count: bytes[2],
        }),
        20 => Some(GameEvent::Autorun),
        21 => Some(GameEvent::AutorunStop {
            cause: decode_autorun_stop(bytes[1]),
        }),
        22 => Some(GameEvent::UseStrengthPotion { bonus: bytes[1] }),
        23 => Some(GameEvent::CombineItems {
            target: decode_item_kind(bytes[1]),
            source: decode_item_kind(bytes[2]),
        }),
        24 => Some(GameEvent::CombineNoEffect),
        25 => Some(GameEvent::ItemDestroyed {
            kind: decode_item_kind(bytes[1]),
        }),
        26 => Some(GameEvent::UseToughnessPotion { bonus: bytes[1] }),
        _ => None, // 0xFF (None) or unknown future tag
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_vectors() {
        // "123456789" should produce 0x29B1 for CRC-16-CCITT-FALSE
        assert_eq!(crc16(b"123456789"), 0x29B1);
        assert_eq!(crc16(b""), 0xFFFF); // empty input = initial value
    }

    #[test]
    fn enum_encode_decode_round_trip() {
        // MonsterKind
        for &mk in &[MonsterKind::Goblin, MonsterKind::Orc, MonsterKind::Troll] {
            let encoded = encode_opt_monster_kind(Some(mk));
            assert_eq!(decode_opt_monster_kind(encoded), Some(mk));
        }
        assert_eq!(decode_opt_monster_kind(encode_opt_monster_kind(None)), None);
        assert_eq!(decode_opt_monster_kind(0xFE), None); // unknown

        // AiPersonality
        for &ai in &[
            AiPersonality::Player,
            AiPersonality::Aggressive,
            AiPersonality::Patrol,
            AiPersonality::Coward,
        ] {
            let encoded = encode_ai_personality(ai);
            assert_eq!(decode_ai_personality(encoded), ai);
        }

        // ItemKind
        for &ik in &[
            ItemKind::HealthPotion,
            ItemKind::ShortSword,
            ItemKind::LeatherArmor,
        ] {
            let encoded = encode_opt_item_kind(Some(ik));
            assert_eq!(decode_opt_item_kind(encoded), Some(ik));
            assert_eq!(decode_item_kind(encode_item_kind(ik)), ik);
        }
        assert_eq!(decode_opt_item_kind(encode_opt_item_kind(None)), None);
    }

    #[test]
    fn combatant_round_trip() {
        let cases = [
            Combatant::Player,
            Combatant::Monster(MonsterKind::Goblin),
            Combatant::Monster(MonsterKind::Orc),
            Combatant::Monster(MonsterKind::Troll),
            Combatant::UnknownMonster,
        ];
        for c in cases {
            assert_eq!(decode_combatant(encode_combatant(c)), c);
        }
    }

    #[test]
    fn game_event_round_trip_all_variants() {
        use crate::rules::health::HealthTier;
        use crate::rules::message::{AutorunStopCause, SoundDistance};

        let events: &[Option<GameEvent>] = &[
            None,
            Some(GameEvent::Attack {
                attacker: Combatant::Player,
                defender: Combatant::Monster(MonsterKind::Orc),
                damage: 7,
            }),
            Some(GameEvent::NoDamage {
                attacker: Combatant::Monster(MonsterKind::Goblin),
                defender: Combatant::Player,
            }),
            Some(GameEvent::Kill {
                attacker: Combatant::Player,
                victim: Combatant::Monster(MonsterKind::Troll),
            }),
            Some(GameEvent::HealthStatus {
                who: Combatant::Monster(MonsterKind::Orc),
                tier: HealthTier::Severe,
            }),
            Some(GameEvent::EntityNotice {
                who: Combatant::Monster(MonsterKind::Goblin),
            }),
            Some(GameEvent::DrinkPotion {
                kind: ItemKind::HealthPotion,
                healed: 10,
            }),
            Some(GameEvent::EquipWeapon {
                kind: ItemKind::ShortSword,
                bonus: 3,
            }),
            Some(GameEvent::EquipArmor {
                kind: ItemKind::LeatherArmor,
                bonus: 2,
            }),
            Some(GameEvent::UnequipWeapon {
                kind: ItemKind::LongSword,
            }),
            Some(GameEvent::UnequipArmor {
                kind: ItemKind::ChainMail,
            }),
            Some(GameEvent::NoStairs),
            Some(GameEvent::Descend {
                depth: 3,
                target: 5,
            }),
            Some(GameEvent::Victory { depth: 5 }),
            Some(GameEvent::Welcome),
            Some(GameEvent::SoundCue {
                distance: SoundDistance::Near,
            }),
            Some(GameEvent::SoundCue {
                distance: SoundDistance::Far,
            }),
            Some(GameEvent::PlayerDeath),
            Some(GameEvent::PickupItem {
                kind: ItemKind::HealthPotion,
            }),
            Some(GameEvent::DropItem {
                kind: ItemKind::IronMace,
            }),
            Some(GameEvent::InventoryFull),
            Some(GameEvent::ItemsHere {
                kind: ItemKind::StrengthPotion,
                count: 2,
            }),
            Some(GameEvent::Autorun),
            Some(GameEvent::AutorunStop {
                cause: AutorunStopCause::MonsterSpotted,
            }),
            Some(GameEvent::UseStrengthPotion { bonus: 1 }),
            Some(GameEvent::UseToughnessPotion { bonus: 1 }),
            Some(GameEvent::CombineItems {
                target: ItemKind::ShortSword,
                source: ItemKind::IronMace,
            }),
            Some(GameEvent::CombineNoEffect),
            Some(GameEvent::ItemDestroyed {
                kind: ItemKind::LeatherArmor,
            }),
        ];

        for &event in events {
            let encoded = encode_game_event(event);
            let decoded = decode_game_event(encoded);
            assert_eq!(decoded, event, "round-trip failed for {event:?}");
        }
    }

    #[test]
    fn game_event_none_is_0xff() {
        let encoded = encode_game_event(None);
        assert_eq!(encoded[0], 0xFF);
    }

    #[test]
    fn game_event_unknown_tag_decodes_to_none() {
        // Future tags or corrupt data → None (forward compat)
        assert_eq!(decode_game_event([0xFE, 0, 0, 0]), None);
        assert_eq!(decode_game_event([27, 0, 0, 0]), None);
        assert_eq!(decode_game_event([99, 1, 2, 3]), None);
    }
}
