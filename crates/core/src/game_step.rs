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
    AutoExploreResult, AutoFightResult, AutorunResult, AutorunStopReason, EntityInfo,
    GameObservation, GameState, ItemInfo, StepOutcome, StepResult, TileInfo,
};
use crate::map::MapPreset;
use crate::message_log::format_event;
use crate::rules::color::GameColor;
use crate::rules::items as rules_items;
use crate::rules::{balance, monster_table};
use crate::seed_code::{self, SeedParams};
use crate::tier_micro::fov::MicroFov;
use crate::tier_micro::game::MicroGameState;
use crate::tier_micro::item_store::ItemStore;
use crate::tier_micro::map::{TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL, TILE_WALL};
use crate::tier_micro::types::PLAYER_IDX;
use crate::types::{Coord, Stat};

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
        seed_code::Tier::Micro => {
            // Validate dimensions for micro tier.
            let w = width as u8;
            let h = height as u8;
            if w < balance::MIN_MAP_WIDTH || h < balance::MIN_MAP_HEIGHT {
                return Err(format!(
                    "Map must be at least {}x{} tiles",
                    balance::MIN_MAP_WIDTH,
                    balance::MIN_MAP_HEIGHT
                ));
            }
            if w > balance::MICRO_MAX_MAP_WIDTH || h > balance::MICRO_MAX_MAP_HEIGHT {
                return Err(format!(
                    "Micro-tier map must be at most {}x{} tiles",
                    balance::MICRO_MAX_MAP_WIDTH,
                    balance::MICRO_MAX_MAP_HEIGHT
                ));
            }
            Ok(Box::new(MicroGameStateAdapter::new(seed as u16, w, h)))
        }
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
    pub fn new(seed: u16, width: u8, height: u8) -> Self {
        Self {
            game: MicroGameState::new(seed, width, height),
            seed,
        }
    }

    /// Create an adapter with C64 default dimensions (64×48).
    pub fn new_default(seed: u16) -> Self {
        Self {
            game: MicroGameState::new_default(seed),
            seed,
        }
    }

    /// Create a stepper for directional autorun (micro tier).
    pub fn start_autorun(&self, dir: Direction) -> MicroAutorunStepper {
        MicroAutorunStepper {
            inner: crate::tier_micro::autorun::MicroAutorunStepper::new(dir),
            all_messages: Vec::new(),
            explored_floor_before: self.game.fov.explored_floor_count(&self.game.map) as Stat,
        }
    }

    /// Run in a direction until something interesting happens.
    pub fn autorun(&mut self, dir: Direction) -> AutorunResult {
        let stepper = self.start_autorun(dir);
        stepper.run_to_completion(self)
    }

    /// Pathfind to (tx, ty) using BFS. Returns an `AutorunResult`.
    pub fn pathfind_to(&mut self, tx: i32, ty: i32) -> Result<AutorunResult, String> {
        if tx < 0 || ty < 0 || tx >= self.game.map.width as i32 || ty >= self.game.map.height as i32
        {
            return Err("Target is out of bounds".into());
        }
        let tx = tx as u8;
        let ty = ty as u8;
        if !self.game.fov.is_explored(tx, ty) {
            return Err("Target tile has not been explored".into());
        }
        if !self.game.map.is_walkable(tx, ty) {
            return Err("Target tile is not walkable".into());
        }

        let explored_before = self.game.fov.explored_floor_count(&self.game.map) as Stat;
        let mut buf = crate::tier_micro::pathfinding::BfsBuffers::new();
        let mut stepper = crate::tier_micro::autorun::MicroBfsStepper::new(tx, ty);
        let mut all_messages = Vec::new();

        loop {
            use crate::tier_micro::autorun::MicroStepOutcome;
            use crate::tier_micro::msglog::MSG_COUNT;

            let msg_count_before = self.game.log.total();
            let outcome = stepper.next_step(&mut self.game, &mut buf);

            // Collect new messages.
            let new_count = self.game.log.total();
            let added = new_count
                .wrapping_sub(msg_count_before)
                .min(MSG_COUNT as u16) as u8;
            for i in 0..added {
                if let Some(evt) = self.game.log.recent(added - 1 - i) {
                    all_messages.push(format_event(evt));
                }
            }

            match outcome {
                MicroStepOutcome::Continue => continue,
                MicroStepOutcome::Done(stop) => {
                    let explored_after = self.game.fov.explored_floor_count(&self.game.map) as Stat;
                    return Ok(AutorunResult {
                        steps_taken: stepper.steps_taken() as Stat,
                        stop_reason: map_stop_reason(stop),
                        messages: all_messages,
                        new_tiles_revealed: explored_after - explored_before,
                    });
                }
            }
        }
    }

    /// Auto-explore: find nearest frontier via BFS, then walk to it.
    pub fn auto_explore(&mut self) -> Result<AutoExploreResult, String> {
        let pi = PLAYER_IDX as usize;
        let px = self.game.entities.x[pi];
        let py = self.game.entities.y[pi];

        let mut buf = crate::tier_micro::pathfinding::BfsBuffers::new();

        let (tx, ty) = crate::tier_micro::pathfinding::find_nearest_frontier(
            px,
            py,
            &self.game.map,
            &self.game.fov,
            &mut buf,
        )
        .ok_or_else(|| "No unexplored areas reachable".to_string())?;

        let movement = self.pathfind_to(tx as i32, ty as i32)?;

        Ok(AutoExploreResult {
            target_x: tx as Coord,
            target_y: ty as Coord,
            movement,
        })
    }

    /// Count reachable frontier tiles (for MCP response).
    pub fn frontier_count(&self) -> i32 {
        let pi = PLAYER_IDX as usize;
        let mut buf = crate::tier_micro::pathfinding::BfsBuffers::new();
        crate::tier_micro::pathfinding::frontier_count(
            self.game.entities.x[pi],
            self.game.entities.y[pi],
            &self.game.map,
            &self.game.fov,
            &mut buf,
        ) as i32
    }

    /// Auto-fight: resolve adjacent combat in one call (micro tier).
    pub fn auto_fight(&mut self) -> Result<AutoFightResult, String> {
        use crate::tier_micro::msglog::MSG_COUNT;

        let msg_count_before = self.game.log.total();

        let result = self
            .game
            .auto_fight()
            .ok_or_else(|| "No adjacent monster to fight.".to_string())?;

        // Collect messages from the fight.
        let new_count = self.game.log.total();
        let added = new_count
            .wrapping_sub(msg_count_before)
            .min(MSG_COUNT as u16) as u8;
        let mut messages = Vec::new();
        for i in 0..added {
            if let Some(evt) = self.game.log.recent(added - 1 - i) {
                messages.push(format_event(evt));
            }
        }

        let target_name = result
            .target_kind
            .map(|k| monster_table::name(k).to_string())
            .unwrap_or_else(|| "Something".to_string());

        Ok(AutoFightResult {
            rounds: result.rounds as Stat,
            target_name,
            target_killed: result.target_killed,
            player_hp_lost: result.player_hp_lost as Stat,
            messages,
        })
    }

    /// The seed used to create this micro game.
    pub fn seed(&self) -> u16 {
        self.seed
    }
}

impl GameStep for MicroGameStateAdapter {
    fn step(&mut self, cmd: GameCommand) -> StepResult {
        let msg_count_before = self.game.log.total();
        let result = self.game.step(cmd);

        // Collect new messages by diffing the wrapping total counter.
        // Cap to buffer capacity to avoid stale re-reads.
        use crate::tier_micro::msglog::MSG_COUNT;
        let new_count = self.game.log.total();
        let added = new_count
            .wrapping_sub(msg_count_before)
            .min(MSG_COUNT as u16) as u8;
        let new_messages: Vec<String> = (0..added)
            .filter_map(|i| self.game.log.recent(added - 1 - i).map(format_event))
            .collect();

        StepResult {
            action_taken: result.action_taken,
            new_messages,
            game_over: result.game_over,
            game_won: result.game_won,
        }
    }

    fn observe(&self) -> GameObservation {
        let pi = PLAYER_IDX as usize;
        let entities = &self.game.entities;
        let map = &self.game.map;
        let fov = &self.game.fov;

        // Build ASCII map — only rows with visible content.
        let map_ascii = build_micro_map_ascii(map, fov, entities, &self.game.items);

        // Visible entities (excluding player).
        let visible_entities = build_micro_visible_entities(entities, fov);

        // Visible items on ground.
        let visible_items = build_micro_visible_items(&self.game.items, fov);

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

        // Seed code — encode with runtime dimensions.
        let seed_code = seed_code::encode(&SeedParams {
            seed: self.seed as u64,
            width: map.width as i32,
            height: map.height as i32,
            preset: None,
        });

        let (inv_strings, inv_colors) = build_micro_inventory(&self.game.inventory);

        GameObservation {
            player_hp: entities.hp[pi] as i32,
            player_max_hp: entities.max_hp[pi] as i32,
            player_atk: self.game.effective_attack() as i32,
            player_def: self.game.effective_defense() as i32,
            player_x: entities.x[pi] as i32,
            player_y: entities.y[pi] as i32,
            map_ascii,
            visible_entities,
            visible_items,
            recent_messages,
            game_over: self.game.game_over,
            turn_count: self.game.turn_count as i32,
            weapon: self
                .game
                .equipment
                .weapon
                .map(|k| rules_items::name(k).to_string()),
            armor: self
                .game
                .equipment
                .armor
                .map(|k| rules_items::name(k).to_string()),
            kills: self.game.kills as i32,
            rooms_found: map.room_count as i32,
            explored_pct,
            inventory: inv_strings,
            inventory_colors: inv_colors,
            seed: self.seed as u64,
            seed_code,
            depth: self.game.depth as i32,
            target_depth: balance::TARGET_DEPTH as i32,
            game_won: self.game.game_won,
            stairs: find_explored_stairs(map, fov),
        }
    }

    fn look_at(&self, x: i32, y: i32) -> TileInfo {
        let map = &self.game.map;
        // Out of bounds or negative → unknown.
        if x < 0 || y < 0 || x >= map.width as i32 || y >= map.height as i32 {
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
            TILE_WALL => "Void".into(),
            TILE_STRUCTURAL => "Wall".into(),
            TILE_FLOOR => "Floor".into(),
            TILE_STAIRS_DOWN => "Stairs down".into(),
            _ => "Unknown".into(),
        };

        let entity = if visible {
            entity_info_at(entities, ux, uy)
        } else {
            None
        };

        let items_at: Vec<ItemInfo> = if visible {
            build_micro_items_at(&self.game.items, ux, uy)
        } else {
            Vec::new()
        };

        let glyph = if visible {
            if let Some(ref ei) = entity {
                ei.glyph
            } else if let Some(first) = items_at.first() {
                first.glyph
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
            items: items_at,
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
        self.game.is_terminal()
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

/// Autorun stepper for the micro tier — wraps the no_std stepper from
/// `tier_micro::autorun` and adds std-tier concerns (message collection,
/// explored tile tracking).
pub struct MicroAutorunStepper {
    inner: crate::tier_micro::autorun::MicroAutorunStepper,
    all_messages: Vec<String>,
    explored_floor_before: Stat,
}

/// Map a no_std `MicroAutorunStop` to the std-tier `AutorunStopReason`.
fn map_stop_reason(stop: crate::tier_micro::autorun::MicroAutorunStop) -> AutorunStopReason {
    use crate::tier_micro::autorun::MicroAutorunStop;
    match stop {
        MicroAutorunStop::WallReached => AutorunStopReason::WallReached,
        MicroAutorunStop::MonsterSpotted => AutorunStopReason::MonsterSpotted,
        MicroAutorunStop::DamageTaken => AutorunStopReason::DamageTaken,
        MicroAutorunStop::GameOver => AutorunStopReason::GameOver,
        MicroAutorunStop::CorridorBranches => AutorunStopReason::CorridorBranches,
        MicroAutorunStop::MaxSteps => AutorunStopReason::MaxSteps,
        MicroAutorunStop::PathComplete => AutorunStopReason::PathComplete,
        MicroAutorunStop::StairsFound => AutorunStopReason::StairsFound,
    }
}

impl MicroAutorunStepper {
    /// Execute one step of the autorun sequence.
    pub fn next_step(&mut self, adapter: &mut MicroGameStateAdapter) -> StepOutcome {
        use crate::tier_micro::autorun::MicroStepOutcome;
        use crate::tier_micro::msglog::MSG_COUNT;

        // Capture message count before the inner step (which calls state.step()).
        let msg_count_before = adapter.game.log.total();

        let outcome = self.inner.next_step(&mut adapter.game);

        // Collect new messages by diffing the wrapping total counter.
        let new_count = adapter.game.log.total();
        let added = new_count
            .wrapping_sub(msg_count_before)
            .min(MSG_COUNT as u16) as u8;
        for i in 0..added {
            if let Some(evt) = adapter.game.log.recent(added - 1 - i) {
                self.all_messages.push(format_event(evt));
            }
        }

        match outcome {
            MicroStepOutcome::Continue => StepOutcome::Continue,
            MicroStepOutcome::Done(stop) => {
                let explored_floor_after =
                    adapter.game.fov.explored_floor_count(&adapter.game.map) as Stat;
                StepOutcome::Done(AutorunResult {
                    steps_taken: self.inner.steps_taken() as Stat,
                    stop_reason: map_stop_reason(stop),
                    messages: std::mem::take(&mut self.all_messages),
                    new_tiles_revealed: explored_floor_after - self.explored_floor_before,
                })
            }
        }
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
}

// ── Micro adapter helpers ────────────────────────────────────────────

/// Find explored stairs-down position on the micro-tier map, if any.
fn find_explored_stairs(
    map: &crate::tier_micro::map::MicroMap,
    fov: &MicroFov,
) -> Option<(Coord, Coord)> {
    for y in 0..map.height {
        for x in 0..map.width {
            if fov.is_explored(x, y) && map.tile_at(x, y) == TILE_STAIRS_DOWN {
                return Some((x as Coord, y as Coord));
            }
        }
    }
    None
}

fn tile_glyph(tile: u8) -> char {
    match tile {
        TILE_WALL => ' ',
        TILE_STRUCTURAL => '#',
        TILE_FLOOR => '.',
        TILE_STAIRS_DOWN => '>',
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
    items: &ItemStore,
) -> Vec<String> {
    let mut lines = Vec::new();
    for y in 0..map.height {
        let mut line = String::with_capacity(map.width as usize);
        let mut has_content = false;

        for x in 0..map.width {
            if fov.is_visible(x, y) {
                has_content = true;
                // Priority: entity > item > tile.
                if let Some(ei) = entity_info_at(entities, x, y) {
                    line.push(ei.glyph);
                } else if let Some(glyph) = item_glyph_at(items, x, y) {
                    line.push(glyph);
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
    // Collect up to MSG_COUNT recent messages (oldest first).
    use crate::tier_micro::msglog::MSG_COUNT;
    for i in (0..MSG_COUNT as u8).rev() {
        if let Some(event) = log.recent(i) {
            messages.push(format_event(event));
        }
    }
    messages
}

/// Get the glyph of the first alive item at (x, y), if any.
fn item_glyph_at(items: &ItemStore, x: u8, y: u8) -> Option<char> {
    for i in 0..items.count as usize {
        if items.alive[i] && items.x[i] == x && items.y[i] == y {
            return Some(rules_items::glyph(items.kind[i]));
        }
    }
    None
}

/// Build visible items list for observe().
fn build_micro_visible_items(items: &ItemStore, fov: &MicroFov) -> Vec<ItemInfo> {
    let mut result = Vec::new();
    for i in 0..items.count as usize {
        if items.alive[i] && fov.is_visible(items.x[i], items.y[i]) {
            result.push(ItemInfo {
                name: rules_items::name(items.kind[i]).to_string(),
                glyph: rules_items::glyph(items.kind[i]),
                x: items.x[i] as i32,
                y: items.y[i] as i32,
            });
        }
    }
    result
}

/// Build inventory display strings and colors for a micro-tier Inventory.
fn build_micro_inventory(inv: &crate::rules::items::Inventory) -> (Vec<String>, Vec<GameColor>) {
    let mut strings = Vec::new();
    let mut colors = Vec::new();
    for (i, slot) in inv.iter() {
        let letter = (b'a' + i as u8) as char;
        let name = rules_items::name(slot.kind);
        if slot.count > 1 {
            strings.push(format!("{}) {} (x{})", letter, name, slot.count));
        } else {
            strings.push(format!("{}) {}", letter, name));
        }
        colors.push(rules_items::color(slot.kind));
    }
    (strings, colors)
}

/// Build item info list at a specific tile for look_at().
fn build_micro_items_at(items: &ItemStore, x: u8, y: u8) -> Vec<ItemInfo> {
    let mut result = Vec::new();
    for i in 0..items.count as usize {
        if items.alive[i] && items.x[i] == x && items.y[i] == y {
            result.push(ItemInfo {
                name: rules_items::name(items.kind[i]).to_string(),
                glyph: rules_items::glyph(items.kind[i]),
                x: x as i32,
                y: y as i32,
            });
        }
    }
    result
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
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
        let obs = adapter.observe();
        // The map should show the player '@' somewhere.
        let has_player = obs.map_ascii.iter().any(|l| l.contains('@'));
        assert!(has_player, "map should show player glyph");
    }

    #[test]
    fn micro_adapter_step_translates_result() {
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
        let result = adapter.step(GameCommand::Wait);
        assert!(result.action_taken);
        assert!(!result.game_won);
        assert!(!result.game_over);
    }

    #[test]
    fn micro_adapter_look_at_player_tile() {
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
        let tile = adapter.look_at(-1, -1);
        assert!(!tile.visible);
        assert!(!tile.explored);
        assert_eq!(tile.terrain, "Out of bounds");
    }

    #[test]
    fn micro_adapter_seed_code() {
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
        let obs = adapter.observe();
        assert!(!obs.seed_code.is_empty());
        assert_eq!(obs.seed, 42);
    }

    #[test]
    fn both_tiers_same_trait_object() {
        let mut standard = test_standard_game();
        let mut micro = MicroGameStateAdapter::new(42, 80, 40);

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
    fn micro_adapter_observe_depth() {
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
        let obs = adapter.observe();
        assert_eq!(obs.depth, 1);
        assert_eq!(obs.target_depth, balance::TARGET_DEPTH as i32);
        assert!(!obs.game_won);
    }

    #[test]
    fn micro_adapter_descend_updates_depth() {
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
        // Teleport player to stairs
        let last = adapter.game.map.rooms[(adapter.game.map.room_count - 1) as usize];
        adapter.game.entities.x[0] = last.cx();
        adapter.game.entities.y[0] = last.cy();

        let result = adapter.step(GameCommand::Descend);
        assert!(result.action_taken);
        assert!(!result.game_won);

        let obs = adapter.observe();
        assert_eq!(obs.depth, 2);
        assert_eq!(obs.target_depth, balance::TARGET_DEPTH as i32);
    }

    #[test]
    fn micro_adapter_messages_after_step() {
        let adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let game = create_game(42, 80, 40, None, &gd).unwrap();
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
        // Micro tier also validates dimensions now.
        assert!(create_game(42, 10, 10, None, &gd).is_err());
    }

    #[test]
    fn create_game_micro_validates_dimensions() {
        let gd = data::load_game_data();
        // Too small.
        assert!(create_game(42, 10, 10, None, &gd).is_err());
        // Too large for micro tier.
        assert!(create_game(42, 90, 70, None, &gd).is_err());
        // Valid micro dimensions.
        assert!(create_game(42, 80, 40, None, &gd).is_ok());
        assert!(create_game(42, 64, 48, None, &gd).is_ok());
        assert!(create_game(42, 20, 15, None, &gd).is_ok());
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
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
        let result = adapter.autorun(Direction::East);
        assert!(result.steps_taken <= balance::MAX_AUTORUN_STEPS as Stat);
    }

    #[test]
    fn micro_autorun_stepper_step_by_step() {
        use crate::command::Direction;
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
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
        let mut adapter = MicroGameStateAdapter::new(42, 80, 40);
        let result = adapter.autorun(Direction::East);
        // Messages vec should be valid (may be empty if no combat).
        let _ = result.messages;
    }
}
