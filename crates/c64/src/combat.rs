// Combat system — direct port of the Rust combat.rs.
//
// damage = max(0, attacker.atk - defender.def)
//
// This is the simplest module to port: a single subtraction and clamp.
// On the 6502 it's literally SEC / SBC / BCS .positive / LDA #0.

use crate::entity;
use crate::msglog;

/// Execute a melee attack. Returns true if the defender was killed.
pub fn melee_attack(attacker: u8, defender: u8) -> bool {
    let atk = entity::atk(attacker);
    let def = entity::def(defender);

    let damage = if atk > def { atk - def } else { 0 };

    if damage > 0 {
        let new_hp = if entity::hp(defender) > damage {
            entity::hp(defender) - damage
        } else {
            0
        };
        entity::set_hp(defender, new_hp);

        msglog::add_hit_msg(attacker, defender, damage);

        if new_hp == 0 {
            entity::kill(defender);
            msglog::add_death_msg(defender);
            return true;
        }
    } else {
        msglog::add_miss_msg(attacker, defender);
    }

    false
}
