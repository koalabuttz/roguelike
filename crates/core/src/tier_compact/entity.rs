//! Entity storage for the compact tier (GBA).
//!
//! Parallel arrays (struct-of-arrays) indexed by entity slot. Player is
//! always slot 0. Stats are `u8`, positions are `Coord` (i32, ARM7-native).

use super::types::*;
use crate::rules::balance;
use crate::rules::monster_table::{self, AiBehavior, MonsterKind};

pub struct EntityStore {
    pub x: [Coord; MAX_ENTITIES],
    pub y: [Coord; MAX_ENTITIES],
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

    /// Initialize the player at slot 0.
    pub fn spawn_player(&mut self, px: Coord, py: Coord) {
        self.x[0] = px;
        self.y[0] = py;
        self.hp[0] = balance::PLAYER_HP;
        self.max_hp[0] = balance::PLAYER_HP;
        self.atk[0] = balance::PLAYER_ATK;
        self.def[0] = balance::PLAYER_DEF;
        self.kind[0] = None;
        self.ai[0] = AiBehavior::None;
        self.alive[0] = true;
        self.sight[0] = balance::FOV_RADIUS;
        if self.count == 0 {
            self.count = 1;
        }
    }

    /// Add a monster at the next available slot. Returns false if full.
    pub fn spawn_monster(
        &mut self,
        kind: MonsterKind,
        mx: Coord,
        my: Coord,
        behavior: AiBehavior,
    ) -> bool {
        if self.count as usize >= MAX_ENTITIES {
            return false;
        }
        let i = self.count as usize;
        self.x[i] = mx;
        self.y[i] = my;
        self.hp[i] = monster_table::max_hp(kind);
        self.max_hp[i] = monster_table::max_hp(kind);
        self.atk[i] = monster_table::attack(kind);
        self.def[i] = monster_table::defense(kind);
        self.kind[i] = Some(kind);
        self.ai[i] = behavior;
        self.alive[i] = true;
        self.sight[i] = monster_table::sight_radius(kind);
        self.count += 1;
        true
    }

    /// Find any alive entity at (ex, ey). Returns slot index or NO_ENTITY.
    pub fn entity_at(&self, ex: Coord, ey: Coord) -> u8 {
        for i in 0..self.count as usize {
            if self.alive[i] && self.x[i] == ex && self.y[i] == ey {
                return i as u8;
            }
        }
        NO_ENTITY
    }

    /// Find an alive monster (non-player) at (ex, ey). Returns slot index or NO_ENTITY.
    pub fn monster_at(&self, ex: Coord, ey: Coord) -> u8 {
        for i in 1..self.count as usize {
            if self.alive[i] && self.x[i] == ex && self.y[i] == ey {
                return i as u8;
            }
        }
        NO_ENTITY
    }

    /// Check if any alive entity except `skip_idx` occupies (ex, ey).
    pub fn is_occupied(&self, ex: Coord, ey: Coord, skip_idx: u8) -> bool {
        for i in 0..self.count as usize {
            if i as u8 != skip_idx && self.alive[i] && self.x[i] == ex && self.y[i] == ey {
                return true;
            }
        }
        false
    }

    /// Mark entity as dead. Does not decrement count — slot stays allocated.
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
        e.spawn_player(5, 10);
        assert_eq!(e.count, 1);
        assert!(e.alive[0]);
        assert_eq!(e.x[0], 5);
        assert_eq!(e.y[0], 10);
        assert_eq!(e.hp[0], balance::PLAYER_HP);
        assert_eq!(e.atk[0], balance::PLAYER_ATK);
        assert_eq!(e.def[0], balance::PLAYER_DEF);
        assert!(e.kind[0].is_none());
    }

    #[test]
    fn spawn_monster_increments_count() {
        let mut e = EntityStore::new();
        e.spawn_player(0, 0);
        assert!(e.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase));
        assert_eq!(e.count, 2);
        assert!(e.alive[1]);
        assert_eq!(e.x[1], 5);
        assert_eq!(e.y[1], 5);
        assert!(matches!(e.kind[1], Some(MonsterKind::Goblin)));
    }

    #[test]
    fn spawn_beyond_max_returns_false() {
        let mut e = EntityStore::new();
        e.spawn_player(0, 0);
        for i in 1..MAX_ENTITIES {
            assert!(e.spawn_monster(MonsterKind::Goblin, i as Coord, 0, AiBehavior::Chase));
        }
        assert_eq!(e.count as usize, MAX_ENTITIES);
        assert!(!e.spawn_monster(MonsterKind::Goblin, 99, 99, AiBehavior::Chase));
    }

    #[test]
    fn entity_at_finds_alive() {
        let mut e = EntityStore::new();
        e.spawn_player(3, 7);
        e.spawn_monster(MonsterKind::Orc, 10, 20, AiBehavior::Chase);
        assert_eq!(e.entity_at(3, 7), 0);
        assert_eq!(e.entity_at(10, 20), 1);
        assert_eq!(e.entity_at(99, 99), NO_ENTITY);
    }

    #[test]
    fn monster_at_skips_player() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        assert_eq!(e.monster_at(5, 5), NO_ENTITY);
        e.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase);
        assert_eq!(e.monster_at(5, 5), 1);
    }

    #[test]
    fn kill_marks_dead() {
        let mut e = EntityStore::new();
        e.spawn_player(0, 0);
        e.spawn_monster(MonsterKind::Goblin, 5, 5, AiBehavior::Chase);
        e.kill(1);
        assert!(!e.alive[1]);
        assert_eq!(e.entity_at(5, 5), NO_ENTITY);
    }

    #[test]
    fn is_occupied_respects_skip() {
        let mut e = EntityStore::new();
        e.spawn_player(5, 5);
        e.spawn_monster(MonsterKind::Goblin, 10, 10, AiBehavior::Chase);
        // Position occupied by monster, skip player
        assert!(e.is_occupied(10, 10, 0));
        // Position occupied by monster, skip self → not occupied
        assert!(!e.is_occupied(10, 10, 1));
        // Empty position
        assert!(!e.is_occupied(99, 99, 0));
    }

    #[test]
    fn stats_from_rules() {
        let mut e = EntityStore::new();
        e.spawn_player(0, 0);
        e.spawn_monster(MonsterKind::Troll, 5, 5, AiBehavior::Chase);
        assert_eq!(e.hp[1], monster_table::max_hp(MonsterKind::Troll));
        assert_eq!(e.atk[1], monster_table::attack(MonsterKind::Troll));
        assert_eq!(e.def[1], monster_table::defense(MonsterKind::Troll));
        assert_eq!(e.sight[1], monster_table::sight_radius(MonsterKind::Troll));
    }
}
