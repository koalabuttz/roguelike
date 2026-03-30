//! No-std formatting helpers for GBA display.
//!
//! Since we can't use `format!` without alloc, these functions write
//! ASCII directly into fixed-size buffers.

use roguelike_core::rules::health::HealthTier;
use roguelike_core::rules::message::{Combatant, GameEvent};

/// Write a u32 as 8-digit uppercase hexadecimal into `buf` starting at `pos`.
/// Returns the new position after the 8 hex digits.
pub fn write_hex(buf: &mut [u8], pos: usize, val: u32) -> usize {
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

/// Write a u16 as decimal ASCII digits into `buf` starting at `pos`.
/// Returns the new position after the digits.
pub fn write_u16(buf: &mut [u8], pos: usize, val: u16) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
        }
        return pos + 1;
    }

    // Extract digits in reverse
    let mut digits = [0u8; 5];
    let mut n = val;
    let mut count = 0;
    while n > 0 {
        digits[count] = b'0' + (n % 10) as u8;
        n /= 10;
        count += 1;
    }

    // Write digits in correct order
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
/// Returns the new position after the string.
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

/// Write a combatant name, truncating "Player" to "You" for readability.
fn write_combatant(buf: &mut [u8], pos: usize, who: Combatant) -> usize {
    match who {
        Combatant::Player => write_str(buf, pos, "You"),
        _ => write_str(buf, pos, who.name()),
    }
}

/// Format a GameEvent into a fixed-size ASCII buffer.
/// Returns the number of bytes written.
pub fn format_event(event: GameEvent, buf: &mut [u8; 30]) -> usize {
    buf.fill(b' ');
    let len = format_event_inner(event, buf);
    len.min(30)
}

fn format_event_inner(event: GameEvent, buf: &mut [u8; 30]) -> usize {
    match event {
        GameEvent::Welcome => write_str(buf, 0, "Welcome to the dungeon!"),

        GameEvent::Attack {
            attacker,
            defender,
            damage,
        } => {
            let mut p = write_combatant(buf, 0, attacker);
            p = write_str(buf, p, " hit ");
            p = write_combatant(buf, p, defender);
            p = write_str(buf, p, " for ");
            p = write_u16(buf, p, damage as u16);
            p
        }

        GameEvent::NoDamage {
            attacker,
            defender,
        } => {
            let mut p = write_combatant(buf, 0, attacker);
            p = write_str(buf, p, " miss ");
            p = write_combatant(buf, p, defender);
            p
        }

        GameEvent::Kill { attacker, victim } => {
            let mut p = write_combatant(buf, 0, attacker);
            p = write_str(buf, p, " killed ");
            p = write_combatant(buf, p, victim);
            p
        }

        GameEvent::PlayerDeath => write_str(buf, 0, "You have been slain..."),

        GameEvent::Descend { depth, .. } => {
            let mut p = write_str(buf, 0, "Descended to depth ");
            p = write_u16(buf, p, depth as u16);
            p
        }

        GameEvent::Victory { .. } => write_str(buf, 0, "You escaped the dungeon!"),

        GameEvent::PickupItem { kind } => {
            let mut p = write_str(buf, 0, "Picked up ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::DropItem { kind } => {
            let mut p = write_str(buf, 0, "Dropped ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::DrinkPotion { healed, .. } => {
            let mut p = write_str(buf, 0, "Healed ");
            p = write_u16(buf, p, healed as u16);
            p = write_str(buf, p, " HP");
            p
        }

        GameEvent::EquipWeapon { kind, .. } => {
            let mut p = write_str(buf, 0, "Equipped ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::EquipArmor { kind, .. } => {
            let mut p = write_str(buf, 0, "Equipped ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::NoStairs => write_str(buf, 0, "No stairs here."),

        GameEvent::InventoryFull => write_str(buf, 0, "Inventory full!"),

        GameEvent::ItemsHere { kind, count } => {
            if count == 1 {
                let mut p = write_str(buf, 0, "You see ");
                p = write_str(buf, p, roguelike_core::rules::items::name(kind));
                p
            } else {
                let mut p = write_str(buf, 0, "Items here (");
                p = write_u16(buf, p, count as u16);
                p = write_str(buf, p, ")");
                p
            }
        }

        GameEvent::EntityNotice { who } => {
            let mut p = write_combatant(buf, 0, who);
            p = write_str(buf, p, " notices you!");
            p
        }

        GameEvent::AutorunStop { .. } => write_str(buf, 0, "Stopped."),
        GameEvent::Autorun => write_str(buf, 0, "Running..."),

        GameEvent::SoundCue { .. } => write_str(buf, 0, "You hear something..."),

        GameEvent::HealthStatus { who, tier } => {
            let mut p = write_combatant(buf, 0, who);
            p = write_str(buf, p, match tier {
                HealthTier::Healthy => " looks healthy",
                HealthTier::Moderate => " looks damaged",
                HealthTier::Severe => " looks wounded",
                HealthTier::AlmostDead => " is dying",
            });
            p
        }

        GameEvent::UnequipWeapon { kind } => {
            let mut p = write_str(buf, 0, "Unequipped ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::UnequipArmor { kind } => {
            let mut p = write_str(buf, 0, "Unequipped ");
            p = write_str(buf, p, roguelike_core::rules::items::name(kind));
            p
        }

        GameEvent::UseStrengthPotion { bonus } => {
            let mut p = write_str(buf, 0, "ATK +");
            p = write_u16(buf, p, bonus as u16);
            p = write_str(buf, p, "!");
            p
        }

        GameEvent::CombineItems { target, .. } => {
            let mut p = write_str(buf, 0, "Combined ");
            p = write_str(buf, p, roguelike_core::rules::items::name(target));
            p
        }

        GameEvent::CombineNoEffect => write_str(buf, 0, "No effect."),

        GameEvent::ItemDestroyed { kind } => {
            let mut p = write_str(buf, 0, roguelike_core::rules::items::name(kind));
            p = write_str(buf, p, " destroyed!");
            p
        }
    }
}
