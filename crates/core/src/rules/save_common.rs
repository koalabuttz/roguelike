//! Shared save utilities used by all tier serializers.
//!
//! CRC-16-CCITT checksum, `SaveError` enum, and enum encode/decode helpers
//! live here so that micro, compact, and (future) standard saves share
//! the same wire format for common types.

use super::items::ItemKind;
use super::monster_table::{AiBehavior, MonsterKind};

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

pub fn encode_ai_behavior(ai: AiBehavior) -> u8 {
    match ai {
        AiBehavior::None => 0,
        AiBehavior::Chase => 1,
        AiBehavior::Wander => 2,
    }
}

pub fn decode_ai_behavior(b: u8) -> AiBehavior {
    match b {
        1 => AiBehavior::Chase,
        2 => AiBehavior::Wander,
        _ => AiBehavior::None,
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

        // AiBehavior
        for &ai in &[AiBehavior::None, AiBehavior::Chase, AiBehavior::Wander] {
            let encoded = encode_ai_behavior(ai);
            assert_eq!(decode_ai_behavior(encoded), ai);
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
}
