//! Top-level compact-tier game state and step API (GBA).
//!
//! `CompactGameState` owns all game data — map, entities, FOV, messages, RNG.
//! The `step()` method processes one player command and runs a full game tick.
//!
//! This module is under construction — see Phase 0 plan.

use super::entity::EntityStore;
use super::fov::CompactFov;
use super::item_store::ItemStore;
use super::map::CompactMap;
use super::msglog::CompactMessageLog;
use super::prng::LfsrRng32;
use super::spawn;
use super::types::*;
use crate::command::GameCommand;
use crate::rules::balance;
use crate::rules::damage;
use crate::rules::items::{Equipment, Inventory};
use crate::rules::message::GameEvent;

/// Result of a single step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactStepResult {
    pub action_taken: bool,
    pub game_over: bool,
    pub game_won: bool,
}

#[allow(dead_code)] // Fields used once step_inner is fully implemented.
pub struct CompactGameState {
    pub map: CompactMap,
    pub fov: CompactFov,
    pub entities: EntityStore,
    pub items: ItemStore,
    pub equipment: Equipment,
    pub inventory: Inventory,
    pub log: CompactMessageLog,
    pub rng: LfsrRng32,
    pub seed: u32,
    pub turn_count: u16,
    pub kills: u8,
    pub depth: u8,
    pub game_over: bool,
    pub game_won: bool,
    pub idle_count: u8,
    pub wandering_spawned: u8,
    pub(crate) regen_counter: u8,
    pub(crate) wandering_counter: u8,
    pub auto_pickup: bool,
    pub(crate) ambient_sound_counter: u8,
}

impl CompactGameState {
    /// Create a new game with the given seed.
    pub fn new(seed: u32) -> Self {
        let mut rng = LfsrRng32::new(seed);
        let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
        let (sx, sy) = map.generate(&mut rng);

        let mut entities = EntityStore::new();
        entities.spawn_player(sx, sy);

        let mut items = ItemStore::new();
        spawn::spawn_monsters(&mut entities, &map, &mut rng);
        spawn::spawn_items(&mut items, &map, 1, &mut rng);
        spawn::apply_depth_scaling(&mut entities, 1);

        let mut fov = CompactFov::new(MAP_WIDTH, MAP_HEIGHT);
        fov.compute_fov(sx, sy, balance::FOV_RADIUS, &map);

        let mut log = CompactMessageLog::new();
        log.add(GameEvent::Welcome);

        Self {
            map,
            fov,
            entities,
            items,
            equipment: Equipment::default(),
            inventory: Inventory::new(),
            log,
            rng,
            seed,
            turn_count: 0,
            kills: 0,
            depth: 1,
            game_over: false,
            game_won: false,
            idle_count: 0,
            wandering_spawned: 0,
            regen_counter: balance::REGEN_INTERVAL,
            wandering_counter: balance::WANDERING_GRACE_PERIOD - 1,
            auto_pickup: false,
            ambient_sound_counter: balance::WANDERING_AMBIENT_SOUND_INTERVAL - 1,
        }
    }

    /// Effective attack: base + weapon bonus.
    pub fn effective_attack(&self) -> u8 {
        let base = self.entities.atk[PLAYER_IDX as usize];
        damage::effective_attack(base, self.equipment.attack_bonus())
    }

    /// Effective defense: base + armor bonus.
    pub fn effective_defense(&self) -> u8 {
        let base = self.entities.def[PLAYER_IDX as usize];
        damage::effective_defense(base, self.equipment.defense_bonus())
    }

    /// Process one player command. Returns step result.
    pub fn step(&mut self, cmd: GameCommand) -> CompactStepResult {
        self.step_inner(cmd, true)
    }

    /// Process one command without recomputing FOV (for autorun intermediate steps).
    pub fn step_skip_fov(&mut self, cmd: GameCommand) -> CompactStepResult {
        self.step_inner(cmd, false)
    }

    fn step_inner(&mut self, _cmd: GameCommand, _compute_fov: bool) -> CompactStepResult {
        // TODO: Full command dispatch — will be implemented in the game.rs commit.
        CompactStepResult {
            action_taken: false,
            game_over: false,
            game_won: false,
        }
    }
}
