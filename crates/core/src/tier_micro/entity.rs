//! Fixed-size parallel array entity storage for the micro tier.
//!
//! Player is always slot 0. Monsters occupy slots 1..count.
//! All fields are `pub` for direct access from combat, AI, and rendering.

use super::types::*;
use crate::rules::balance;
use crate::rules::monster_table::{self, AiBehavior, MonsterKind};

pub struct EntityStore {
    pub x: [u8; MAX_ENTITIES],
    pub y: [u8; MAX_ENTITIES],
    pub hp: [Stat; MAX_ENTITIES],
    pub max_hp: [Stat; MAX_ENTITIES],
    pub atk: [Stat; MAX_ENTITIES],
    pub def: [Stat; MAX_ENTITIES],
    pub kind: [Option<MonsterKind>; MAX_ENTITIES],
    pub ai: [AiBehavior; MAX_ENTITIES],
    pub alive: [bool; MAX_ENTITIES],
    pub sight: [u8; MAX_ENTITIES],
    pub count: u8,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityStore {
    pub fn new() -> Self {
        Self {
            x: [0; MAX_ENTITIES],
            y: [0; MAX_ENTITIES],
            hp: [0; MAX_ENTITIES],
            max_hp: [0; MAX_ENTITIES],
            atk: [0; MAX_ENTITIES],
            def: [0; MAX_ENTITIES],
            kind: [None; MAX_ENTITIES],
            ai: [AiBehavior::None; MAX_ENTITIES],
            alive: [false; MAX_ENTITIES],
            sight: [0; MAX_ENTITIES],
            count: 0,
        }
    }

    pub fn spawn_player(&mut self, px: u8, py: u8) {
        let i = PLAYER_IDX as usize;
        self.x[i] = px;
        self.y[i] = py;
        self.kind[i] = None; // player has no MonsterKind
        self.hp[i] = balance::PLAYER_HP;
        self.max_hp[i] = balance::PLAYER_HP;
        self.atk[i] = balance::PLAYER_ATK;
        self.def[i] = balance::PLAYER_DEF;
        self.ai[i] = AiBehavior::None;
        self.alive[i] = true;
        self.sight[i] = balance::MICRO_FOV_RADIUS;
        if self.count == 0 {
            self.count = 1;
        }
    }

    /// Spawn a monster. Returns false if entity slots are full.
    pub fn spawn_monster(
        &mut self,
        kind: MonsterKind,
        mx: u8,
        my: u8,
        behavior: AiBehavior,
    ) -> bool {
        if self.count as usize >= MAX_ENTITIES {
            return false;
        }
        let i = self.count as usize;
        self.x[i] = mx;
        self.y[i] = my;
        self.kind[i] = Some(kind);
        self.hp[i] = monster_table::max_hp(kind);
        self.max_hp[i] = monster_table::max_hp(kind);
        self.atk[i] = monster_table::attack(kind);
        self.def[i] = monster_table::defense(kind);
        self.ai[i] = behavior;
        self.alive[i] = true;
        self.sight[i] = monster_table::sight_radius(kind);
        self.count += 1;
        true
    }

    /// Find any alive entity at position. Returns slot index or NO_ENTITY.
    pub fn entity_at(&self, ex: u8, ey: u8) -> u8 {
        for i in 0..self.count {
            let idx = i as usize;
            if self.alive[idx] && self.x[idx] == ex && self.y[idx] == ey {
                return i;
            }
        }
        NO_ENTITY
    }

    /// Find alive monster (non-player) at position.
    pub fn monster_at(&self, ex: u8, ey: u8) -> u8 {
        for i in 1..self.count {
            let idx = i as usize;
            if self.alive[idx] && self.x[idx] == ex && self.y[idx] == ey {
                return i;
            }
        }
        NO_ENTITY
    }

    /// Check if position is occupied by any alive entity (excluding skip_idx).
    pub fn is_occupied(&self, ex: u8, ey: u8, skip_idx: u8) -> bool {
        for i in 0..self.count {
            if i == skip_idx {
                continue;
            }
            let idx = i as usize;
            if self.alive[idx] && self.x[idx] == ex && self.y[idx] == ey {
                return true;
            }
        }
        false
    }

    pub fn kill(&mut self, i: u8) {
        self.alive[i as usize] = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_player_slot_zero() {
        let mut e = EntityStore::new();
        e.spawn_player(10, 20);
        assert_eq!(e.x[0], 10);
        assert_eq!(e.y[0], 20);
        assert!(e.alive[0]);
        assert_eq!(e.kind[0], None);
        assert_eq!(e.count, 1);
    }

    #[test]
    fn spawn_monster_increments_count() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        assert!(e.spawn_monster(MonsterKind::Goblin, 10, 10, AiBehavior::Wander));
        assert_eq!(e.count, 2);
        assert_eq!(e.kind[1], Some(MonsterKind::Goblin));
    }

    #[test]
    fn spawn_beyond_max_returns_false() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        for i in 1..MAX_ENTITIES {
            assert!(e.spawn_monster(MonsterKind::Goblin, i as u8, 0, AiBehavior::Wander,));
        }
        assert!(!e.spawn_monster(MonsterKind::Goblin, 20, 20, AiBehavior::Wander));
    }

    #[test]
    fn entity_at_finds_alive() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Orc, 10, 10, AiBehavior::Chase);
        assert_eq!(e.entity_at(5, 5), 0);
        assert_eq!(e.entity_at(10, 10), 1);
        assert_eq!(e.entity_at(99, 99), NO_ENTITY);
    }

    #[test]
    fn monster_at_skips_player() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        assert_eq!(e.monster_at(5, 5), NO_ENTITY);
    }

    #[test]
    fn kill_marks_dead() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Goblin, 10, 10, AiBehavior::Chase);
        e.kill(1);
        assert!(!e.alive[1]);
        assert_eq!(e.entity_at(10, 10), NO_ENTITY);
    }

    #[test]
    fn is_occupied_respects_skip() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Goblin, 10, 10, AiBehavior::Chase);
        assert!(e.is_occupied(10, 10, 0));
        assert!(!e.is_occupied(10, 10, 1));
    }

    #[test]
    fn stats_from_rules() {
        let mut e = EntityStore::new();
        e.spawn_player(0, 0);
        e.spawn_monster(MonsterKind::Troll, 1, 1, AiBehavior::Chase);
        assert_eq!(e.hp[1], monster_table::max_hp(MonsterKind::Troll));
        assert_eq!(e.atk[1], monster_table::attack(MonsterKind::Troll));
        assert_eq!(e.def[1], monster_table::defense(MonsterKind::Troll));
        assert_eq!(e.sight[1], monster_table::sight_radius(MonsterKind::Troll));
    }
}
