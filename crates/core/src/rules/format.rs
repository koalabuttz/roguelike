//! No-std ASCII text formatting for port HUDs and message logs.
//!
//! Ports without an allocator can't use `format!` or build `String`s — they
//! write directly into fixed-size byte buffers. This module provides the
//! shared primitives: decimal/integer rendering, string concatenation, and
//! the `GameEvent` → ASCII formatter consumed by the compact-tier ports
//! (GBA, NDS) and any future port with the same constraints.
//!
//! # Overflow handling
//!
//! Writes past `buf.len()` are silently dropped. The returned position may
//! exceed `buf.len()` — callers should clamp at use sites if they need a
//! valid index. [`format_event`] clamps its return value for convenience.
//!
//! # Why a slice, not a fixed-size array
//!
//! Each port has a different screen width. GBA's HUD row holds 30 bytes;
//! the NDS bottom screen holds 32. A slice parameter lets both callers
//! pass their native buffer without parameterizing the function on a const
//! generic.
//!
//! # Wire-format contract
//!
//! C64 intentionally does not share this formatter — its PETSCII encoding,
//! different phrasing, and per-variant bonus values diverge enough that
//! parameterization would defeat the deduplication. See
//! `crates/c64/src/render.rs` for the C64 formatter.

use crate::rules::health::HealthTier;
use crate::rules::items;
use crate::rules::message::{Combatant, GameEvent};

/// Write a `u16` as decimal ASCII digits into `buf` starting at `pos`.
/// Returns the position one past the last digit written.
pub fn write_u16(buf: &mut [u8], pos: usize, val: u16) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
        }
        return pos + 1;
    }

    // Extract digits in reverse (small stack buffer; u16::MAX is 5 digits).
    let mut digits = [0u8; 5];
    let mut n = val;
    let mut count = 0;
    while n > 0 {
        digits[count] = b'0' + (n % 10) as u8;
        n /= 10;
        count += 1;
    }

    let mut p = pos;
    for i in (0..count).rev() {
        if p < buf.len() {
            buf[p] = digits[i];
        }
        p += 1;
    }
    p
}

/// Write a `&str` into `buf` starting at `pos`.
/// Returns the position one past the last byte written.
pub fn write_str(buf: &mut [u8], pos: usize, s: &str) -> usize {
    let mut p = pos;
    for &byte in s.as_bytes() {
        if p < buf.len() {
            buf[p] = byte;
        }
        p += 1;
    }
    p
}

/// Write a `u32` as 8-digit uppercase hexadecimal into `buf` starting at `pos`.
/// Always writes exactly 8 bytes (the full width of a u32). Returns `pos + 8`.
pub fn write_u32_hex(buf: &mut [u8], pos: usize, val: u32) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut p = pos;
    for i in (0..8).rev() {
        if p < buf.len() {
            buf[p] = HEX[((val >> (i * 4)) & 0xF) as usize];
        }
        p += 1;
    }
    p
}

/// Write a `u16` as 4-digit uppercase hexadecimal into `buf` starting at `pos`.
/// Always writes exactly 4 bytes (the full width of a u16). Returns `pos + 4`.
pub fn write_u16_hex(buf: &mut [u8], pos: usize, val: u16) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut p = pos;
    for shift in [12, 8, 4, 0] {
        if p < buf.len() {
            buf[p] = HEX[((val >> shift) & 0xF) as usize];
        }
        p += 1;
    }
    p
}

/// Write a combatant name: "You" for the player, the monster name otherwise.
///
/// Exposed so ports that want the "You vs. monster" convention but need custom
/// sentence structure (e.g. C64's PETSCII formatter) can reuse it without
/// duplicating the logic.
pub fn write_combatant(buf: &mut [u8], pos: usize, who: Combatant) -> usize {
    match who {
        Combatant::Player => write_str(buf, pos, "You"),
        _ => write_str(buf, pos, who.name()),
    }
}

/// Format a `GameEvent` into the caller-provided ASCII buffer.
///
/// Fills the buffer with spaces first so unused trailing bytes render as
/// padding. Returns bytes written, clamped to `buf.len()`.
pub fn format_event(event: GameEvent, buf: &mut [u8]) -> usize {
    buf.fill(b' ');
    format_event_inner(event, buf).min(buf.len())
}

fn format_event_inner(event: GameEvent, buf: &mut [u8]) -> usize {
    match event {
        GameEvent::Welcome => write_str(buf, 0, "Welcome to the dungeon!"),

        GameEvent::Attack {
            attacker,
            defender,
            damage,
        } => {
            let mut p = write_combatant(buf, 0, attacker);
            let verb = if matches!(attacker, Combatant::Player) {
                " hit "
            } else {
                " hits "
            };
            p = write_str(buf, p, verb);
            p = write_combatant(buf, p, defender);
            p = write_str(buf, p, " for ");
            p = write_u16(buf, p, damage as u16);
            p
        }

        GameEvent::NoDamage { attacker, defender } => {
            let mut p = write_combatant(buf, 0, attacker);
            let verb = if matches!(attacker, Combatant::Player) {
                " miss "
            } else {
                " misses "
            };
            p = write_str(buf, p, verb);
            write_combatant(buf, p, defender)
        }

        GameEvent::Kill { attacker, victim } => {
            let mut p = write_combatant(buf, 0, attacker);
            let verb = if matches!(attacker, Combatant::Player) {
                " killed "
            } else {
                " kills "
            };
            p = write_str(buf, p, verb);
            write_combatant(buf, p, victim)
        }

        GameEvent::PlayerDeath => write_str(buf, 0, "You have been slain..."),

        GameEvent::Descend { depth, .. } => {
            let p = write_str(buf, 0, "Descended to depth ");
            write_u16(buf, p, depth as u16)
        }

        GameEvent::Victory { .. } => write_str(buf, 0, "You escaped the dungeon!"),

        GameEvent::PickupItem { kind } => {
            let p = write_str(buf, 0, "Picked up ");
            write_str(buf, p, items::name(kind))
        }

        GameEvent::DropItem { kind } => {
            let p = write_str(buf, 0, "Dropped ");
            write_str(buf, p, items::name(kind))
        }

        GameEvent::DrinkPotion { healed, .. } => {
            let mut p = write_str(buf, 0, "Healed ");
            p = write_u16(buf, p, healed as u16);
            write_str(buf, p, " HP")
        }

        GameEvent::EquipWeapon { kind, .. } | GameEvent::EquipArmor { kind, .. } => {
            let p = write_str(buf, 0, "Equipped ");
            write_str(buf, p, items::name(kind))
        }

        GameEvent::UnequipWeapon { kind } | GameEvent::UnequipArmor { kind } => {
            let p = write_str(buf, 0, "Unequipped ");
            write_str(buf, p, items::name(kind))
        }

        GameEvent::NoStairs => write_str(buf, 0, "No stairs here."),

        GameEvent::InventoryFull => write_str(buf, 0, "Inventory full!"),

        GameEvent::ItemsHere { kind, count } => {
            if count == 1 {
                let p = write_str(buf, 0, "You see ");
                write_str(buf, p, items::name(kind))
            } else {
                let mut p = write_str(buf, 0, "Items here (");
                p = write_u16(buf, p, count as u16);
                write_str(buf, p, ")")
            }
        }

        GameEvent::EntityNotice { who } => {
            let mut p = write_str(buf, 0, "The ");
            p = write_str(buf, p, who.name());
            write_str(buf, p, " notices you!")
        }

        GameEvent::AutorunStop { .. } => write_str(buf, 0, "Stopped."),
        GameEvent::Autorun => write_str(buf, 0, "Running..."),

        GameEvent::SoundCue { .. } => write_str(buf, 0, "You hear something..."),

        GameEvent::HealthStatus { who, tier } => match who {
            Combatant::Player => write_str(
                buf,
                0,
                match tier {
                    HealthTier::Healthy => "You look healthy",
                    HealthTier::Moderate => "You look damaged",
                    HealthTier::Severe => "You look wounded",
                    HealthTier::AlmostDead => "You are dying",
                },
            ),
            _ => {
                let p = write_combatant(buf, 0, who);
                write_str(
                    buf,
                    p,
                    match tier {
                        HealthTier::Healthy => " looks healthy",
                        HealthTier::Moderate => " looks damaged",
                        HealthTier::Severe => " looks wounded",
                        HealthTier::AlmostDead => " is dying",
                    },
                )
            }
        },

        GameEvent::UseStrengthPotion { bonus } => {
            let mut p = write_str(buf, 0, "ATK +");
            p = write_u16(buf, p, bonus as u16);
            write_str(buf, p, "!")
        }

        GameEvent::UseToughnessPotion { bonus } => {
            let mut p = write_str(buf, 0, "DEF +");
            p = write_u16(buf, p, bonus as u16);
            write_str(buf, p, "!")
        }

        GameEvent::CombineItems { target, .. } => {
            let p = write_str(buf, 0, "Combined ");
            write_str(buf, p, items::name(target))
        }

        GameEvent::CombineNoEffect => write_str(buf, 0, "No effect."),

        GameEvent::ItemDestroyed { kind } => {
            let p = write_str(buf, 0, items::name(kind));
            write_str(buf, p, " destroyed!")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::items::ItemKind;
    use crate::rules::message::{AutorunStopCause, SoundDistance};
    use crate::rules::monster_table::MonsterKind;

    /// Render into a 64-byte buffer (larger than any port's HUD row) and
    /// assert exact byte equality. Keeps tests alloc-free so they run in
    /// any no_std context the shared module might be used from.
    #[track_caller]
    fn assert_renders(event: GameEvent, expected: &str) {
        let mut buf = [0u8; 64];
        let len = format_event(event, &mut buf);
        let got = core::str::from_utf8(&buf[..len]).unwrap();
        assert_eq!(got, expected, "event: {event:?}");
    }

    #[test]
    fn welcome() {
        assert_renders(GameEvent::Welcome, "Welcome to the dungeon!");
    }

    #[test]
    fn player_attack() {
        assert_renders(
            GameEvent::Attack {
                attacker: Combatant::Player,
                defender: Combatant::Monster(MonsterKind::Orc),
                damage: 7,
            },
            "You hit Orc for 7",
        );
    }

    #[test]
    fn monster_attack() {
        assert_renders(
            GameEvent::Attack {
                attacker: Combatant::Monster(MonsterKind::Goblin),
                defender: Combatant::Player,
                damage: 3,
            },
            "Goblin hits You for 3",
        );
    }

    #[test]
    fn player_miss() {
        assert_renders(
            GameEvent::NoDamage {
                attacker: Combatant::Player,
                defender: Combatant::Monster(MonsterKind::Orc),
            },
            "You miss Orc",
        );
    }

    #[test]
    fn monster_miss() {
        assert_renders(
            GameEvent::NoDamage {
                attacker: Combatant::Monster(MonsterKind::Troll),
                defender: Combatant::Player,
            },
            "Troll misses You",
        );
    }

    #[test]
    fn player_kill() {
        assert_renders(
            GameEvent::Kill {
                attacker: Combatant::Player,
                victim: Combatant::Monster(MonsterKind::Goblin),
            },
            "You killed Goblin",
        );
    }

    #[test]
    fn monster_kill() {
        assert_renders(
            GameEvent::Kill {
                attacker: Combatant::Monster(MonsterKind::Troll),
                victim: Combatant::Player,
            },
            "Troll kills You",
        );
    }

    #[test]
    fn player_death() {
        assert_renders(GameEvent::PlayerDeath, "You have been slain...");
    }

    #[test]
    fn descend() {
        assert_renders(
            GameEvent::Descend {
                depth: 3,
                target: 5,
            },
            "Descended to depth 3",
        );
    }

    #[test]
    fn victory() {
        assert_renders(GameEvent::Victory { depth: 5 }, "You escaped the dungeon!");
    }

    #[test]
    fn pickup_item() {
        assert_renders(
            GameEvent::PickupItem {
                kind: ItemKind::HealthPotion,
            },
            "Picked up Health Potion",
        );
    }

    #[test]
    fn drop_item() {
        assert_renders(
            GameEvent::DropItem {
                kind: ItemKind::ShortSword,
            },
            "Dropped Short Sword",
        );
    }

    #[test]
    fn drink_potion() {
        assert_renders(
            GameEvent::DrinkPotion {
                kind: ItemKind::HealthPotion,
                healed: 10,
            },
            "Healed 10 HP",
        );
    }

    #[test]
    fn equip_weapon() {
        assert_renders(
            GameEvent::EquipWeapon {
                kind: ItemKind::IronMace,
                bonus: 3,
            },
            "Equipped Iron Mace",
        );
    }

    #[test]
    fn equip_armor() {
        assert_renders(
            GameEvent::EquipArmor {
                kind: ItemKind::LeatherArmor,
                bonus: 2,
            },
            "Equipped Leather Armor",
        );
    }

    #[test]
    fn unequip_weapon() {
        assert_renders(
            GameEvent::UnequipWeapon {
                kind: ItemKind::LongSword,
            },
            "Unequipped Long Sword",
        );
    }

    #[test]
    fn unequip_armor() {
        assert_renders(
            GameEvent::UnequipArmor {
                kind: ItemKind::ChainMail,
            },
            "Unequipped Chain Mail",
        );
    }

    #[test]
    fn no_stairs() {
        assert_renders(GameEvent::NoStairs, "No stairs here.");
    }

    #[test]
    fn inventory_full() {
        assert_renders(GameEvent::InventoryFull, "Inventory full!");
    }

    #[test]
    fn items_here_singular() {
        assert_renders(
            GameEvent::ItemsHere {
                kind: ItemKind::HealthPotion,
                count: 1,
            },
            "You see Health Potion",
        );
    }

    #[test]
    fn items_here_plural() {
        assert_renders(
            GameEvent::ItemsHere {
                kind: ItemKind::HealthPotion,
                count: 3,
            },
            "Items here (3)",
        );
    }

    #[test]
    fn entity_notice() {
        assert_renders(
            GameEvent::EntityNotice {
                who: Combatant::Monster(MonsterKind::Goblin),
            },
            "The Goblin notices you!",
        );
    }

    #[test]
    fn autorun() {
        assert_renders(GameEvent::Autorun, "Running...");
    }

    #[test]
    fn autorun_stop() {
        assert_renders(
            GameEvent::AutorunStop {
                cause: AutorunStopCause::MonsterSpotted,
            },
            "Stopped.",
        );
    }

    #[test]
    fn sound_cue() {
        assert_renders(
            GameEvent::SoundCue {
                distance: SoundDistance::Near,
            },
            "You hear something...",
        );
    }

    #[test]
    fn player_health_healthy() {
        assert_renders(
            GameEvent::HealthStatus {
                who: Combatant::Player,
                tier: HealthTier::Healthy,
            },
            "You look healthy",
        );
    }

    #[test]
    fn player_health_dying() {
        assert_renders(
            GameEvent::HealthStatus {
                who: Combatant::Player,
                tier: HealthTier::AlmostDead,
            },
            "You are dying",
        );
    }

    #[test]
    fn monster_health_wounded() {
        assert_renders(
            GameEvent::HealthStatus {
                who: Combatant::Monster(MonsterKind::Orc),
                tier: HealthTier::Severe,
            },
            "Orc looks wounded",
        );
    }

    #[test]
    fn monster_health_dying() {
        assert_renders(
            GameEvent::HealthStatus {
                who: Combatant::Monster(MonsterKind::Troll),
                tier: HealthTier::AlmostDead,
            },
            "Troll is dying",
        );
    }

    #[test]
    fn use_strength_potion() {
        assert_renders(GameEvent::UseStrengthPotion { bonus: 2 }, "ATK +2!");
        assert_renders(GameEvent::UseToughnessPotion { bonus: 2 }, "DEF +2!");
    }

    #[test]
    fn combine_items() {
        assert_renders(
            GameEvent::CombineItems {
                target: ItemKind::ShortSword,
                source: ItemKind::IronMace,
            },
            "Combined Short Sword",
        );
    }

    #[test]
    fn combine_no_effect() {
        assert_renders(GameEvent::CombineNoEffect, "No effect.");
    }

    #[test]
    fn item_destroyed() {
        assert_renders(
            GameEvent::ItemDestroyed {
                kind: ItemKind::LeatherArmor,
            },
            "Leather Armor destroyed!",
        );
    }

    // ── Buffer overflow behavior ────────────────────────────────────

    #[test]
    fn small_buffer_truncates_without_panicking() {
        let ev = GameEvent::Welcome;
        let mut buf = [0u8; 5];
        let len = format_event(ev, &mut buf);
        assert_eq!(len, 5);
        assert_eq!(&buf, b"Welco");
    }

    #[test]
    fn empty_buffer_returns_zero() {
        let ev = GameEvent::Welcome;
        let mut buf: [u8; 0] = [];
        let len = format_event(ev, &mut buf);
        assert_eq!(len, 0);
    }

    // ── write_u16 / write_str basic checks ──────────────────────────

    #[test]
    fn write_u16_zero() {
        let mut buf = [b'.'; 5];
        let p = write_u16(&mut buf, 0, 0);
        assert_eq!(p, 1);
        assert_eq!(&buf, b"0....");
    }

    #[test]
    fn write_u16_max() {
        let mut buf = [b'.'; 5];
        let p = write_u16(&mut buf, 0, 65535);
        assert_eq!(p, 5);
        assert_eq!(&buf, b"65535");
    }

    #[test]
    fn write_str_past_end() {
        let mut buf = [0u8; 5];
        let p = write_str(&mut buf, 3, "hello");
        assert_eq!(p, 8);
        assert_eq!(&buf, b"\0\0\0he");
    }
}
