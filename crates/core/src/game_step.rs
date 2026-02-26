//! Cross-tier game trait — uniform interface for any capability tier.
//!
//! `GameStep` lets consumers (MCP server, FrameSink, TUI) drive a game
//! without knowing which tier produced it. Standard-tier `GameState`
//! implements directly; lower tiers use adapter wrappers that widen
//! coordinates and translate result types.

use std::any::Any;

use crate::command::{Direction, GameCommand};
use crate::data::GameData;
use crate::game::{
    AutorunResult, AutorunStopReason, EntityInfo, GameObservation, GameState, StepOutcome,
    StepResult, TileInfo,
};
use crate::map::MapPreset;
use crate::message_log::format_event;
use crate::rules::{balance, monster_table};
use crate::seed_code::{self, SeedParams};
use crate::tier_micro::fov::MicroFov;
use crate::tier_micro::game::MicroGameState;
use crate::tier_micro::map::{TILE_FLOOR, TILE_WALL};
use crate::tier_micro::types::{MAP_HEIGHT, MAP_WIDTH, PLAYER_IDX};
use crate::types::Stat;

/// Uniform interface for driving a game of any capability tier.
///
/// All coordinates use `i32` (widened from `u8`/`i16` for lower tiers).
/// Return types are the standard-tier structs from `game.rs`, which
/// adapters populate from their tier's internal representation.
pub trait GameStep: Send {
    /// Execute one player command + monster turns.
    fn step(&mut self, cmd: GameCommand) -> StepResult;

    /// Produce a snapshot of the visible game state.
    fn observe(&self) -> GameObservation;

    /// Query tile information at (x, y) for look mode.
    fn look_at(&self, x: i32, y: i32) -> TileInfo;

    /// Player position as (x, y).
    fn player_pos(&self) -> (i32, i32);

    /// Player HP as (current, max).
    fn player_hp(&self) -> (i32, i32);

    /// Whether the game has ended (player died or won).
    fn is_game_over(&self) -> bool;

    /// Current turn count.
    fn turn_count(&self) -> u32;

    /// Downcast to `&dyn Any` for tier-specific operations.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to `&mut dyn Any` for tier-specific operations.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ── Standard tier ────────────────────────────────────────────────────

impl GameStep for GameState {
    fn step(&mut self, cmd: GameCommand) -> StepResult {
        self.step(cmd)
    }

    fn observe(&self) -> GameObservation {
        self.observe()
    }

    fn look_at(&self, x: i32, y: i32) -> TileInfo {
        self.look_at(x, y)
    }

    fn player_pos(&self) -> (i32, i32) {
        let p = &self.entities[0];
        (p.x, p.y)
    }

    fn player_hp(&self) -> (i32, i32) {
        let p = &self.entities[0];
        (p.hp, p.max_hp)
    }

    fn is_game_over(&self) -> bool {
        self.game_over || self.game_won
    }

    fn turn_count(&self) -> u32 {
        self.turn_count as u32
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Factory functions ────────────────────────────────────────────────

/// Create a game for the given seed and parameters.
///
/// Routes micro-tier seeds (`<= 0xFFFF`) to [`MicroGameStateAdapter`],
/// standard-tier seeds to [`GameState`].  Calls `update_fov()` for
/// standard tier.
pub fn create_game(
    seed: u64,
    width: i32,
    height: i32,
    preset: Option<MapPreset>,
    game_data: &GameData,
) -> Result<Box<dyn GameStep>, String> {
    match seed_code::tier_from_seed(seed) {
        seed_code::Tier::Micro => Ok(Box::new(MicroGameStateAdapter::new(seed as u16))),
        _ => {
            if width < balance::MIN_MAP_WIDTH as i32 || height < balance::MIN_MAP_HEIGHT as i32 {
                return Err(format!(
                    "Map must be at least {}x{} tiles",
                    balance::MIN_MAP_WIDTH,
                    balance::MIN_MAP_HEIGHT
                ));
            }
            let mut state = if let Some(p) = preset {
                GameState::with_preset_data(width, height, seed, p, game_data)
            } else {
                GameState::with_data(width, height, seed, game_data)
            };
            state.update_fov();
            Ok(Box::new(state))
        }
    }
}

/// Create a standard-tier game with a random seed.
///
/// Random seeds are always standard tier (u64 range).
pub fn create_random_game(
    width: i32,
    height: i32,
    game_data: &GameData,
) -> Result<Box<dyn GameStep>, String> {
    if width < balance::MIN_MAP_WIDTH as i32 || height < balance::MIN_MAP_HEIGHT as i32 {
        return Err(format!(
            "Map must be at least {}x{} tiles",
            balance::MIN_MAP_WIDTH,
            balance::MIN_MAP_HEIGHT
        ));
    }
    let mut state = GameState::new_with_data(width, height, game_data);
    state.update_fov();
    Ok(Box::new(state))
}

// ── Micro tier adapter ───────────────────────────────────────────────

/// Adapter wrapping `MicroGameState` to implement `GameStep`.
///
/// Widens `u8` coordinates/stats to `i32` and builds std-tier
/// observation types from the micro tier's fixed-size arrays.
pub struct MicroGameStateAdapter {
    pub game: MicroGameState,
    seed: u16,
}

impl MicroGameStateAdapter {
    pub fn new(seed: u16) -> Self {
        Self {
            game: MicroGameState::new(seed),
            seed,
        }
    }

    /// Create a stepper for directional autorun (micro tier).
    pub fn start_autorun(&self, dir: Direction) -> MicroAutorunStepper {
        MicroAutorunStepper {
            dir,
            steps_taken: 0,
            max_steps: balance::MAX_AUTORUN_STEPS as Stat,
            all_messages: Vec::new(),
            visible_before: [false; balance::MICRO_MAX_ENTITIES as usize],
            explored_floor_before: self.game.fov.explored_floor_count(&self.game.map) as Stat,
        }
    }

    /// Run in a direction until something interesting happens.
    pub fn autorun(&mut self, dir: Direction) -> AutorunResult {
        let stepper = self.start_autorun(dir);
        stepper.run_to_completion(self)
    }
}

impl GameStep for MicroGameStateAdapter {
    fn step(&mut self, cmd: GameCommand) -> StepResult {
        let msg_count_before = self.game.log.total();
        let result = self.game.step(cmd);

        // Collect new messages by diffing the wrapping total counter.
        // Cap to 8 (circular buffer capacity) to avoid stale re-reads.
        let new_count = self.game.log.total();
        let added = new_count.wrapping_sub(msg_count_before).min(8) as u8;
        let new_messages: Vec<String> = (0..added)
            .filter_map(|i| self.game.log.recent(added - 1 - i).map(format_event))
            .collect();

        StepResult {
            action_taken: result.action_taken,
            new_messages,
            game_over: result.game_over,
            game_won: false,
        }
    }

    fn observe(&self) -> GameObservation {
        let pi = PLAYER_IDX as usize;
        let entities = &self.game.entities;
        let map = &self.game.map;
        let fov = &self.game.fov;

        // Build ASCII map — only rows with visible content.
        let map_ascii = build_micro_map_ascii(map, fov, entities);

        // Visible entities (excluding player).
        let visible_entities = build_micro_visible_entities(entities, fov);

        // Recent messages.
        let recent_messages = build_micro_recent_messages(&self.game.log);

        // Explored percentage.
        let total_floor = map.floor_count();
        let explored_floor = fov.explored_floor_count(map);
        let explored_pct = if total_floor > 0 {
            ((explored_floor as i32) * 100) / (total_floor as i32)
        } else {
            0
        };

        // Seed code.
        let seed_code = seed_code::encode(&SeedParams {
            seed: self.seed as u64,
            width: MAP_WIDTH as i32,
            height: MAP_HEIGHT as i32,
            preset: None,
        });

        GameObservation {
            player_hp: entities.hp[pi] as i32,
            player_max_hp: entities.max_hp[pi] as i32,
            player_atk: entities.atk[pi] as i32,
            player_def: entities.def[pi] as i32,
            player_x: entities.x[pi] as i32,
            player_y: entities.y[pi] as i32,
            map_ascii,
            visible_entities,
            visible_items: Vec::new(),
            recent_messages,
            game_over: self.game.game_over,
            turn_count: self.game.turn_count as i32,
            weapon: None,
            armor: None,
            kills: self.game.kills as i32,
            rooms_found: map.room_count as i32,
            explored_pct,
            seed: self.seed as u64,
            seed_code,
            depth: 1,
            target_depth: 1,
            game_won: false,
        }
    }

    fn look_at(&self, x: i32, y: i32) -> TileInfo {
        // Out of bounds or negative → unknown.
        if x < 0 || y < 0 || x >= MAP_WIDTH as i32 || y >= MAP_HEIGHT as i32 {
            return TileInfo {
                x,
                y,
                terrain: "Out of bounds".into(),
                entity: None,
                items: Vec::new(),
                visible: false,
                explored: false,
                glyph: ' ',
            };
        }

        let ux = x as u8;
        let uy = y as u8;
        let fov = &self.game.fov;
        let map = &self.game.map;
        let entities = &self.game.entities;

        let explored = fov.is_explored(ux, uy);
        if !explored {
            return TileInfo {
                x,
                y,
                terrain: "Unknown".into(),
                entity: None,
                items: Vec::new(),
                visible: false,
                explored: false,
                glyph: ' ',
            };
        }

        let visible = fov.is_visible(ux, uy);
        let tile = map.tile_at(ux, uy);
        let terrain = match tile {
            TILE_FLOOR => "Floor".into(),
            TILE_WALL => "Wall".into(),
            _ => "Unknown".into(),
        };

        let entity = if visible {
            entity_info_at(entities, ux, uy)
        } else {
            None
        };

        let glyph = if visible {
            if let Some(ref ei) = entity {
                ei.glyph
            } else {
                tile_glyph(tile)
            }
        } else {
            tile_glyph(tile)
        };

        TileInfo {
            x,
            y,
            terrain,
            entity,
            items: Vec::new(),
            visible,
            explored,
            glyph,
        }
    }

    fn player_pos(&self) -> (i32, i32) {
        let pi = PLAYER_IDX as usize;
        (
            self.game.entities.x[pi] as i32,
            self.game.entities.y[pi] as i32,
        )
    }

    fn player_hp(&self) -> (i32, i32) {
        let pi = PLAYER_IDX as usize;
        (
            self.game.entities.hp[pi] as i32,
            self.game.entities.max_hp[pi] as i32,
        )
    }

    fn is_game_over(&self) -> bool {
        self.game.game_over
    }

    fn turn_count(&self) -> u32 {
        self.game.turn_count as u32
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ── Micro tier autorun ───────────────────────────────────────────────

/// Autorun stepper for the micro tier — directional movement only.
///
/// Mirrors the standard tier's `AutorunStepper` stop conditions but operates
/// on `MicroGameStateAdapter` with `u8` coordinates and fixed arrays.
pub struct MicroAutorunStepper {
    dir: Direction,
    steps_taken: Stat,
    max_steps: Stat,
    all_messages: Vec<String>,
    /// Tracks which entity slots were visible before each step.
    visible_before: [bool; balance::MICRO_MAX_ENTITIES as usize],
    explored_floor_before: Stat,
}

impl MicroAutorunStepper {
    /// Execute one step of the autorun sequence.
    pub fn next_step(&mut self, adapter: &mut MicroGameStateAdapter) -> StepOutcome {
        let pi = PLAYER_IDX as usize;

        // Check 1: max steps.
        if self.steps_taken >= self.max_steps {
            return self.finish(adapter, AutorunStopReason::MaxSteps);
        }

        // Check 2: adjacent monster before stepping.
        if has_adjacent_monster_micro(&adapter.game.entities) {
            return self.finish(adapter, AutorunStopReason::MonsterSpotted);
        }

        // Snapshot HP and visible monsters before step.
        let hp_before = adapter.game.entities.hp[pi];
        self.snapshot_visible_monsters(&adapter.game.entities, &adapter.game.fov);

        // Execute step via the adapter (handles message collection).
        let result = adapter.step(GameCommand::Move(self.dir));
        self.all_messages.extend(result.new_messages);

        // Check 3: wall hit.
        if !result.action_taken {
            return self.finish(adapter, AutorunStopReason::WallReached);
        }

        self.steps_taken += 1;

        // Check 4: game over.
        if result.game_over {
            return self.finish(adapter, AutorunStopReason::GameOver);
        }

        // Check 5: damage taken.
        if adapter.game.entities.hp[pi] < hp_before {
            return self.finish(adapter, AutorunStopReason::DamageTaken);
        }

        // Check 6: new monster spotted.
        if self.has_new_visible_monster(&adapter.game.entities, &adapter.game.fov) {
            return self.finish(adapter, AutorunStopReason::MonsterSpotted);
        }

        // Check 7: corridor branches.
        let (dx, dy) = self.dir.to_offset();
        let px = adapter.game.entities.x[pi];
        let py = adapter.game.entities.y[pi];
        let ahead_x = (px as i8 + dx as i8) as u8;
        let ahead_y = (py as i8 + dy as i8) as u8;
        if !adapter.game.map.is_walkable(ahead_x, ahead_y) {
            let alternatives =
                adapter
                    .game
                    .map
                    .open_neighbors_excluding(px, py, -(dx as i8), -(dy as i8));
            if alternatives >= 2 {
                return self.finish(adapter, AutorunStopReason::CorridorBranches);
            }
            return self.finish(adapter, AutorunStopReason::WallReached);
        }

        StepOutcome::Continue
    }

    /// Run all remaining steps without pausing.
    pub fn run_to_completion(mut self, adapter: &mut MicroGameStateAdapter) -> AutorunResult {
        loop {
            match self.next_step(adapter) {
                StepOutcome::Continue => continue,
                StepOutcome::Done(result) => return result,
            }
        }
    }

    fn snapshot_visible_monsters(
        &mut self,
        entities: &crate::tier_micro::entity::EntityStore,
        fov: &MicroFov,
    ) {
        for i in 1..entities.count as usize {
            self.visible_before[i] =
                entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]);
        }
    }

    fn has_new_visible_monster(
        &self,
        entities: &crate::tier_micro::entity::EntityStore,
        fov: &MicroFov,
    ) -> bool {
        for i in 1..entities.count as usize {
            if entities.alive[i]
                && fov.is_visible(entities.x[i], entities.y[i])
                && !self.visible_before[i]
            {
                return true;
            }
        }
        false
    }

    fn finish(
        &mut self,
        adapter: &MicroGameStateAdapter,
        reason: AutorunStopReason,
    ) -> StepOutcome {
        let explored_floor_after = adapter.game.fov.explored_floor_count(&adapter.game.map) as Stat;
        StepOutcome::Done(AutorunResult {
            steps_taken: self.steps_taken,
            stop_reason: reason,
            messages: std::mem::take(&mut self.all_messages),
            new_tiles_revealed: explored_floor_after - self.explored_floor_before,
        })
    }
}

/// Check if any alive monster is adjacent (Chebyshev distance <= 1) to the player.
fn has_adjacent_monster_micro(entities: &crate::tier_micro::entity::EntityStore) -> bool {
    let pi = PLAYER_IDX as usize;
    let px = entities.x[pi];
    let py = entities.y[pi];
    for i in 1..entities.count as usize {
        if entities.alive[i] {
            let dx = entities.x[i].abs_diff(px);
            let dy = entities.y[i].abs_diff(py);
            if dx <= 1 && dy <= 1 {
                return true;
            }
        }
    }
    false
}

// ── Micro adapter helpers ────────────────────────────────────────────

fn tile_glyph(tile: u8) -> char {
    match tile {
        TILE_FLOOR => '.',
        TILE_WALL => '#',
        _ => ' ',
    }
}

fn entity_info_at(
    entities: &crate::tier_micro::entity::EntityStore,
    x: u8,
    y: u8,
) -> Option<EntityInfo> {
    for i in 0..entities.count as usize {
        if entities.alive[i] && entities.x[i] == x && entities.y[i] == y {
            let (name, glyph) = if let Some(kind) = entities.kind[i] {
                (
                    monster_table::name(kind).to_string(),
                    monster_table::glyph(kind),
                )
            } else {
                ("Player".to_string(), '@')
            };
            return Some(EntityInfo {
                name,
                glyph,
                x: x as i32,
                y: y as i32,
                hp: entities.hp[i] as i32,
                max_hp: entities.max_hp[i] as i32,
                alive: true,
            });
        }
    }
    None
}

fn build_micro_map_ascii(
    map: &crate::tier_micro::map::MicroMap,
    fov: &MicroFov,
    entities: &crate::tier_micro::entity::EntityStore,
) -> Vec<String> {
    let mut lines = Vec::new();
    for y in 0..MAP_HEIGHT {
        let mut line = String::with_capacity(MAP_WIDTH as usize);
        let mut has_content = false;

        for x in 0..MAP_WIDTH {
            if fov.is_visible(x, y) {
                has_content = true;
                // Check for alive entity at this position.
                if let Some(ei) = entity_info_at(entities, x, y) {
                    line.push(ei.glyph);
                } else {
                    line.push(tile_glyph(map.tile_at(x, y)));
                }
            } else {
                line.push(' ');
            }
        }

        if has_content {
            lines.push(line.trim_end().to_string());
        }
    }
    lines
}

fn build_micro_visible_entities(
    entities: &crate::tier_micro::entity::EntityStore,
    fov: &MicroFov,
) -> Vec<EntityInfo> {
    let mut result = Vec::new();
    // Skip player (index 0).
    for i in 1..entities.count as usize {
        if entities.alive[i] && fov.is_visible(entities.x[i], entities.y[i]) {
            let kind = entities.kind[i];
            let (name, glyph) = if let Some(k) = kind {
                (monster_table::name(k).to_string(), monster_table::glyph(k))
            } else {
                ("Unknown".to_string(), '?')
            };
            result.push(EntityInfo {
                name,
                glyph,
                x: entities.x[i] as i32,
                y: entities.y[i] as i32,
                hp: entities.hp[i] as i32,
                max_hp: entities.max_hp[i] as i32,
                alive: true,
            });
        }
    }
    result
}

fn build_micro_recent_messages(log: &crate::tier_micro::msglog::MicroMessageLog) -> Vec<String> {
    let mut messages = Vec::new();
    // Collect up to 8 recent messages (oldest first).
    for i in (0..8u8).rev() {
        if let Some(event) = log.recent(i) {
            messages.push(format_event(event));
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    fn test_standard_game() -> GameState {
        let gd = data::load_game_data();
        let mut state = GameState::with_data(40, 30, 42, &gd);
        state.update_fov();
        state
    }

    #[test]
    fn trait_object_dispatch_standard() {
        let mut state = test_standard_game();
        let game: &mut dyn GameStep = &mut state;

        let (px, py) = game.player_pos();
        assert!(px >= 0 && py >= 0);

        let (hp, max_hp) = game.player_hp();
        assert!(hp > 0 && max_hp > 0);
        assert!(hp <= max_hp);

        assert!(!game.is_game_over());
        assert_eq!(game.turn_count(), 0);

        let obs = game.observe();
        assert!(!obs.map_ascii.is_empty());
        assert_eq!(obs.player_x, px);
        assert_eq!(obs.player_y, py);

        let tile = game.look_at(px, py);
        assert!(tile.visible);
        assert!(tile.explored);

        let result = game.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert_eq!(game.turn_count(), 1);
    }

    #[test]
    fn trait_object_dispatch_micro() {
        let mut adapter = MicroGameStateAdapter::new(42);
        let game: &mut dyn GameStep = &mut adapter;

        let (px, py) = game.player_pos();
        assert!(px >= 0 && py >= 0);

        let (hp, max_hp) = game.player_hp();
        assert!(hp > 0 && max_hp > 0);
        assert!(hp <= max_hp);

        assert!(!game.is_game_over());
        assert_eq!(game.turn_count(), 0);

        let obs = game.observe();
        assert!(!obs.map_ascii.is_empty());
        assert_eq!(obs.player_x, px);
        assert_eq!(obs.player_y, py);

        let tile = game.look_at(px, py);
        assert!(tile.visible);
        assert!(tile.explored);

        let result = game.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert_eq!(game.turn_count(), 1);
    }

    #[test]
    fn micro_adapter_observe_has_map() {
        let adapter = MicroGameStateAdapter::new(42);
        let obs = adapter.observe();
        assert!(
            !obs.map_ascii.is_empty(),
            "micro observe should produce map lines"
        );
        // At least some lines should have non-space content.
        let has_content = obs
            .map_ascii
            .iter()
            .any(|l| l.contains('.') || l.contains('#'));
        assert!(has_content, "map should contain floor or wall tiles");
    }

    #[test]
    fn micro_adapter_observe_has_entities() {
        let adapter = MicroGameStateAdapter::new(42);
        let obs = adapter.observe();
        // The map should show the player '@' somewhere.
        let has_player = obs.map_ascii.iter().any(|l| l.contains('@'));
        assert!(has_player, "map should show player glyph");
    }

    #[test]
    fn micro_adapter_step_translates_result() {
        let mut adapter = MicroGameStateAdapter::new(42);
        let result = adapter.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert!(!result.game_won);
        assert!(!result.game_over);
    }

    #[test]
    fn micro_adapter_look_at_player_tile() {
        let adapter = MicroGameStateAdapter::new(42);
        let (px, py) = adapter.player_pos();
        let tile = adapter.look_at(px, py);
        assert!(tile.visible);
        assert!(tile.explored);
        assert!(tile.entity.is_some(), "player tile should have entity info");
        let entity = tile.entity.unwrap();
        assert_eq!(entity.name, "Player");
        assert_eq!(entity.glyph, '@');
    }

    #[test]
    fn micro_adapter_look_at_out_of_bounds() {
        let adapter = MicroGameStateAdapter::new(42);
        let tile = adapter.look_at(-1, -1);
        assert!(!tile.visible);
        assert!(!tile.explored);
        assert_eq!(tile.terrain, "Out of bounds");
    }

    #[test]
    fn micro_adapter_seed_code() {
        let adapter = MicroGameStateAdapter::new(42);
        let obs = adapter.observe();
        assert!(!obs.seed_code.is_empty());
        assert_eq!(obs.seed, 42);
    }

    #[test]
    fn both_tiers_same_trait_object() {
        let mut standard = test_standard_game();
        let mut micro = MicroGameStateAdapter::new(42);

        let games: Vec<&mut dyn GameStep> = vec![&mut standard, &mut micro];

        for game in games {
            let (px, py) = game.player_pos();
            assert!(px >= 0 && py >= 0);

            let (hp, max_hp) = game.player_hp();
            assert!(hp > 0 && max_hp > 0);

            assert!(!game.is_game_over());

            let result = game.step(GameCommand::Wait);
            assert!(result.action_taken);
            assert_eq!(game.turn_count(), 1);
        }
    }

    #[test]
    fn micro_adapter_messages_after_step() {
        let adapter = MicroGameStateAdapter::new(42);
        // Welcome message should have been added at construction.
        let obs = adapter.observe();
        assert!(
            obs.recent_messages.iter().any(|m| m.contains("Welcome")),
            "should contain welcome message"
        );
    }

    #[test]
    fn create_game_standard_tier() {
        let gd = data::load_game_data();
        let game = create_game(1_000_000, 40, 30, None, &gd).unwrap();
        assert!(!game.is_game_over());
        // Should downcast to GameState (standard tier).
        assert!(game.as_any().downcast_ref::<GameState>().is_some());
    }

    #[test]
    fn create_game_micro_tier() {
        let gd = data::load_game_data();
        let game = create_game(42, 40, 30, None, &gd).unwrap();
        assert!(!game.is_game_over());
        // Should downcast to MicroGameStateAdapter (micro tier).
        assert!(
            game.as_any()
                .downcast_ref::<MicroGameStateAdapter>()
                .is_some()
        );
    }

    #[test]
    fn create_random_game_is_standard() {
        let gd = data::load_game_data();
        let game = create_random_game(40, 30, &gd).unwrap();
        assert!(!game.is_game_over());
        assert!(game.as_any().downcast_ref::<GameState>().is_some());
    }

    #[test]
    fn create_game_rejects_small_dimensions() {
        let gd = data::load_game_data();
        assert!(create_game(1_000_000, 10, 10, None, &gd).is_err());
        assert!(create_game(1_000_000, 19, 30, None, &gd).is_err());
        assert!(create_game(1_000_000, 40, 14, None, &gd).is_err());
        // Micro tier ignores dimensions — fixed 64x48.
        assert!(create_game(42, 10, 10, None, &gd).is_ok());
    }

    #[test]
    fn create_random_game_rejects_small_dimensions() {
        let gd = data::load_game_data();
        assert!(create_random_game(10, 10, &gd).is_err());
        assert!(create_random_game(20, 15, &gd).is_ok());
    }

    #[test]
    fn micro_autorun_stops() {
        use crate::command::Direction;
        let mut adapter = MicroGameStateAdapter::new(42);
        let result = adapter.autorun(Direction::North);
        assert!(result.steps_taken >= 0);
        // Should have stopped for a valid reason.
        assert!(matches!(
            result.stop_reason,
            AutorunStopReason::WallReached
                | AutorunStopReason::MonsterSpotted
                | AutorunStopReason::CorridorBranches
                | AutorunStopReason::DamageTaken
                | AutorunStopReason::GameOver
                | AutorunStopReason::MaxSteps
        ));
    }

    #[test]
    fn micro_autorun_respects_max_steps() {
        use crate::command::Direction;
        let mut adapter = MicroGameStateAdapter::new(42);
        let result = adapter.autorun(Direction::East);
        assert!(result.steps_taken <= balance::MAX_AUTORUN_STEPS as Stat);
    }

    #[test]
    fn micro_autorun_stepper_step_by_step() {
        use crate::command::Direction;
        let mut adapter = MicroGameStateAdapter::new(42);
        let mut stepper = adapter.start_autorun(Direction::South);
        let mut continues = 0;
        loop {
            match stepper.next_step(&mut adapter) {
                StepOutcome::Continue => continues += 1,
                StepOutcome::Done(result) => {
                    // steps_taken includes the final step that triggered the
                    // stop, which returns Done rather than Continue.
                    assert!(result.steps_taken >= continues);
                    assert!(result.steps_taken <= continues + 1);
                    break;
                }
            }
        }
    }

    #[test]
    fn micro_autorun_collects_messages() {
        use crate::command::Direction;
        let mut adapter = MicroGameStateAdapter::new(42);
        let result = adapter.autorun(Direction::East);
        // Messages vec should be valid (may be empty if no combat).
        let _ = result.messages;
    }
}
