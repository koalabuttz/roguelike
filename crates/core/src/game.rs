use std::collections::HashSet;

use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::combat;
use crate::command::{Direction, GameCommand};
use crate::data;
use crate::entity::{Entity, EntityKind};
use crate::fov;
use crate::item::{self, Equipment, Item};
use crate::map;
use crate::message_log::MessageLog;
use crate::pathfinding;
use crate::rules::color::GameColor;
use crate::rules::interactions;
use crate::rules::items::{self as rules_items, Inventory};
use crate::rules::message::{GameEvent, SoundDistance};
use crate::seed_code::{self, SeedParams};
use crate::spawn;
use crate::types::{Coord, Pos, Stat};

/// Result of executing one complete game step (player command + monster turns).
pub struct StepResult {
    /// Whether the player's command consumed a turn.
    pub action_taken: bool,
    /// Messages generated during this step (combat, deaths, etc.).
    pub new_messages: Vec<String>,
    /// Whether the game ended this step (player died).
    pub game_over: bool,
    /// Whether the player won this step (descended past target depth).
    pub game_won: bool,
}

/// Why autorun stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutorunStopReason {
    /// Hit a wall or dead end.
    WallReached,
    /// A new living monster entered the field of view.
    MonsterSpotted,
    /// Player took damage from a monster.
    DamageTaken,
    /// Player died.
    GameOver,
    /// Forward path blocked with multiple alternative directions available.
    CorridorBranches,
    /// Pathfinding reached the destination tile.
    PathComplete,
    /// Safety cap on steps reached.
    MaxSteps,
    /// Player stepped onto stairs.
    StairsFound,
}

/// Result of an autorun sequence — multiple steps collapsed into one call.
#[derive(Debug, Serialize)]
pub struct AutorunResult {
    /// How many tiles the player moved.
    pub steps_taken: Stat,
    /// Why the run stopped.
    pub stop_reason: AutorunStopReason,
    /// All messages generated during the run.
    pub messages: Vec<String>,
    /// How many new tiles were added to the explored set during this run.
    pub new_tiles_revealed: Stat,
}

/// A snapshot of the visible game state, suitable for serialization.
#[derive(Serialize)]
pub struct GameObservation {
    #[serde(rename = "hp")]
    pub player_hp: Stat,
    #[serde(rename = "max_hp")]
    pub player_max_hp: Stat,
    #[serde(rename = "atk")]
    pub player_atk: Stat,
    #[serde(rename = "def")]
    pub player_def: Stat,
    #[serde(rename = "x")]
    pub player_x: Coord,
    #[serde(rename = "y")]
    pub player_y: Coord,
    #[serde(rename = "map")]
    pub map_ascii: Vec<String>,
    #[serde(rename = "entities")]
    pub visible_entities: Vec<EntityInfo>,
    #[serde(rename = "items")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub visible_items: Vec<ItemInfo>,
    #[serde(rename = "messages")]
    pub recent_messages: Vec<String>,
    pub game_over: bool,
    pub turn_count: Stat,
    // --- equipment ---
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor: Option<String>,
    // --- game stats ---
    pub kills: Stat,
    pub rooms_found: Stat,
    #[serde(rename = "explored")]
    pub explored_pct: Stat,
    pub seed: u64,
    pub seed_code: String,
    // --- inventory ---
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub inventory: Vec<String>,
    /// Per-slot item color for inventory rendering (parallel to `inventory`).
    #[serde(skip)]
    #[serde(default)]
    pub inventory_colors: Vec<GameColor>,
    // --- depth ---
    pub depth: Stat,
    pub target_depth: Stat,
    pub game_won: bool,
    // --- stairs ---
    /// Explored stairs-down position, if known. Populated once the player
    /// has explored the tile containing stairs (spatial memory analogue).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stairs: Option<(Coord, Coord)>,
}

/// Result of an auto-fight sequence — combat resolved in one call.
#[derive(Debug, Serialize)]
pub struct AutoFightResult {
    /// How many rounds (full turns) the fight lasted.
    pub rounds: Stat,
    /// Name of the monster fought.
    pub target_name: String,
    /// Whether the target was killed.
    pub target_killed: bool,
    /// Total HP the player lost during the fight (from all sources).
    pub player_hp_lost: Stat,
    /// All messages generated during the fight.
    pub messages: Vec<String>,
}

/// Result of an auto-explore action — finds and walks to the nearest frontier.
#[derive(Debug)]
pub struct AutoExploreResult {
    /// X coordinate of the chosen frontier target.
    pub target_x: Coord,
    /// Y coordinate of the chosen frontier target.
    pub target_y: Coord,
    /// The movement result from pathfinding to the frontier.
    pub movement: AutorunResult,
}

/// Determines how the stepper decides the next direction each step.
pub enum StepperMode {
    /// Move in a fixed direction each step (autorun behavior).
    Directional(Direction),
    /// Follow a precomputed A* path, stepping through each waypoint.
    FollowPath { path: Vec<Pos>, index: usize },
}

/// Yields one game step at a time for multi-step movement sequences.
///
/// Created by `GameState::start_autorun()`, `start_pathfind()`, or
/// `start_auto_explore()`. Consumed either step-by-step (for animation)
/// or all at once via `run_to_completion()`.
pub struct AutorunStepper {
    mode: StepperMode,
    steps_taken: Stat,
    max_steps: Stat,
    all_messages: Vec<String>,
    explored_before: Stat,
    stairs_visible_before: bool,
}

/// Result of a single stepper step.
pub enum StepOutcome {
    /// Step succeeded, stepper can continue.
    Continue,
    /// Sequence is finished.
    Done(AutorunResult),
}

impl AutorunStepper {
    /// Execute one step of the multi-step sequence.
    pub fn next_step(&mut self, state: &mut GameState) -> StepOutcome {
        // Check 1: max steps cap.
        if self.steps_taken >= self.max_steps {
            return self.finish(state, AutorunStopReason::MaxSteps);
        }

        // Check 2: adjacent monster before stepping.
        if state.has_adjacent_monster() {
            return self.finish(state, AutorunStopReason::MonsterSpotted);
        }

        // Compute direction from mode.
        let dir = match &self.mode {
            StepperMode::Directional(dir) => *dir,
            StepperMode::FollowPath { path, index } => {
                if *index >= path.len() {
                    return self.finish(state, AutorunStopReason::PathComplete);
                }
                let (nx, ny) = path[*index];
                let cx = state.entities[0].x;
                let cy = state.entities[0].y;
                match Direction::from_offset(nx - cx, ny - cy) {
                    Some(d) => d,
                    None => return self.finish(state, AutorunStopReason::PathComplete),
                }
            }
        };

        // Snapshot before step.
        let hp_before = state.entities[0].hp;
        let visible_monsters_before = state.visible_monster_ids();

        let result = state.step(GameCommand::Move(dir));
        self.all_messages.extend(result.new_messages);

        // Check 3: wall hit.
        if !result.action_taken {
            return self.finish(state, AutorunStopReason::WallReached);
        }

        self.steps_taken += 1;

        // Advance path index for FollowPath mode.
        if let StepperMode::FollowPath { index, .. } = &mut self.mode {
            *index += 1;
        }

        // Check 4: game over.
        if result.game_over {
            return self.finish(state, AutorunStopReason::GameOver);
        }

        // Check 5: damage taken.
        if state.entities[0].hp < hp_before {
            return self.finish(state, AutorunStopReason::DamageTaken);
        }

        // Check 6: new monster spotted.
        let visible_monsters_after = state.visible_monster_ids();
        if visible_monsters_after
            .difference(&visible_monsters_before)
            .next()
            .is_some()
        {
            return self.finish(state, AutorunStopReason::MonsterSpotted);
        }

        // Check 6b: stairs newly visible in FOV.
        if !self.stairs_visible_before && state.stairs_in_visible() {
            return self.finish(state, AutorunStopReason::StairsFound);
        }

        // Mode-specific post-step logic.
        match &self.mode {
            StepperMode::Directional(dir) => {
                let (dx, dy) = dir.to_offset();
                let px = state.entities[0].x;
                let py = state.entities[0].y;
                if !state.map.is_walkable(px + dx, py + dy) {
                    let alternatives = state.map.open_neighbors_excluding(px, py, -dx, -dy);
                    if alternatives >= 2 {
                        return self.finish(state, AutorunStopReason::CorridorBranches);
                    }
                    return self.finish(state, AutorunStopReason::WallReached);
                }
            }
            StepperMode::FollowPath { path, index } => {
                if *index >= path.len() {
                    return self.finish(state, AutorunStopReason::PathComplete);
                }
            }
        }

        // Check: stepped onto stairs.
        {
            let px = state.entities[0].x;
            let py = state.entities[0].y;
            let idx = state.map.idx(px, py);
            if state.map.tiles[idx] == map::Tile::StairsDown {
                return self.finish(state, AutorunStopReason::StairsFound);
            }
        }

        StepOutcome::Continue
    }

    /// Run all remaining steps without pausing.
    pub fn run_to_completion(mut self, state: &mut GameState) -> AutorunResult {
        loop {
            match self.next_step(state) {
                StepOutcome::Continue => continue,
                StepOutcome::Done(result) => return result,
            }
        }
    }

    /// Build the final AutorunResult.
    fn finish(&mut self, state: &GameState, reason: AutorunStopReason) -> StepOutcome {
        StepOutcome::Done(AutorunResult {
            steps_taken: self.steps_taken,
            stop_reason: reason,
            messages: std::mem::take(&mut self.all_messages),
            new_tiles_revealed: state.explored.len() as Stat - self.explored_before,
        })
    }
}

/// Info about a visible item on the ground.
#[derive(Debug, Serialize)]
pub struct ItemInfo {
    pub name: String,
    pub glyph: char,
    pub x: Coord,
    pub y: Coord,
}

/// Info about a tile at a given position, returned by `look_at()`.
#[derive(Debug, Serialize)]
pub struct TileInfo {
    pub x: Coord,
    pub y: Coord,
    pub terrain: String,
    pub entity: Option<EntityInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemInfo>,
    pub visible: bool,
    pub explored: bool,
    pub glyph: char,
}

/// Info about a visible entity (monster or corpse).
#[derive(Debug, Serialize)]
pub struct EntityInfo {
    pub name: String,
    pub glyph: char,
    pub x: Coord,
    pub y: Coord,
    pub hp: Stat,
    pub max_hp: Stat,
    pub alive: bool,
}

/// Options that control what `look_at` reveals beyond normal FOV.
///
/// Default is conservative (reveal nothing). Platform layers build this
/// from their own debug/dev state and pass it into `look_at_with`.
#[derive(Debug, Clone, Default)]
pub struct LookOptions {
    /// Show alive monsters even outside the player's FOV.
    pub reveal_monsters: bool,
}

fn default_regen_interval() -> Stat {
    3
}
fn default_max_autorun_steps() -> Stat {
    100
}
fn default_depth() -> Stat {
    1
}
fn default_target_depth() -> Stat {
    5
}
fn default_map_config() -> Stat {
    30
}
fn default_room_size_min() -> Coord {
    4
}
fn default_room_size_max() -> Coord {
    10
}
fn default_max_monsters_per_room() -> Stat {
    2
}

/// Independent RNG streams derived from a master seed.
/// Order matters for determinism: map (1st), spawn (2nd), wandering (3rd), items (4th).
struct DerivedRngs {
    map: StdRng,
    spawn: StdRng,
    wandering_seed: u64,
    item: StdRng,
}

impl DerivedRngs {
    fn from_seed(seed: u64) -> Self {
        let mut master = StdRng::seed_from_u64(seed);
        Self {
            map: StdRng::from_rng(&mut master).unwrap(),
            spawn: StdRng::from_rng(&mut master).unwrap(),
            wandering_seed: StdRng::from_rng(&mut master).unwrap().next_u64(),
            item: StdRng::from_rng(&mut master).unwrap(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct GameState {
    pub map: map::Map,
    pub entities: Vec<Entity>,
    pub fov_radius: Coord,
    #[serde(skip)]
    pub visible: HashSet<Pos>,
    pub explored: HashSet<Pos>,
    pub log: MessageLog,
    pub game_over: bool,
    pub turn_count: Stat,
    /// The seed used to generate this game. Enables reproducible dungeons,
    /// seed sharing, and deterministic replay.
    pub seed: u64,
    /// The map preset used for this game, if any.
    #[serde(default)]
    pub preset: Option<map::MapPreset>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(default = "default_regen_interval")]
    pub regen_interval: Stat,
    #[serde(default = "default_max_autorun_steps")]
    pub max_autorun_steps: Stat,
    /// Deterministic RNG seed for wandering monster spawns and movement.
    #[serde(default)]
    pub wandering_seed: u64,
    /// Wandering monster spawn/sound config (copied from GameData at creation).
    #[serde(default)]
    pub wandering_config: data::WanderingConfig,
    /// Consecutive Wait commands (resets on any non-wait action).
    #[serde(default)]
    pub idle_count: Stat,
    /// Total wandering monsters spawned this game (for analytics).
    #[serde(default)]
    pub wandering_spawned: Stat,
    /// Monster spawn table for wandering spawns.
    #[serde(default)]
    pub wandering_spawn_table: Vec<data::MonsterDef>,
    /// Items lying on the ground.
    #[serde(default)]
    pub ground_items: Vec<Item>,
    /// Player's equipped items.
    #[serde(default)]
    pub equipment: Equipment,
    /// Player's inventory (Brogue-style 26 slots, a-z).
    #[serde(default)]
    pub inventory: Inventory,
    /// Auto-pickup consumable items when walking over them.
    #[serde(default)]
    pub auto_pickup: bool,
    /// Current dungeon depth (1-based).
    #[serde(default = "default_depth")]
    pub depth: Stat,
    /// Target depth — reaching this wins the game.
    #[serde(default = "default_target_depth")]
    pub target_depth: Stat,
    /// Whether the player has won by descending past target_depth.
    #[serde(default)]
    pub game_won: bool,
    /// Depth scaling config (copied from GameData at creation).
    #[serde(default)]
    pub depth_scaling: data::DepthScaling,
    /// Map generation config (copied from GameConfig at creation, persists across floors).
    #[serde(default = "default_map_config")]
    pub max_rooms: Stat,
    #[serde(default = "default_room_size_min")]
    pub room_size_min: Coord,
    #[serde(default = "default_room_size_max")]
    pub room_size_max: Coord,
    #[serde(default = "default_max_monsters_per_room")]
    pub max_monsters_per_room: Stat,
}

impl GameState {
    /// Create a new game with a random seed.
    #[cfg(feature = "data-files")]
    pub fn new(width: Coord, height: Coord) -> Self {
        Self::with_seed(width, height, rand::random::<u64>())
    }

    /// Create a new game with a random seed and custom game data.
    pub fn new_with_data(width: Coord, height: Coord, game_data: &data::GameData) -> Self {
        Self::with_data(width, height, rand::random::<u64>(), game_data)
    }

    /// Create a new game using a named map preset.
    ///
    /// Presets produce deterministic topologies for testing and development.
    /// Monster spawning still uses the seed for placement within rooms.
    #[cfg(feature = "data-files")]
    pub fn with_preset(width: Coord, height: Coord, seed: u64, preset: map::MapPreset) -> Self {
        Self::with_preset_data(width, height, seed, preset, data::defaults())
    }

    /// Create a new game using a named map preset and custom game data.
    pub fn with_preset_data(
        width: Coord,
        height: Coord,
        seed: u64,
        preset: map::MapPreset,
        game_data: &data::GameData,
    ) -> Self {
        let cfg = &game_data.config;
        let DerivedRngs {
            map: mut map_rng,
            spawn: mut spawn_rng,
            wandering_seed,
            item: mut item_rng,
        } = DerivedRngs::from_seed(seed);

        let mut map = map::Map::new(width, height);
        let (px, py) = map.from_preset(preset, &mut map_rng);
        map.compute_structural_walls();

        let mut entities = vec![Entity::player_from_def(&game_data.player, px, py)];
        let monsters = spawn::spawn_monsters(
            &map,
            &game_data.monsters,
            cfg.max_monsters_per_room,
            &mut spawn_rng,
        );
        entities.extend(monsters);

        let ground_items = spawn::spawn_items(&map, item::MAX_ITEMS_PER_ROOM, 1, &mut item_rng);

        let visible = fov::compute_fov(&map, px, py, cfg.fov_radius);
        let explored = visible.clone();

        map.place_stairs_down();

        let mut log = MessageLog::new();
        log.add(format!("Welcome! Map preset: {:?}", preset));

        GameState {
            map,
            entities,
            fov_radius: cfg.fov_radius,
            visible,
            explored,
            log,
            game_over: false,
            turn_count: 0,
            seed,
            preset: Some(preset),
            dirty: false,
            regen_interval: cfg.regen_interval,
            max_autorun_steps: cfg.max_autorun_steps,
            wandering_seed,
            wandering_config: game_data.wandering.clone(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: game_data.monsters.clone(),
            ground_items,
            equipment: Equipment::default(),
            inventory: Inventory::new(),
            auto_pickup: false,
            depth: 1,
            target_depth: cfg.target_depth,
            game_won: false,
            depth_scaling: game_data.depth_scaling.clone(),
            max_rooms: cfg.max_rooms,
            room_size_min: cfg.room_size_min,
            room_size_max: cfg.room_size_max,
            max_monsters_per_room: cfg.max_monsters_per_room,
        }
    }

    /// Create a new game with a specific seed for reproducible dungeons.
    ///
    /// The seed determines map layout and monster placement. Separate RNG
    /// streams ensure that changes to one system (e.g., spawn weights)
    /// don't alter another (e.g., map layout) for the same seed.
    #[cfg(feature = "data-files")]
    pub fn with_seed(width: Coord, height: Coord, seed: u64) -> Self {
        Self::with_data(width, height, seed, data::defaults())
    }

    /// Create a new game with a specific seed and custom game data.
    ///
    /// This is the modding entry point — pass custom `GameData` to override
    /// all balance values, monster definitions, and player stats.
    pub fn with_data(width: Coord, height: Coord, seed: u64, game_data: &data::GameData) -> Self {
        let cfg = &game_data.config;
        let DerivedRngs {
            map: mut map_rng,
            spawn: mut spawn_rng,
            wandering_seed,
            item: mut item_rng,
        } = DerivedRngs::from_seed(seed);

        let mut map = map::Map::new(width, height);
        let (px, py) = map.generate(
            cfg.max_rooms,
            cfg.room_size_min,
            cfg.room_size_max,
            &mut map_rng,
        );
        map.compute_structural_walls();
        map.place_stairs_down();

        let mut entities = vec![Entity::player_from_def(&game_data.player, px, py)];
        let monsters = spawn::spawn_monsters(
            &map,
            &game_data.monsters,
            cfg.max_monsters_per_room,
            &mut spawn_rng,
        );
        entities.extend(monsters);

        let ground_items = spawn::spawn_items(&map, item::MAX_ITEMS_PER_ROOM, 1, &mut item_rng);

        let visible = fov::compute_fov(&map, px, py, cfg.fov_radius);
        let explored = visible.clone();

        let mut log = MessageLog::new();
        log.add_event(GameEvent::Welcome);

        GameState {
            map,
            entities,
            fov_radius: cfg.fov_radius,
            visible,
            explored,
            log,
            game_over: false,
            turn_count: 0,
            seed,
            preset: None,
            dirty: false,
            regen_interval: cfg.regen_interval,
            max_autorun_steps: cfg.max_autorun_steps,
            wandering_seed,
            wandering_config: game_data.wandering.clone(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: game_data.monsters.clone(),
            ground_items,
            equipment: Equipment::default(),
            inventory: Inventory::new(),
            auto_pickup: false,
            depth: 1,
            target_depth: cfg.target_depth,
            game_won: false,
            depth_scaling: game_data.depth_scaling.clone(),
            max_rooms: cfg.max_rooms,
            room_size_min: cfg.room_size_min,
            room_size_max: cfg.room_size_max,
            max_monsters_per_room: cfg.max_monsters_per_room,
        }
    }

    /// Generate a shareable seed code for this game.
    pub fn seed_code(&self) -> String {
        seed_code::encode(&SeedParams {
            seed: self.seed,
            width: self.map.width,
            height: self.map.height,
            preset: self.preset,
        })
    }

    pub fn update_fov(&mut self) {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        self.visible = fov::compute_fov(&self.map, px, py, self.fov_radius);
        self.explored.extend(&self.visible);
    }

    /// Serialize the current game state to a JSON string.
    pub fn save_to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize a game state from a JSON string.
    ///
    /// After loading, `update_fov()` is called to rebuild the `visible` set
    /// (which is skipped during serialization as runtime-only derived state).
    pub fn load_from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut state: Self = serde_json::from_str(json)?;
        state.map.compute_structural_walls();
        state.update_fov();
        // Old saves lack the wandering_spawn_table field; fall back to defaults.
        #[cfg(feature = "data-files")]
        if state.wandering_spawn_table.is_empty() {
            state.wandering_spawn_table = data::defaults().monsters.clone();
        }
        Ok(state)
    }

    /// Find a living entity at (x, y). Returns its index.
    pub fn entity_at(&self, x: Coord, y: Coord) -> Option<usize> {
        self.entities
            .iter()
            .position(|e| e.alive && e.x == x && e.y == y)
    }

    /// Try to move the player. If a living monster occupies the target cell, attack it instead.
    /// Returns true if the player took an action (moved or attacked).
    pub fn player_move_or_attack(&mut self, dx: Coord, dy: Coord) -> bool {
        let new_x = self.entities[0].x + dx;
        let new_y = self.entities[0].y + dy;

        if let Some(target_idx) = self.entity_at(new_x, new_y)
            && target_idx != 0
        {
            let atk = self.effective_attack();
            let def = self.entities[target_idx].defense;
            combat::melee_attack(&mut self.entities, 0, target_idx, atk, def, &mut self.log);
            return true;
        }

        if self.map.is_walkable(new_x, new_y) {
            self.entities[0].x = new_x;
            self.entities[0].y = new_y;
            self.notify_items_here(new_x, new_y);
            return true;
        }

        false
    }

    /// Notify the player about items on the ground at their position.
    /// When auto_pickup is enabled, consumables are picked up first.
    fn notify_items_here(&mut self, x: Coord, y: Coord) {
        if self.auto_pickup {
            self.auto_pickup_items(x, y);
        }
        // Notify about remaining items (if inventory was full).
        let mut counts = [0u8; rules_items::KIND_COUNT];
        for item in &self.ground_items {
            if item.x == x && item.y == y {
                counts[item.kind as usize] += 1;
            }
        }
        for (idx, &count) in counts.iter().enumerate() {
            if count > 0 {
                self.log.add_event(GameEvent::ItemsHere {
                    kind: rules_items::ALL_KINDS[idx],
                    count,
                });
            }
        }
    }

    /// Auto-pickup all items at (x, y).
    fn auto_pickup_items(&mut self, x: Coord, y: Coord) {
        loop {
            let idx = self
                .ground_items
                .iter()
                .position(|it| it.x == x && it.y == y);
            let Some(idx) = idx else { break };
            let kind = self.ground_items[idx].kind;
            if !self.inventory.add(kind) {
                break; // inventory full — remaining items get normal notifications
            }
            self.ground_items.remove(idx);
            self.log.add_event(GameEvent::PickupItem { kind });
        }
    }

    /// Pick up the first item at the player's position.
    fn pickup_item(&mut self) -> bool {
        let px = self.entities[0].x;
        let py = self.entities[0].y;

        let idx = self
            .ground_items
            .iter()
            .position(|it| it.x == px && it.y == py);
        let idx = match idx {
            Some(i) => i,
            None => return false, // nothing to pick up
        };

        let kind = self.ground_items[idx].kind;
        if !self.inventory.add(kind) {
            self.log.add_event(GameEvent::InventoryFull);
            return true; // turn consumed even on failure
        }
        self.ground_items.remove(idx);
        self.log.add_event(GameEvent::PickupItem { kind });
        true
    }

    /// Use an item from inventory (consumables only).
    fn use_item(&mut self, slot: u8) -> bool {
        let inv_slot = match self.inventory.get(slot as usize) {
            Some(s) => *s,
            None => return false,
        };

        if rules_items::is_consumable(inv_slot.kind) {
            let heal = rules_items::heal_amount(inv_slot.kind) as Stat;
            if heal > 0 {
                let player = &mut self.entities[0];
                let healed = heal.min(player.max_hp.saturating_sub(player.hp));
                player.hp = player.hp.saturating_add(healed);
                self.inventory.remove_one(slot as usize);
                self.log.add_event(GameEvent::DrinkPotion {
                    kind: inv_slot.kind,
                    healed: healed as u8,
                });
                return true;
            }
            let boost = rules_items::strength_boost(inv_slot.kind);
            if boost > 0 {
                self.entities[0].attack += boost as Stat;
                self.inventory.remove_one(slot as usize);
                self.log
                    .add_event(GameEvent::UseStrengthPotion { bonus: boost });
                return true;
            }
        }
        false
    }

    /// Combine two inventory items: apply source's properties onto target.
    /// Consumable sources are consumed; equipment sources are kept.
    /// Stacked items are split: only one item from each stack participates.
    fn combine_items(&mut self, target_slot: u8, source_slot: u8) -> bool {
        if target_slot == source_slot {
            return false;
        }
        let target = match self.inventory.get(target_slot as usize) {
            Some(s) => *s,
            None => return false,
        };
        let source = match self.inventory.get(source_slot as usize) {
            Some(s) => *s,
            None => return false,
        };

        let mut a_props = target.props;
        let mut b_props = source.props;
        let mut effects = [interactions::Effect {
            effect_type: interactions::EffectType::Glow,
            intensity: 0,
        }; interactions::MAX_EFFECTS];

        let _effect_count = interactions::interact(&mut a_props, &mut b_props, &mut effects);

        // Check if any properties actually changed.
        if a_props == target.props && b_props == source.props {
            self.log.add_event(GameEvent::CombineNoEffect);
            return false;
        }

        // Source is removed if consumable OR if its material was destroyed
        // by the interaction (Cancel rules can reduce METAL/ORGANIC on both
        // items). Remove source first to free a slot for the split target.
        let source_consumed = rules_items::is_consumable(source.kind);
        let source_destroyed =
            !source_consumed && rules_items::is_material_dead(source.kind, &b_props);
        let source_removed = source_consumed || source_destroyed;
        if source_removed {
            self.inventory.remove_one(source_slot as usize);
        }

        // Check if the target's structural material was destroyed.
        let target_destroyed = rules_items::is_material_dead(target.kind, &a_props);

        // Remove target and re-add with modified props (unless destroyed).
        self.inventory.remove_one(target_slot as usize);
        if !target_destroyed && !self.inventory.add_with_props(target.kind, a_props) {
            // Inventory full — undo everything.
            // Invariant: remove_one never mutates props on the remaining
            // stack, so add_with_props with original props always re-stacks.
            let ok = self.inventory.add_with_props(target.kind, target.props);
            debug_assert!(ok, "undo target re-insert must succeed");
            if source_removed {
                let ok = self.inventory.add_with_props(source.kind, source.props);
                debug_assert!(ok, "undo source re-insert must succeed");
            }
            self.log.add_event(GameEvent::InventoryFull);
            return false;
        }

        // Update surviving non-removed source's props.
        if !source_removed {
            self.inventory.set_props(source_slot as usize, b_props);
        }

        self.log.add_event(GameEvent::CombineItems {
            target: target.kind,
            source: source.kind,
        });
        if target_destroyed {
            self.log
                .add_event(GameEvent::ItemDestroyed { kind: target.kind });
        }
        if source_destroyed {
            self.log
                .add_event(GameEvent::ItemDestroyed { kind: source.kind });
        }

        true
    }

    /// Drop an item from inventory onto the ground.
    fn drop_item(&mut self, slot: u8) -> bool {
        if let Some(kind) = self.inventory.remove_one(slot as usize) {
            let px = self.entities[0].x;
            let py = self.entities[0].y;
            self.ground_items.push(Item { x: px, y: py, kind });
            self.log.add_event(GameEvent::DropItem { kind });
            return true;
        }
        false
    }

    /// Equip an item from inventory (weapon or armor).
    fn equip_item(&mut self, slot: u8) -> bool {
        let inv_slot = match self.inventory.get(slot as usize) {
            Some(s) => *s,
            None => return false,
        };
        let kind = inv_slot.kind;
        let props = inv_slot.props;

        if rules_items::is_weapon(kind) {
            self.inventory.remove_one(slot as usize);
            if let Some(old) = self.equipment.weapon {
                self.inventory
                    .add_with_props(old, self.equipment.weapon_props);
            }
            self.equipment.weapon = Some(kind);
            self.equipment.weapon_props = props;
            self.log.add_event(GameEvent::EquipWeapon {
                kind,
                bonus: rules_items::attack_from_bag(&props),
            });
            true
        } else if rules_items::is_armor(kind) {
            self.inventory.remove_one(slot as usize);
            if let Some(old) = self.equipment.armor {
                self.inventory
                    .add_with_props(old, self.equipment.armor_props);
            }
            self.equipment.armor = Some(kind);
            self.equipment.armor_props = props;
            self.log.add_event(GameEvent::EquipArmor {
                kind,
                bonus: rules_items::defense_from_bag(&props),
            });
            true
        } else {
            false
        }
    }

    /// Unequip the current weapon, returning it to inventory.
    fn unequip_weapon(&mut self) -> bool {
        if let Some(kind) = self.equipment.weapon {
            if !self
                .inventory
                .add_with_props(kind, self.equipment.weapon_props)
            {
                self.log.add_event(GameEvent::InventoryFull);
                return false;
            }
            self.equipment.weapon = None;
            self.equipment.weapon_props = crate::rules::properties::EMPTY;
            self.log.add_event(GameEvent::UnequipWeapon { kind });
            true
        } else {
            false
        }
    }

    /// Unequip the current armor, returning it to inventory.
    fn unequip_armor(&mut self) -> bool {
        if let Some(kind) = self.equipment.armor {
            if !self
                .inventory
                .add_with_props(kind, self.equipment.armor_props)
            {
                self.log.add_event(GameEvent::InventoryFull);
                return false;
            }
            self.equipment.armor = None;
            self.equipment.armor_props = crate::rules::properties::EMPTY;
            self.log.add_event(GameEvent::UnequipArmor { kind });
            true
        } else {
            false
        }
    }

    /// Drop an equipped weapon directly to the ground (bypasses inventory).
    /// Note: ground Item doesn't carry a PropertyBag yet — bag is lost on drop.
    fn drop_equipped_weapon(&mut self) -> bool {
        if let Some(kind) = self.equipment.weapon.take() {
            self.equipment.weapon_props = crate::rules::properties::EMPTY;
            let px = self.entities[0].x;
            let py = self.entities[0].y;
            self.ground_items.push(Item { x: px, y: py, kind });
            self.log.add_event(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    /// Drop equipped armor directly to the ground (bypasses inventory).
    /// Note: ground Item doesn't carry a PropertyBag yet — bag is lost on drop.
    fn drop_equipped_armor(&mut self) -> bool {
        if let Some(kind) = self.equipment.armor.take() {
            self.equipment.armor_props = crate::rules::properties::EMPTY;
            let px = self.entities[0].x;
            let py = self.entities[0].y;
            self.ground_items.push(Item { x: px, y: py, kind });
            self.log.add_event(GameEvent::DropItem { kind });
            true
        } else {
            false
        }
    }

    /// Build inventory display strings for `GameObservation`.
    fn build_inventory_strings(&self) -> (Vec<String>, Vec<GameColor>) {
        let mut strings = Vec::new();
        let mut colors = Vec::new();
        for (i, slot) in self.inventory.iter() {
            let letter = (b'a' + i as u8) as char;
            let name = item::described_item_name(slot.kind, &slot.props);
            if slot.count > 1 {
                strings.push(format!("{}) {} (x{})", letter, name, slot.count));
            } else {
                strings.push(format!("{}) {}", letter, name));
            }
            colors.push(rules_items::color(slot.kind));
        }
        (strings, colors)
    }

    /// Player's effective attack (base + equipment).
    pub fn effective_attack(&self) -> Stat {
        use crate::rules::damage::{effective_attack as eff_atk, narrow};
        eff_atk(
            narrow(self.entities[0].attack),
            self.equipment.attack_bonus(),
        ) as Stat
    }

    /// Player's effective defense (base + equipment).
    pub fn effective_defense(&self) -> Stat {
        use crate::rules::damage::{effective_defense as eff_def, narrow};
        eff_def(
            narrow(self.entities[0].defense),
            self.equipment.defense_bonus(),
        ) as Stat
    }

    /// Attempt to descend stairs. Returns `true` if successful (turn consumed).
    ///
    /// The player must be standing on a `StairsDown` tile. On success:
    /// - Depth increments by 1
    /// - If past target_depth: game is won
    /// - Otherwise: generate a new floor, spawn monsters with depth scaling,
    ///   preserve player HP/equipment, reset exploration
    pub fn descend(&mut self) -> bool {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        let idx = self.map.idx(px, py);
        if self.map.tiles[idx] != map::Tile::StairsDown {
            self.log.add_event(GameEvent::NoStairs);
            return false;
        }

        self.depth += 1;

        if self.depth > self.target_depth {
            self.game_won = true;
            self.log.add_event(GameEvent::Victory {
                depth: self.target_depth as u8,
            });
            return true;
        }

        // Derive deterministic seed for this floor (XOR-multiply decorrelates adjacent seeds).
        let floor_seed = self.seed ^ (self.depth as u64).wrapping_mul(0x9E37);

        let DerivedRngs {
            map: mut map_rng,
            spawn: mut spawn_rng,
            wandering_seed,
            item: mut item_rng,
        } = DerivedRngs::from_seed(floor_seed);

        // Generate new map with same dimensions.
        let width = self.map.width;
        let height = self.map.height;
        let mut new_map = map::Map::new(width, height);

        let (new_px, new_py) = if let Some(preset) = self.preset {
            new_map.from_preset(preset, &mut map_rng)
        } else {
            new_map.generate(
                self.max_rooms,
                self.room_size_min,
                self.room_size_max,
                &mut map_rng,
            )
        };
        new_map.compute_structural_walls();
        new_map.place_stairs_down();

        // Spawn monsters and apply depth scaling.
        let spawn_table = &self.wandering_spawn_table;
        let mut monsters = spawn::spawn_monsters(
            &new_map,
            spawn_table,
            self.max_monsters_per_room,
            &mut spawn_rng,
        );
        for m in &mut monsters {
            self.apply_depth_scaling(m);
        }

        // Spawn items on new floor.
        let ground_items = spawn::spawn_items(
            &new_map,
            item::MAX_ITEMS_PER_ROOM,
            self.depth as u8,
            &mut item_rng,
        );

        // Preserve player entity with current HP/stats, move to new start.
        self.entities[0].x = new_px;
        self.entities[0].y = new_py;
        // Replace entities: keep player, add new monsters.
        self.entities.truncate(1);
        self.entities.extend(monsters);

        // Replace map and reset exploration.
        self.map = new_map;
        self.ground_items = ground_items;
        self.visible = fov::compute_fov(&self.map, new_px, new_py, self.fov_radius);
        self.explored = self.visible.clone();

        // Reset wandering state for new floor.
        self.wandering_seed = wandering_seed;
        self.wandering_spawned = 0;
        self.idle_count = 0;

        self.log.add_event(GameEvent::Descend {
            depth: self.depth as u8,
            target: self.target_depth as u8,
        });

        true
    }

    /// Dispatch a game command. Returns `true` if the player took an action
    /// (i.e. a turn was consumed), `false` otherwise.
    pub fn handle_command(&mut self, cmd: GameCommand) -> bool {
        match cmd {
            GameCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                self.player_move_or_attack(dx, dy)
            }
            GameCommand::Wait => true,
            GameCommand::Descend => self.descend(),
            GameCommand::Pickup => self.pickup_item(),
            GameCommand::UseItem(slot) => self.use_item(slot),
            GameCommand::DropItem(slot) => self.drop_item(slot),
            GameCommand::EquipItem(slot) => self.equip_item(slot),
            GameCommand::UnequipWeapon => self.unequip_weapon(),
            GameCommand::UnequipArmor => self.unequip_armor(),
            GameCommand::DropEquippedWeapon => self.drop_equipped_weapon(),
            GameCommand::DropEquippedArmor => self.drop_equipped_armor(),
            GameCommand::Combine(target, source) => self.combine_items(target, source),
            // Autorun, AutoExplore, Look, Help are handled at a higher level (main loop / MCP).
            GameCommand::Autorun(_)
            | GameCommand::AutoExplore
            | GameCommand::OpenInventory
            | GameCommand::Look
            | GameCommand::Help
            | GameCommand::MessageHistory
            | GameCommand::Quit => false,
        }
    }

    /// Heal the player by 1 HP if enough turns have passed (NetHack-style regen).
    fn apply_regen(&mut self) {
        let player = &mut self.entities[0];
        if player.alive && player.hp < player.max_hp && self.turn_count % self.regen_interval == 0 {
            player.hp += 1;
        }
    }

    /// Attempt to spawn a wandering monster if conditions are met.
    ///
    /// Checks: grace period, spawn interval (with idle acceleration),
    /// Apply depth-based stat scaling to a monster entity.
    fn apply_depth_scaling(&self, entity: &mut Entity) {
        let steps = (self.depth - 1) / self.depth_scaling.depth_scale_interval;
        let bonus_hp = steps * self.depth_scaling.monster_hp_per_floor;
        let bonus_atk = steps * self.depth_scaling.monster_atk_per_floor;
        entity.hp += bonus_hp;
        entity.max_hp += bonus_hp;
        entity.attack += bonus_atk;
    }

    /// random chance, and wandering cap. Spawns offscreen in a random room.
    // FUTURE: Replace with SoundEvent for acoustic propagation system.
    fn try_spawn_wandering(&mut self, rng: &mut impl rand::Rng) {
        // Read config fields into locals to avoid borrow conflicts with &mut self.
        // All fields are Copy (Stat/Coord = i32), so this is zero-cost.
        let grace_period = self.wandering_config.grace_period;
        let spawn_interval = self.wandering_config.spawn_interval;
        let idle_threshold = self.wandering_config.idle_threshold;
        let idle_acceleration = self.wandering_config.idle_acceleration;
        let spawn_chance = self.wandering_config.spawn_chance;
        let max_wandering = self.wandering_config.max_wandering;

        if self.turn_count < grace_period {
            return;
        }

        let interval = if self.idle_count >= idle_threshold {
            (spawn_interval / idle_acceleration).max(1)
        } else {
            spawn_interval
        };
        if interval <= 0 || self.turn_count % interval != 0 {
            return;
        }
        if rng.gen_range(0..100) >= spawn_chance {
            return;
        }

        // Respect absolute entity budget (future: read from SimBudget).
        if self.entities.len() >= crate::types::MAX_ENTITIES {
            return;
        }

        // Cap alive wandering monsters (Wander AI = not yet seen player).
        let wander_alive = self
            .entities
            .iter()
            .skip(1)
            .filter(|e| e.alive && e.ai == crate::entity::AiBehavior::Wander)
            .count() as Stat;
        if wander_alive >= max_wandering {
            return;
        }

        if let Some((sx, sy)) = self.pick_offscreen_spawn_pos(rng)
            && let Some(mut entity) = spawn::pick_monster(&self.wandering_spawn_table, rng)
        {
            entity.x = sx;
            entity.y = sy;
            entity.ai = crate::entity::AiBehavior::Wander;
            self.apply_depth_scaling(&mut entity);
            self.emit_spawn_sound_cue(sx, sy);
            self.entities.push(entity);
            self.wandering_spawned += 1;
        }
    }

    /// Pick a random floor tile in a room the player isn't in,
    /// outside the player's FOV and not occupied by another entity.
    fn pick_offscreen_spawn_pos(&self, rng: &mut impl rand::Rng) -> Option<(Coord, Coord)> {
        if self.map.rooms.is_empty() {
            return None;
        }

        let px = self.entities[0].x;
        let py = self.entities[0].y;

        for _ in 0..10 {
            let room_idx = rng.gen_range(0..self.map.rooms.len());
            let room = &self.map.rooms[room_idx];
            // Skip rooms the player is standing in.
            if room.contains_interior(px, py) {
                continue;
            }
            // Pick a random floor tile inside the room.
            let width = room.x2 - room.x1 - 1;
            let height = room.y2 - room.y1 - 1;
            if width <= 0 || height <= 0 {
                continue;
            }
            let sx = room.x1 + 1 + rng.gen_range(0..width);
            let sy = room.y1 + 1 + rng.gen_range(0..height);
            if !self.map.is_walkable(sx, sy) {
                continue;
            }
            if self.visible.contains(&(sx, sy)) {
                continue;
            }
            if self.entity_at(sx, sy).is_some() {
                continue;
            }
            return Some((sx, sy));
        }
        None
    }

    /// Emit a distance-based sound cue when a wandering monster spawns.
    // FUTURE: Replace with SoundEvent for acoustic propagation system.
    fn emit_spawn_sound_cue(&mut self, sx: Coord, sy: Coord) {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        let dist = (px - sx).abs() + (py - sy).abs();
        let cfg = &self.wandering_config;

        let distance = if dist <= cfg.sound_near {
            Some(SoundDistance::Near)
        } else if dist <= cfg.sound_medium {
            Some(SoundDistance::Medium)
        } else if dist <= cfg.sound_far {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add_event(GameEvent::SoundCue { distance });
        }
    }

    /// Emit distance-based ambient sound cues for nearby wandering monsters.
    ///
    /// Rate-limited: at most 1 cue per turn, only every 5 turns, prioritizing
    /// the closest wanderer.
    // FUTURE: Replace with SoundEvent for acoustic propagation system.
    fn emit_ambient_sound_cues(&mut self) {
        if self.turn_count % 5 != 0 {
            return;
        }

        let px = self.entities[0].x;
        let py = self.entities[0].y;
        let cfg = &self.wandering_config;

        // Find the closest alive wandering monster.
        let mut closest_dist = Coord::MAX;
        for e in self.entities.iter().skip(1) {
            if e.alive && e.ai == crate::entity::AiBehavior::Wander {
                let dist = (px - e.x).abs() + (py - e.y).abs();
                if dist < closest_dist {
                    closest_dist = dist;
                }
            }
        }

        let distance = if closest_dist <= cfg.sound_near {
            Some(SoundDistance::Near)
        } else if closest_dist <= cfg.sound_medium {
            Some(SoundDistance::Medium)
        } else if closest_dist <= cfg.sound_far {
            Some(SoundDistance::Far)
        } else {
            None
        };
        if let Some(distance) = distance {
            self.log.add_event(GameEvent::SoundCue { distance });
        }
    }

    /// Execute one complete game step: player command, FOV update, monster turns.
    ///
    /// This is the atomic turn operation used by the MCP server and any other
    /// non-terminal consumer. It bundles the logic that `main.rs` performs
    /// across multiple calls into a single method.
    pub fn step(&mut self, cmd: GameCommand) -> StepResult {
        let msg_count_before = self.log.len();
        let is_wait = matches!(cmd, GameCommand::Wait);
        let action_taken = self.handle_command(cmd);

        if action_taken {
            self.dirty = true;
            if is_wait {
                self.idle_count += 1;
            } else {
                self.idle_count = 0;
            }
            self.update_fov();

            // Skip monster turns and spawning if the player just won.
            if !self.game_won {
                let mut turn_rng =
                    StdRng::seed_from_u64(self.wandering_seed.wrapping_add(self.turn_count as u64));
                let player_def = self.effective_defense();
                if ai::run_monster_turns(
                    &mut self.entities,
                    &self.map,
                    &mut self.log,
                    &mut turn_rng,
                    player_def,
                ) {
                    self.game_over = true;
                }
                self.turn_count += 1;
                self.try_spawn_wandering(&mut turn_rng);
                self.emit_ambient_sound_cues();
                self.apply_regen();
            }
        }

        StepResult {
            action_taken,
            new_messages: self.log.messages_since(msg_count_before),
            game_over: self.game_over,
            game_won: self.game_won,
        }
    }

    /// Check if any stairs tile is currently visible.
    fn stairs_in_visible(&self) -> bool {
        self.visible
            .iter()
            .any(|&(x, y)| self.map.tiles[self.map.idx(x, y)] == map::Tile::StairsDown)
    }

    /// Create a stepper for directional autorun.
    pub fn start_autorun(&self, dir: Direction) -> AutorunStepper {
        AutorunStepper {
            mode: StepperMode::Directional(dir),
            steps_taken: 0,
            max_steps: self.max_autorun_steps,
            all_messages: Vec::new(),
            explored_before: self.explored.len() as Stat,
            stairs_visible_before: self.stairs_in_visible(),
        }
    }

    /// Create a stepper that follows an A* path to (tx, ty).
    pub fn start_pathfind(&self, tx: Coord, ty: Coord) -> Result<AutorunStepper, String> {
        let px = self.entities[0].x;
        let py = self.entities[0].y;

        let path = pathfinding::find_path(&self.map, px, py, tx, ty, &self.explored)
            .ok_or_else(|| "No path found to target.".to_string())?;

        Ok(AutorunStepper {
            mode: StepperMode::FollowPath { path, index: 0 },
            steps_taken: 0,
            max_steps: self.max_autorun_steps,
            all_messages: Vec::new(),
            explored_before: self.explored.len() as Stat,
            stairs_visible_before: self.stairs_in_visible(),
        })
    }

    /// Create a stepper for auto-explore: pick nearest frontier, pathfind to it.
    ///
    /// Uses Dijkstra to find the frontier tile with the lowest actual walking
    /// cost, respecting dungeon topology (walls, corridors, tile costs).
    /// Returns the stepper and the target (x, y) coordinates.
    pub fn start_auto_explore(&self) -> Result<(AutorunStepper, Coord, Coord), String> {
        let frontiers = self.frontier_tiles();
        if frontiers.is_empty() {
            return Err("No unexplored areas — map is fully explored.".to_string());
        }

        let px = self.entities[0].x;
        let py = self.entities[0].y;

        let frontier_set: HashSet<Pos> = frontiers.into_iter().collect();
        let (tx, ty) =
            pathfinding::nearest_by_cost(&self.map, px, py, &frontier_set, &self.explored)
                .ok_or_else(|| "No reachable frontier tiles.".to_string())?;

        let stepper = self.start_pathfind(tx, ty)?;
        Ok((stepper, tx, ty))
    }

    /// Run in a direction until something interesting happens.
    ///
    /// Convenience wrapper around the stepper — runs to completion in one call.
    pub fn autorun(&mut self, dir: Direction) -> AutorunResult {
        let stepper = self.start_autorun(dir);
        stepper.run_to_completion(self)
    }

    /// Walk the shortest path to (tx, ty) using A* pathfinding.
    ///
    /// Convenience wrapper around the stepper — runs to completion in one call.
    pub fn pathfind_to(&mut self, tx: Coord, ty: Coord) -> Result<AutorunResult, String> {
        let stepper = self.start_pathfind(tx, ty)?;
        // Preserve empty-path early return for behavioral parity.
        if let StepperMode::FollowPath { ref path, .. } = stepper.mode
            && path.is_empty()
        {
            return Ok(AutorunResult {
                steps_taken: 0,
                stop_reason: AutorunStopReason::PathComplete,
                messages: Vec::new(),
                new_tiles_revealed: 0,
            });
        }
        Ok(stepper.run_to_completion(self))
    }

    /// Automatically explore: find the nearest frontier tile and pathfind to it.
    ///
    /// Convenience wrapper around the stepper — runs to completion in one call.
    pub fn auto_explore(&mut self) -> Result<AutoExploreResult, String> {
        let (stepper, tx, ty) = self.start_auto_explore()?;
        let movement = stepper.run_to_completion(self);
        Ok(AutoExploreResult {
            target_x: tx,
            target_y: ty,
            movement,
        })
    }

    /// Fight an adjacent monster to the death in one call.
    ///
    /// Picks the adjacent monster with the lowest HP (quickest kill).
    /// Each round is a full `step()`, so other monsters still act.
    /// Stops when the target dies, the player dies, or the target moves away.
    pub fn auto_fight(&mut self) -> Result<AutoFightResult, String> {
        let px = self.entities[0].x;
        let py = self.entities[0].y;

        let target_idx = self
            .entities
            .iter()
            .enumerate()
            .filter(|(i, e)| *i != 0 && e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
            .min_by_key(|(_, e)| e.hp)
            .map(|(i, _)| i)
            .ok_or_else(|| "No adjacent monster to fight.".to_string())?;

        let hp_before = self.entities[0].hp;
        let target_name = self.entities[target_idx].name.clone();
        let msg_count_before = self.log.len();
        let mut rounds = 0;

        loop {
            if !self.entities[target_idx].alive {
                break;
            }

            // Recompute direction to target each round (safe if target moves)
            let tx = self.entities[target_idx].x;
            let ty = self.entities[target_idx].y;
            // Target moved out of melee range — stop
            if (tx - self.entities[0].x).abs() > 1 || (ty - self.entities[0].y).abs() > 1 {
                break;
            }

            let cmd = GameCommand::move_or_wait(tx - self.entities[0].x, ty - self.entities[0].y);
            let result = self.step(cmd);
            rounds += 1;

            if result.game_over {
                break;
            }
        }

        Ok(AutoFightResult {
            rounds,
            target_name,
            target_killed: !self.entities[target_idx].alive,
            player_hp_lost: hp_before - self.entities[0].hp,
            messages: self.log.messages_since(msg_count_before),
        })
    }

    /// True if any living monster is adjacent to the player (within 1 tile).
    pub fn has_adjacent_monster(&self) -> bool {
        let px = self.entities[0].x;
        let py = self.entities[0].y;
        self.entities
            .iter()
            .skip(1)
            .any(|e| e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
    }

    /// Set of entity indices for living, visible monsters.
    pub fn visible_monster_ids(&self) -> HashSet<usize> {
        self.entities
            .iter()
            .enumerate()
            .filter(|(i, e)| *i != 0 && e.alive && self.visible.contains(&(e.x, e.y)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Produce a snapshot of the current visible game state.
    ///
    /// Returns structured data suitable for JSON serialization. The ASCII map
    /// shows only visible tiles (non-visible positions are spaces) with entity
    /// glyphs overlaid, matching the terminal renderer's behavior.
    pub fn observe(&self) -> GameObservation {
        let player = &self.entities[0];
        let (inv_strings, inv_colors) = self.build_inventory_strings();

        // Build ASCII map — only rows with visible content
        let mut map_lines = Vec::new();
        for y in 0..self.map.height {
            let mut line = String::with_capacity(self.map.width as usize);
            let mut has_content = false;

            for x in 0..self.map.width {
                if self.visible.contains(&(x, y)) {
                    has_content = true;
                    // Check for entities/items (alive first, then dead, then items)
                    if let Some(glyph) = self.glyph_at(x, y) {
                        line.push(glyph);
                    } else {
                        match self.map.tiles[self.map.idx(x, y)] {
                            map::Tile::Floor => line.push('.'),
                            map::Tile::Wall => line.push('#'),
                            map::Tile::StairsDown => line.push('>'),
                        }
                    }
                } else {
                    line.push(' ');
                }
            }

            if has_content {
                map_lines.push(line.trim_end().to_string());
            }
        }

        // Visible entities (excluding player)
        let visible_entities: Vec<EntityInfo> = self
            .entities
            .iter()
            .filter(|e| e.kind != EntityKind::Player && self.visible.contains(&(e.x, e.y)))
            .map(|e| EntityInfo {
                name: e.name.clone(),
                glyph: if e.alive { e.glyph } else { '%' },
                x: e.x,
                y: e.y,
                hp: e.hp,
                max_hp: e.max_hp,
                alive: e.alive,
            })
            .collect();

        // Visible items on ground
        let visible_items: Vec<ItemInfo> = self
            .ground_items
            .iter()
            .filter(|it| self.visible.contains(&(it.x, it.y)))
            .map(|it| ItemInfo {
                name: item::item_name(it.kind).to_string(),
                glyph: item::item_glyph(it.kind),
                x: it.x,
                y: it.y,
            })
            .collect();

        // --- game stats ---
        let kills = self.kill_count();
        let rooms_found = self
            .map
            .rooms
            .iter()
            .filter(|r| self.explored.contains(&r.center()))
            .count() as Stat;
        let explored_pct = self.explored_pct();

        // Stairs position — report if the stairs tile has been explored.
        let stairs = self
            .explored
            .iter()
            .find(|&&(x, y)| self.map.tiles[self.map.idx(x, y)] == map::Tile::StairsDown)
            .copied();

        GameObservation {
            player_hp: player.hp,
            player_max_hp: player.max_hp,
            player_atk: self.effective_attack(),
            player_def: self.effective_defense(),
            player_x: player.x,
            player_y: player.y,
            map_ascii: map_lines,
            visible_entities,
            visible_items,
            recent_messages: self.log.recent(10),
            game_over: self.game_over,
            turn_count: self.turn_count,
            weapon: self
                .equipment
                .weapon
                .map(|k| item::described_item_name(k, &self.equipment.weapon_props)),
            armor: self
                .equipment
                .armor
                .map(|k| item::described_item_name(k, &self.equipment.armor_props)),
            kills,
            rooms_found,
            explored_pct,
            inventory: inv_strings,
            inventory_colors: inv_colors,
            seed: self.seed,
            seed_code: self.seed_code(),
            depth: self.depth,
            target_depth: self.target_depth,
            game_won: self.game_won,
            stairs,
        }
    }

    /// Extract lightweight metadata for save-slot display.
    pub fn extract_metadata(&self) -> crate::saves::SlotMetadata {
        let player = &self.entities[0];
        crate::saves::SlotMetadata {
            turn_count: self.turn_count,
            player_hp: player.hp,
            player_max_hp: player.max_hp,
            explored_pct: self.explored_pct(),
            player_name: None,
            depth: self.depth,
        }
    }

    /// Number of dead (non-player) entities.
    pub fn kill_count(&self) -> Stat {
        self.entities.iter().skip(1).filter(|e| !e.alive).count() as Stat
    }

    /// Percentage of floor tiles the player has explored (0–100).
    pub fn explored_pct(&self) -> Stat {
        let floor_count = self.map.known_floor_count();
        if floor_count == 0 {
            return 0;
        }
        let explored_floors = self
            .explored
            .iter()
            .filter(|&&(x, y)| {
                self.map.in_bounds(x, y) && self.map.tiles[self.map.idx(x, y)].is_walkable()
            })
            .count() as Stat;
        (explored_floors * 100) / floor_count
    }

    /// Find frontier tiles: explored floor tiles adjacent to at least one
    /// unexplored tile. These mark the boundary of explored territory and
    /// indicate where further exploration is possible.
    pub fn frontier_tiles(&self) -> Vec<Pos> {
        self.explored
            .iter()
            .filter(|&&(x, y)| {
                self.map.is_walkable(x, y)
                    && (-1..=1i32).any(|dy| {
                        (-1..=1i32).any(|dx| {
                            (dx != 0 || dy != 0)
                                && self.map.in_bounds(x + dx, y + dy)
                                && !self.explored.contains(&(x + dx, y + dy))
                        })
                    })
            })
            .copied()
            .collect()
    }

    /// Produce an ASCII map of all explored tiles.
    ///
    /// Unlike `observe()`, which only shows currently visible tiles, this
    /// renders every tile the player has ever seen. Entity glyphs are only
    /// shown at their current positions if visible (no stale positions).
    /// Frontier tiles (explored floor adjacent to unexplored) are rendered
    /// as `~` to highlight exploration boundaries. Rows with no explored
    /// content are omitted.
    pub fn explored_map(&self) -> Vec<String> {
        let frontiers: HashSet<Pos> = self.frontier_tiles().into_iter().collect();
        let mut lines = Vec::new();
        for y in 0..self.map.height {
            let mut line = String::with_capacity(self.map.width as usize);
            let mut has_content = false;

            for x in 0..self.map.width {
                if self.explored.contains(&(x, y)) {
                    has_content = true;
                    // Show entity glyphs only if currently visible.
                    if self.visible.contains(&(x, y))
                        && let Some(glyph) = self.glyph_at(x, y)
                    {
                        line.push(glyph);
                        continue;
                    }
                    if frontiers.contains(&(x, y)) {
                        line.push('~');
                    } else {
                        match self.map.tiles[self.map.idx(x, y)] {
                            map::Tile::Floor => line.push('.'),
                            map::Tile::Wall => line.push('#'),
                            map::Tile::StairsDown => line.push('>'),
                        }
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

    /// Query tile information at (x, y) for look mode.
    ///
    /// Returns terrain, entity info (only if visible), and display glyph.
    /// Out-of-bounds or unexplored tiles return appropriate defaults.
    pub fn look_at(&self, x: Coord, y: Coord) -> TileInfo {
        self.look_at_inner(x, y, &LookOptions::default())
    }

    /// Like `look_at`, but with configurable reveal options.
    ///
    /// Use this when the caller has extra visibility (e.g. dev-tools
    /// RevealMonsters overlay). Corpses still require normal visibility.
    pub fn look_at_with(&self, x: Coord, y: Coord, opts: &LookOptions) -> TileInfo {
        self.look_at_inner(x, y, opts)
    }

    fn look_at_inner(&self, x: Coord, y: Coord, opts: &LookOptions) -> TileInfo {
        if !self.map.in_bounds(x, y) {
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

        let explored = self.explored.contains(&(x, y));
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

        let visible = self.visible.contains(&(x, y));
        let tile = self.map.tiles[self.map.idx(x, y)];
        let terrain = match tile {
            map::Tile::Floor => "Floor".into(),
            map::Tile::Wall => "Wall".into(),
            map::Tile::StairsDown => "Stairs down".into(),
        };

        // Show entity info if visible, or if reveal_monsters is active (alive only).
        let show_entity = visible || opts.reveal_monsters;
        let entity = if show_entity {
            // Check alive entities first (higher priority).
            if let Some(idx) = self
                .entities
                .iter()
                .position(|e| e.alive && e.x == x && e.y == y)
            {
                let e = &self.entities[idx];
                Some(EntityInfo {
                    name: e.name.clone(),
                    glyph: e.glyph,
                    x: e.x,
                    y: e.y,
                    hp: e.hp,
                    max_hp: e.max_hp,
                    alive: true,
                })
            } else if visible {
                // Corpses only shown when normally visible (not via reveal).
                self.entities
                    .iter()
                    .find(|e| !e.alive && e.x == x && e.y == y)
                    .map(|e| EntityInfo {
                        name: e.name.clone(),
                        glyph: '%',
                        x: e.x,
                        y: e.y,
                        hp: e.hp,
                        max_hp: e.max_hp,
                        alive: false,
                    })
            } else {
                None
            }
        } else {
            None
        };

        // Show entity glyph if visible, or if we found a revealed monster.
        let glyph = if visible {
            self.glyph_at(x, y).unwrap_or(match tile {
                map::Tile::Floor => '.',
                map::Tile::Wall => '#',
                map::Tile::StairsDown => '>',
            })
        } else if let Some(e) = entity.as_ref() {
            // Revealed monster — show its glyph.
            e.glyph
        } else {
            match tile {
                map::Tile::Floor => '.',
                map::Tile::Wall => '#',
                map::Tile::StairsDown => '>',
            }
        };

        // Items on this tile (only when visible).
        let items = if visible {
            self.ground_items
                .iter()
                .filter(|it| it.x == x && it.y == y)
                .map(|it| ItemInfo {
                    name: item::item_name(it.kind).to_string(),
                    glyph: item::item_glyph(it.kind),
                    x: it.x,
                    y: it.y,
                })
                .collect()
        } else {
            Vec::new()
        };

        TileInfo {
            x,
            y,
            terrain,
            entity,
            items,
            visible,
            explored,
            glyph,
        }
    }

    /// Get the display glyph for the topmost entity at (x, y).
    /// Living entities take priority over dead ones (corpses).
    fn glyph_at(&self, x: Coord, y: Coord) -> Option<char> {
        // Alive entity on top
        if let Some(idx) = self
            .entities
            .iter()
            .position(|e| e.alive && e.x == x && e.y == y)
        {
            return Some(self.entities[idx].glyph);
        }
        // Dead entity as corpse
        if self
            .entities
            .iter()
            .any(|e| !e.alive && e.x == x && e.y == y)
        {
            return Some('%');
        }
        // Item on ground
        if let Some(it) = self.ground_items.iter().find(|it| it.x == x && it.y == y) {
            return Some(item::item_glyph(it.kind));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// GameView implementation
// ---------------------------------------------------------------------------

impl crate::rules::game_view::GameView for GameState {
    fn map_dims(&self) -> (i32, i32) {
        (self.map.width, self.map.height)
    }
    fn map_in_bounds(&self, x: i32, y: i32) -> bool {
        self.map.in_bounds(x, y)
    }
    fn tile_at(&self, x: i32, y: i32) -> u8 {
        let idx = self.map.idx(x, y);
        match self.map.tiles[idx] {
            map::Tile::Wall if self.map.structural[idx] => 1, // TILE_STRUCTURAL
            map::Tile::Wall => 0,                              // TILE_WALL
            map::Tile::Floor => 2,                             // TILE_FLOOR
            map::Tile::StairsDown => 3,                        // TILE_STAIRS_DOWN
        }
    }
    fn is_visible(&self, x: i32, y: i32) -> bool {
        self.visible.contains(&(x, y))
    }
    fn is_explored(&self, x: i32, y: i32) -> bool {
        self.explored.contains(&(x, y))
    }
    fn player_xy(&self) -> (i32, i32) {
        (self.entities[0].x, self.entities[0].y)
    }
    fn player_hp(&self) -> (u8, u8) {
        (self.entities[0].hp as u8, self.entities[0].max_hp as u8)
    }
    fn effective_attack(&self) -> u8 {
        self.effective_attack() as u8
    }
    fn effective_defense(&self) -> u8 {
        self.effective_defense() as u8
    }
    fn entity_count(&self) -> usize {
        self.entities.len()
    }
    fn entity_xy(&self, i: usize) -> (i32, i32) {
        (self.entities[i].x, self.entities[i].y)
    }
    fn entity_alive(&self, i: usize) -> bool {
        self.entities[i].alive
    }
    fn entity_kind(&self, i: usize) -> Option<crate::rules::monster_table::MonsterKind> {
        self.entities[i].monster_kind
    }
    fn entity_hp(&self, i: usize) -> (u8, u8) {
        (self.entities[i].hp as u8, self.entities[i].max_hp as u8)
    }
    fn entity_at(&self, x: i32, y: i32) -> Option<u8> {
        self.entity_at(x, y).map(|i| i as u8)
    }
    fn item_count(&self) -> usize {
        self.ground_items.len()
    }
    fn item_xy(&self, i: usize) -> (i32, i32) {
        (self.ground_items[i].x, self.ground_items[i].y)
    }
    fn item_alive(&self, _i: usize) -> bool {
        true // Standard tier removes dead items from the Vec
    }
    fn item_kind_at(&self, i: usize) -> crate::rules::items::ItemKind {
        self.ground_items[i].kind
    }
    fn item_at(&self, x: i32, y: i32) -> Option<u8> {
        self.ground_items
            .iter()
            .position(|it| it.x == x && it.y == y)
            .map(|i| i as u8)
    }
    fn equipment(&self) -> &crate::rules::items::Equipment {
        &self.equipment
    }
    fn inventory(&self) -> &crate::rules::items::Inventory {
        &self.inventory
    }
    fn depth(&self) -> u8 {
        self.depth as u8
    }
    fn kills(&self) -> u8 {
        self.kill_count() as u8
    }
    fn turn_count(&self) -> u16 {
        self.turn_count as u16
    }
    fn game_over(&self) -> bool {
        self.game_over
    }
    fn game_won(&self) -> bool {
        self.game_won
    }
    fn seed_u32(&self) -> u32 {
        self.seed as u32
    }
    fn explored_pct(&self) -> u8 {
        GameState::explored_pct(self) as u8
    }
    fn target_depth(&self) -> u8 {
        self.target_depth as u8
    }
    fn recent_message(&self, n: u8) -> Option<crate::rules::message::GameEvent> {
        self.log.recent_event(n as usize)
    }
    fn step_view(&mut self, cmd: GameCommand) -> crate::rules::game_view::GameViewStep {
        let r = GameState::step(self, cmd);
        crate::rules::game_view::GameViewStep {
            action_taken: r.action_taken,
            game_over: r.game_over,
            game_won: r.game_won,
        }
    }

    // Standard tier has per-entity glyph/color (not derived from MonsterKind).
    fn render_entity(&self, i: usize) -> (char, crate::rules::color::GameColor) {
        let e = &self.entities[i];
        if e.alive {
            (e.glyph, e.color)
        } else {
            ('%', crate::rules::color::GameColor::DarkRed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Entity, EntityKind};
    use crate::item::ItemKind;
    use crate::map::{Map, Tile};
    use crate::rules::balance;

    /// Build a minimal GameState with a custom open map (no random generation).
    fn test_game() -> GameState {
        let mut m = Map::new(20, 20);
        // Carve a 10x10 open area from (1,1) to (10,10)
        for y in 1..=10 {
            for x in 1..=10 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        }
    }

    #[test]
    fn player_is_entities_zero() {
        let gs = test_game();
        assert_eq!(gs.entities[0].kind, EntityKind::Player);
    }

    #[test]
    fn entity_at_finds_living_entity() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.entity_at(6, 5), Some(1));
    }

    #[test]
    fn entity_at_ignores_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(data::goblin(), 6, 5);
        monster.alive = false;
        gs.entities.push(monster);
        assert_eq!(gs.entity_at(6, 5), None);
    }

    #[test]
    fn entity_at_returns_none_for_empty_cell() {
        let gs = test_game();
        assert_eq!(gs.entity_at(3, 3), None);
    }

    #[test]
    fn player_moves_into_open_floor() {
        let mut gs = test_game();
        let acted = gs.player_move_or_attack(1, 0); // move right
        assert!(acted);
        assert_eq!(gs.entities[0].x, 6);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn player_blocked_by_wall() {
        let mut gs = test_game();
        // Move player to edge of open area
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        let acted = gs.player_move_or_attack(-1, 0); // into wall at (0,1)
        assert!(!acted);
        assert_eq!(gs.entities[0].x, 1);
    }

    #[test]
    fn player_attacks_monster() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::orc(), 6, 5);
        let monster_hp = monster.hp;
        gs.entities.push(monster);
        let acted = gs.player_move_or_attack(1, 0); // attack orc at (6,5)
        assert!(acted);
        // Player should not have moved
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
        // Orc should have taken damage (player atk=5, orc def=1, dmg=4)
        assert_eq!(gs.entities[1].hp, monster_hp - 4);
    }

    #[test]
    fn handle_command_move_moves_player() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Move(Direction::East));
        assert!(acted);
        assert_eq!(gs.entities[0].x, 6);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn handle_command_wait_consumes_turn() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Wait);
        assert!(acted);
        // Player should not have moved
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn handle_command_quit_does_not_consume_turn() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Quit);
        assert!(!acted);
    }

    // --- step() tests ---

    #[test]
    fn step_move_advances_turn() {
        let mut gs = test_game();
        let result = gs.step(GameCommand::Move(Direction::East));
        assert!(result.action_taken);
        assert!(!result.game_over);
        assert_eq!(gs.entities[0].x, 6);
    }

    #[test]
    fn step_into_wall_does_not_advance() {
        let mut gs = test_game();
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        let result = gs.step(GameCommand::Move(Direction::West));
        assert!(!result.action_taken);
        assert_eq!(gs.entities[0].x, 1);
    }

    #[test]
    fn step_includes_monster_turn() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let hp_before = gs.entities[0].hp;
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // Monster should have attacked (or moved closer), generating messages
        assert!(!result.new_messages.is_empty());
        // Goblin adjacent → attacks: dmg = max(0, 3 - 2) = 1
        assert_eq!(gs.entities[0].hp, hp_before - 1);
    }

    #[test]
    fn step_reports_game_over_on_death() {
        let mut gs = test_game();
        gs.entities[0].hp = 1;
        gs.entities[0].defense = 0;
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.step(GameCommand::Wait);
        assert!(result.game_over);
        assert!(gs.game_over);
    }

    #[test]
    fn step_captures_only_new_messages() {
        let mut gs = test_game();
        gs.log.add("pre-existing message");
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.step(GameCommand::Wait);
        // new_messages should not include the pre-existing message
        assert!(
            !result
                .new_messages
                .iter()
                .any(|m| m == "pre-existing message")
        );
        assert!(!result.new_messages.is_empty());
    }

    // --- observe() tests ---

    #[test]
    fn observe_includes_player_stats() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.player_hp, 30);
        assert_eq!(obs.player_max_hp, 30);
        assert!(!obs.game_over);
    }

    #[test]
    fn observe_map_contains_player_glyph() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        let map_text = obs.map_ascii.join("\n");
        assert!(map_text.contains('@'));
    }

    #[test]
    fn observe_shows_visible_monsters() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.visible_entities.len(), 1);
        assert_eq!(obs.visible_entities[0].name, "Goblin");
        assert!(obs.visible_entities[0].alive);
    }

    #[test]
    fn observe_hides_non_visible_monsters() {
        let mut gs = test_game();
        // Place monster far away, outside the carved area and FOV
        let monster = Entity::from_template(data::goblin(), 19, 19);
        gs.entities.push(monster);
        gs.update_fov();
        let obs = gs.observe();
        assert!(obs.visible_entities.is_empty());
    }

    #[test]
    fn observe_shows_corpses() {
        let mut gs = test_game();
        let mut corpse = Entity::from_template(data::goblin(), 6, 5);
        corpse.alive = false;
        gs.entities.push(corpse);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.visible_entities.len(), 1);
        assert!(!obs.visible_entities[0].alive);
        assert_eq!(obs.visible_entities[0].glyph, '%');
    }

    #[test]
    fn observe_serializes_to_json() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        let json = serde_json::to_string(&obs);
        assert!(json.is_ok());
    }

    #[test]
    fn observe_includes_recent_messages() {
        let mut gs = test_game();
        gs.update_fov();
        gs.log.add("Test message");
        let obs = gs.observe();
        assert!(obs.recent_messages.iter().any(|m| m == "Test message"));
    }

    // --- glyph_at() tests ---

    #[test]
    fn glyph_at_returns_alive_entity() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('g'));
    }

    #[test]
    fn glyph_at_returns_corpse_for_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(data::goblin(), 6, 5);
        monster.alive = false;
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('%'));
    }

    #[test]
    fn glyph_at_alive_over_dead() {
        let mut gs = test_game();
        let mut dead = Entity::from_template(data::goblin(), 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(data::orc(), 6, 5);
        gs.entities.push(alive);
        // Alive entity should win
        assert_eq!(gs.glyph_at(6, 5), Some('o'));
    }

    #[test]
    fn glyph_at_empty_cell() {
        let gs = test_game();
        assert_eq!(gs.glyph_at(3, 3), None);
    }

    // --- autorun() tests ---

    /// Build a horizontal corridor: floor from (1, 5) to (18, 5), walls everywhere else.
    fn corridor_game() -> GameState {
        let mut m = Map::new(20, 10);
        for x in 1..=18 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        }
    }

    #[test]
    fn autorun_stops_at_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), corridor ends at x=18. Running east should reach x=18
        // and stop because the tile ahead (x=19) is a wall.
        let result = gs.autorun(Direction::East);
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(gs.entities[0].x, 18);
        assert_eq!(result.steps_taken, 13);
    }

    #[test]
    fn autorun_stops_when_monster_spotted() {
        let mut gs = corridor_game();
        // Place a goblin at x=14, just outside FOV radius of 8 from (5,5).
        // After moving east a few tiles, the goblin enters FOV.
        let monster = Entity::from_template(data::goblin(), 14, 5);
        gs.entities.push(monster);
        let result = gs.autorun(Direction::East);
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert!(gs.entities[0].x < 14); // stopped before reaching monster
    }

    #[test]
    fn autorun_stops_when_adjacent_to_monster() {
        let mut gs = corridor_game();
        // Place a goblin adjacent at (6, 5). Autorun should stop immediately
        // because a monster is right next to us — don't auto-attack.
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.autorun(Direction::East);
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert_eq!(result.steps_taken, 0);
        assert_eq!(gs.entities[0].x, 5); // didn't move
    }

    #[test]
    fn autorun_runs_through_junction() {
        let mut m = Map::new(20, 10);
        // Horizontal corridor
        for x in 1..=18 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Add a branch going north at x=10
        for y in 1..=4 {
            let idx = m.idx(10, y);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let result = gs.autorun(Direction::East);
        // Forward is always clear through the junction — runs to the wall.
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(gs.entities[0].x, 18);
    }

    #[test]
    fn autorun_stops_at_t_junction() {
        let mut m = Map::new(20, 10);
        // Horizontal corridor from (1,5) to (9,5)
        for x in 1..=9 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // T-junction at x=10: floor + vertical branches north and south
        let idx = m.idx(10, 5);
        m.tiles[idx] = Tile::Floor;
        for y in 1..=4 {
            let idx = m.idx(10, y);
            m.tiles[idx] = Tile::Floor;
        }
        for y in 6..=8 {
            let idx = m.idx(10, y);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let result = gs.autorun(Direction::East);
        // Wall ahead at (11,5) with north and south alternatives → decision point.
        assert_eq!(result.stop_reason, AutorunStopReason::CorridorBranches);
        assert_eq!(gs.entities[0].x, 10);
    }

    #[test]
    fn autorun_respects_max_steps() {
        // Create a very long corridor
        let mut m = Map::new(200, 10);
        for x in 1..=198 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let result = gs.autorun(Direction::East);
        assert_eq!(result.stop_reason, AutorunStopReason::MaxSteps);
        assert_eq!(result.steps_taken, data::config().max_autorun_steps);
    }

    #[test]
    fn autorun_zero_steps_into_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), run north into wall
        let result = gs.autorun(Direction::North);
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(result.steps_taken, 0);
        assert_eq!(gs.entities[0].x, 5);
        assert_eq!(gs.entities[0].y, 5);
    }

    #[test]
    fn autorun_crosses_room_to_far_wall() {
        // test_game() has a 10x10 open area (room-like). Player at (5,5).
        // Running east should cross the room and hit the wall at x=10.
        // With alternatives along the wall, this is a decision point.
        let mut gs = test_game();
        gs.map.rooms.push(crate::map::Rect::new(0, 0, 11, 11));
        let result = gs.autorun(Direction::East);
        assert_eq!(result.stop_reason, AutorunStopReason::CorridorBranches);
        assert_eq!(gs.entities[0].x, 10);
        assert!(result.steps_taken > 1);
    }

    #[test]
    fn autorun_crosses_room_into_corridor() {
        let mut m = Map::new(30, 10);
        // Room from (1,1) to (10,8)
        let room = crate::map::Rect::new(0, 0, 11, 9);
        m.carve_room(&room);
        m.rooms.push(room);
        // Corridor going east from (11,5)
        for x in 11..=20 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let result = gs.autorun(Direction::East);
        // Crosses room boundary freely, runs corridor to dead end at x=20.
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(gs.entities[0].x, 20);
    }

    #[test]
    fn autorun_crosses_corridor_into_room() {
        let mut m = Map::new(30, 10);
        // Corridor from (1,5) to (10,5)
        for x in 1..=10 {
            let idx = m.idx(x, 5);
            m.tiles[idx] = Tile::Floor;
        }
        // Room from (11,1) to (20,8)
        let room = crate::map::Rect::new(10, 0, 11, 9);
        m.carve_room(&room);
        m.rooms.push(room);

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut gs = GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
        };

        let result = gs.autorun(Direction::East);
        // Crosses corridor into room, reaches far wall at x=20.
        // Wall ahead with floor tiles north/south → decision point.
        assert_eq!(result.stop_reason, AutorunStopReason::CorridorBranches);
        assert_eq!(gs.entities[0].x, 20);
    }

    // --- observe() game stats tests ---

    #[test]
    fn observe_stats_no_monsters() {
        let mut gs = test_game();
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.kills, 0);
        assert!(obs.explored_pct > 0);
    }

    #[test]
    fn observe_stats_with_kills() {
        let mut gs = test_game();
        let mut dead = Entity::from_template(data::goblin(), 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(data::orc(), 7, 5);
        gs.entities.push(alive);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.kills, 1);
    }

    #[test]
    fn observe_stats_rooms_found() {
        // Use a real generated map so we have rooms
        let gs = GameState::new(80, 40);
        let obs = gs.observe();
        // Player starts in first room, so at least 1 room found
        assert!(obs.rooms_found >= 1);
    }

    #[test]
    fn observe_stats_explored_pct_range() {
        let gs = GameState::new(80, 40);
        let obs = gs.observe();
        assert!(obs.explored_pct > 0);
        assert!(obs.explored_pct <= 100);
    }

    // --- explored_map() tests ---

    #[test]
    fn explored_map_includes_explored_tiles() {
        let mut gs = test_game();
        gs.update_fov();
        let map = gs.explored_map();
        let text = map.join("\n");
        // Player should be visible
        assert!(text.contains('@'));
        // Floor and wall tiles should appear
        assert!(text.contains('.'));
        assert!(text.contains('#'));
    }

    #[test]
    fn explored_map_shows_more_than_fov_after_moving() {
        let mut gs = test_game();
        gs.update_fov();
        let initial_explored = gs.explored.len();
        // Move east a few times to explore new tiles
        for _ in 0..3 {
            gs.step(GameCommand::Move(Direction::East));
        }
        assert!(gs.explored.len() > initial_explored);
        let map = gs.explored_map();
        let total_chars: usize = map
            .iter()
            .map(|l| l.chars().filter(|c| *c != ' ').count())
            .sum();
        // Explored map should have more non-space characters than a FOV-only view
        let obs = gs.observe();
        let obs_chars: usize = obs
            .map_ascii
            .iter()
            .map(|l| l.chars().filter(|c| *c != ' ').count())
            .sum();
        assert!(total_chars >= obs_chars);
    }

    #[test]
    fn explored_map_hides_monsters_outside_fov() {
        let mut gs = test_game();
        // Place a monster far from player but technically in explored area
        let monster = Entity::from_template(data::goblin(), 3, 3);
        gs.entities.push(monster);
        gs.update_fov();
        // Verify the monster is in explored area but check if it's in FOV
        if !gs.visible.contains(&(3, 3)) {
            let map = gs.explored_map();
            let text = map.join("\n");
            // Monster glyph should NOT appear since it's outside FOV
            assert!(!text.contains('g'));
        }
    }

    // --- auto_fight() tests ---

    #[test]
    fn auto_fight_kills_goblin() {
        let mut gs = test_game();
        let goblin = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(goblin);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(result.target_killed);
        assert_eq!(result.target_name, "Goblin");
        // Player ATK=5, Goblin DEF=0 → 5 dmg/hit, Goblin HP=6 → 2 hits to kill
        assert_eq!(result.rounds, 2);
        // Goblin ATK=3, Player DEF=2 → 1 dmg/hit, 1 hit taken (dies on round 2)
        assert_eq!(result.player_hp_lost, 1);
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn auto_fight_kills_orc() {
        let mut gs = test_game();
        let orc = Entity::from_template(data::orc(), 6, 5);
        gs.entities.push(orc);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(result.target_killed);
        assert_eq!(result.target_name, "Orc");
        // Player ATK=5, Orc DEF=1 → 4 dmg/hit, Orc HP=12 → 3 hits to kill
        assert_eq!(result.rounds, 3);
        // Orc ATK=4, Player DEF=2 → 2 dmg/hit, 2 hits taken (4 raw damage)
        // Regen heals 1 HP on turn 3 (regen_interval=3), so net loss = 3
        assert_eq!(result.player_hp_lost, 3);
    }

    #[test]
    fn auto_fight_no_adjacent_monster_errors() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.auto_fight();
        assert!(result.is_err());
    }

    #[test]
    fn auto_fight_picks_weakest_target() {
        let mut gs = test_game();
        // Place an orc (12 HP) and a goblin (6 HP) adjacent
        let orc = Entity::from_template(data::orc(), 4, 5);
        gs.entities.push(orc);
        let goblin = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(goblin);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        // Should fight the goblin (lower HP) first
        assert_eq!(result.target_name, "Goblin");
        assert!(result.target_killed);
    }

    #[test]
    fn auto_fight_player_dies_to_troll() {
        let mut gs = test_game();
        gs.entities[0].hp = 5; // low HP
        gs.entities[0].defense = 0;
        let troll = Entity::from_template(data::troll(), 6, 5);
        gs.entities.push(troll);
        gs.update_fov();
        let result = gs.auto_fight().unwrap();
        assert!(!result.target_killed);
        assert!(gs.game_over);
    }

    // --- regen tests ---

    #[test]
    fn regen_heals_on_interval() {
        let mut gs = test_game();
        gs.entities[0].hp = 20;
        // Advance to one turn before regen triggers
        gs.turn_count = data::config().regen_interval - 1;
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // turn_count is now regen_interval, so regen fires
        assert_eq!(gs.entities[0].hp, 21);
    }

    #[test]
    fn regen_does_not_heal_between_intervals() {
        let mut gs = test_game();
        gs.entities[0].hp = 20;
        gs.turn_count = data::config().regen_interval; // just healed
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // turn_count is regen_interval + 1, not a multiple — no heal
        assert_eq!(gs.entities[0].hp, 20);
    }

    #[test]
    fn regen_does_not_exceed_max_hp() {
        let mut gs = test_game();
        // Already at full HP
        gs.turn_count = data::config().regen_interval - 1;
        let hp_before = gs.entities[0].hp;
        assert_eq!(hp_before, gs.entities[0].max_hp);
        gs.step(GameCommand::Wait);
        assert_eq!(gs.entities[0].hp, hp_before);
    }

    #[test]
    fn regen_does_not_heal_dead_player() {
        let mut gs = test_game();
        gs.entities[0].hp = 1;
        gs.entities[0].defense = 0;
        gs.turn_count = data::config().regen_interval - 1;
        // Place a monster that will kill the player
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        gs.step(GameCommand::Wait);
        // Player died — regen should not have brought them back
        assert!(gs.game_over);
        assert!(gs.entities[0].hp <= 0);
    }

    #[test]
    fn regen_accumulates_over_multiple_intervals() {
        let mut gs = test_game();
        gs.entities[0].hp = 20;
        let interval = data::config().regen_interval;
        // Run enough turns for 3 regen ticks
        for _ in 0..(interval * 3) {
            gs.step(GameCommand::Wait);
        }
        assert_eq!(gs.entities[0].hp, 23);
    }

    // --- frontier_tiles() tests ---

    #[test]
    fn frontier_tiles_edge_of_explored() {
        let mut gs = test_game();
        gs.update_fov();
        let frontiers = gs.frontier_tiles();
        // Some floor tiles at the edge of FOV should border unexplored tiles.
        assert!(!frontiers.is_empty());
        // Every frontier tile must be a walkable, explored tile.
        for &(x, y) in &frontiers {
            assert!(gs.map.is_walkable(x, y));
            assert!(gs.explored.contains(&(x, y)));
        }
    }

    #[test]
    fn frontier_tiles_none_when_fully_explored() {
        let mut gs = test_game();
        // Mark every in-bounds tile as explored.
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                gs.explored.insert((x, y));
            }
        }
        let frontiers = gs.frontier_tiles();
        assert!(frontiers.is_empty());
    }

    #[test]
    fn frontier_tiles_only_floor() {
        let mut gs = test_game();
        gs.update_fov();
        let frontiers = gs.frontier_tiles();
        // Wall tiles should never appear as frontiers.
        for &(x, y) in &frontiers {
            assert!(
                gs.map.is_walkable(x, y),
                "Frontier at ({},{}) is not floor",
                x,
                y
            );
        }
    }

    #[test]
    fn explored_map_renders_frontier_as_tilde() {
        let mut gs = test_game();
        gs.update_fov();
        let frontiers = gs.frontier_tiles();
        if frontiers.is_empty() {
            return; // nothing to check in this map layout
        }
        let map_lines = gs.explored_map();
        let text = map_lines.join("\n");
        assert!(text.contains('~'), "Frontier tiles should be rendered as ~");
    }

    // --- pathfind_to() tests ---

    #[test]
    fn pathfind_to_reaches_target() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.pathfind_to(8, 5).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::PathComplete);
        assert_eq!(gs.entities[0].x, 8);
        assert_eq!(gs.entities[0].y, 5);
        assert_eq!(result.steps_taken, 3);
    }

    #[test]
    fn pathfind_to_self_is_zero_steps() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.pathfind_to(5, 5).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::PathComplete);
        assert_eq!(result.steps_taken, 0);
    }

    #[test]
    fn pathfind_to_wall_returns_error() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.pathfind_to(0, 0);
        assert!(result.is_err());
    }

    #[test]
    fn pathfind_to_unexplored_returns_error() {
        let mut gs = test_game();
        gs.update_fov();
        // (15, 15) is floor-free in test_game (only 1..=10 is carved),
        // but even if it were floor it's unexplored. Use a coordinate
        // that's definitely out of the explored set.
        let result = gs.pathfind_to(19, 19);
        assert!(result.is_err());
    }

    #[test]
    fn pathfind_to_stops_for_adjacent_monster() {
        let mut gs = test_game();
        // Place monster directly adjacent at (6, 5). The has_adjacent_monster
        // check fires before any steps are taken.
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.pathfind_to(9, 5).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert_eq!(result.steps_taken, 0);
    }

    #[test]
    fn pathfind_to_stops_on_damage() {
        let mut gs = test_game();
        // Place monster 2 tiles away at (7, 5). Player steps to (6, 5),
        // monster is now adjacent and attacks during its turn → damage taken.
        let monster = Entity::from_template(data::goblin(), 7, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.pathfind_to(9, 5).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::DamageTaken);
        assert_eq!(result.steps_taken, 1);
    }

    #[test]
    fn pathfind_to_diagonal_path() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.pathfind_to(8, 8).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::PathComplete);
        assert_eq!(gs.entities[0].x, 8);
        assert_eq!(gs.entities[0].y, 8);
        // Chebyshev optimal: 3 diagonal steps
        assert_eq!(result.steps_taken, 3);
    }

    // --- new_tiles_revealed tests ---

    #[test]
    fn autorun_counts_new_tiles_revealed() {
        let mut gs = corridor_game();
        // Running east from (5,5) in a corridor reveals new tiles.
        let result = gs.autorun(Direction::East);
        assert!(result.new_tiles_revealed > 0);
    }

    #[test]
    fn autorun_into_wall_reveals_zero_tiles() {
        let mut gs = corridor_game();
        // Running north into a wall from (5,5) — no movement, no new tiles.
        let result = gs.autorun(Direction::North);
        assert_eq!(result.new_tiles_revealed, 0);
        assert_eq!(result.steps_taken, 0);
    }

    #[test]
    fn pathfind_counts_new_tiles_revealed() {
        let mut gs = corridor_game();
        let result = gs.pathfind_to(10, 5).unwrap();
        // Walking east in a corridor reveals tiles at the far end.
        assert!(result.new_tiles_revealed >= 0);
        assert!(result.steps_taken > 0);
    }

    // --- auto_explore tests ---

    #[test]
    fn auto_explore_moves_toward_frontier() {
        let mut gs = corridor_game();
        // Corridor has unexplored tiles beyond FOV radius.
        let result = gs.auto_explore().unwrap();
        assert!(result.movement.steps_taken > 0);
        assert_eq!(result.movement.stop_reason, AutorunStopReason::PathComplete);
    }

    #[test]
    fn auto_explore_errors_when_fully_explored() {
        let mut gs = test_game();
        // Mark every in-bounds tile as explored.
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                gs.explored.insert((x, y));
            }
        }
        let result = gs.auto_explore();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("fully explored"));
    }

    #[test]
    fn auto_explore_stops_for_monster() {
        let mut gs = corridor_game();
        // Place monster adjacent at (6, 5).
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.auto_explore().unwrap();
        assert_eq!(
            result.movement.stop_reason,
            AutorunStopReason::MonsterSpotted
        );
        assert_eq!(result.movement.steps_taken, 0);
    }

    // --- AutorunStepper tests ---

    #[test]
    fn stepper_directional_matches_autorun() {
        // Verify stepper produces identical results to convenience method.
        let mut gs1 = corridor_game();
        let mut gs2 = corridor_game();
        let result_convenience = gs1.autorun(Direction::East);
        let stepper = gs2.start_autorun(Direction::East);
        let result_stepper = stepper.run_to_completion(&mut gs2);
        assert_eq!(result_convenience.steps_taken, result_stepper.steps_taken);
        assert_eq!(result_convenience.stop_reason, result_stepper.stop_reason);
        assert_eq!(
            result_convenience.new_tiles_revealed,
            result_stepper.new_tiles_revealed
        );
        assert_eq!(gs1.entities[0].x, gs2.entities[0].x);
        assert_eq!(gs1.entities[0].y, gs2.entities[0].y);
    }

    #[test]
    fn stepper_follow_path_matches_pathfind_to() {
        let mut gs1 = test_game();
        gs1.update_fov();
        let mut gs2 = test_game();
        gs2.update_fov();
        let result_conv = gs1.pathfind_to(8, 5).unwrap();
        let stepper = gs2.start_pathfind(8, 5).unwrap();
        let result_step = stepper.run_to_completion(&mut gs2);
        assert_eq!(result_conv.steps_taken, result_step.steps_taken);
        assert_eq!(result_conv.stop_reason, result_step.stop_reason);
        assert_eq!(gs1.entities[0].x, gs2.entities[0].x);
    }

    #[test]
    fn stepper_yields_continue_then_done() {
        let mut gs = corridor_game();
        let mut stepper = gs.start_autorun(Direction::East);
        // First step should be Continue (corridor is long).
        assert!(matches!(stepper.next_step(&mut gs), StepOutcome::Continue));
        // Run to completion.
        loop {
            match stepper.next_step(&mut gs) {
                StepOutcome::Continue => continue,
                StepOutcome::Done(result) => {
                    assert!(result.steps_taken > 0);
                    break;
                }
            }
        }
    }

    #[test]
    fn stepper_stops_for_adjacent_monster() {
        let mut gs = corridor_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let mut stepper = gs.start_autorun(Direction::East);
        match stepper.next_step(&mut gs) {
            StepOutcome::Done(result) => {
                assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
                assert_eq!(result.steps_taken, 0);
            }
            StepOutcome::Continue => panic!("Expected Done"),
        }
    }

    #[test]
    fn stepper_follow_path_empty_handled_by_convenience() {
        let mut gs = test_game();
        gs.update_fov();
        let result = gs.pathfind_to(5, 5).unwrap();
        assert_eq!(result.stop_reason, AutorunStopReason::PathComplete);
        assert_eq!(result.steps_taken, 0);
    }

    #[test]
    fn start_auto_explore_returns_stepper_and_target() {
        let mut gs = corridor_game();
        let (stepper, tx, ty) = gs.start_auto_explore().unwrap();
        let frontiers = gs.frontier_tiles();
        assert!(frontiers.contains(&(tx, ty)));
        let result = stepper.run_to_completion(&mut gs);
        assert!(result.steps_taken > 0);
    }

    #[test]
    fn start_auto_explore_errors_when_fully_explored() {
        let mut gs = test_game();
        for y in 0..gs.map.height {
            for x in 0..gs.map.width {
                gs.explored.insert((x, y));
            }
        }
        assert!(gs.start_auto_explore().is_err());
    }

    // --- inventory & pickup tests ---

    #[test]
    fn walk_over_item_notifies_only() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 6,
            y: 5,
            kind: ItemKind::HealthPotion,
        });
        gs.update_fov();
        gs.step(GameCommand::Move(Direction::East));
        // Item should still be on ground (no auto-pickup).
        assert_eq!(gs.ground_items.len(), 1);
        assert!(gs.inventory.is_empty());
        // Should have notified about the item.
        assert!(gs.log.recent(5).iter().any(|m| m.contains("You see")));
    }

    #[test]
    fn pickup_adds_to_inventory() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 5,
            y: 5,
            kind: ItemKind::HealthPotion,
        });
        let result = gs.step(GameCommand::Pickup);
        assert!(result.action_taken);
        assert!(gs.ground_items.is_empty());
        assert_eq!(gs.inventory.len(), 1);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
    }

    #[test]
    fn pickup_full_inventory_rejected() {
        let mut gs = test_game();
        for _ in 0..rules_items::MAX_INVENTORY {
            gs.inventory.add(ItemKind::ShortSword);
        }
        gs.ground_items.push(Item {
            x: 5,
            y: 5,
            kind: ItemKind::ShortSword,
        });
        let result = gs.step(GameCommand::Pickup);
        assert!(result.action_taken); // turn consumed
        assert_eq!(gs.ground_items.len(), 1); // item stays
        assert!(gs.log.recent(5).iter().any(|m| m.contains("full")));
    }

    // --- auto-pickup tests ---

    #[test]
    fn auto_pickup_grabs_consumable() {
        let mut gs = test_game();
        gs.auto_pickup = true;
        gs.ground_items.push(Item {
            x: 6,
            y: 5,
            kind: ItemKind::HealthPotion,
        });
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.ground_items.is_empty());
        assert_eq!(gs.inventory.len(), 1);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
    }

    #[test]
    fn auto_pickup_grabs_equipment() {
        let mut gs = test_game();
        gs.auto_pickup = true;
        gs.ground_items.push(Item {
            x: 6,
            y: 5,
            kind: ItemKind::ShortSword,
        });
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.ground_items.is_empty());
        assert_eq!(gs.inventory.len(), 1);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn auto_pickup_multiple_consumables() {
        let mut gs = test_game();
        gs.auto_pickup = true;
        for _ in 0..3 {
            gs.ground_items.push(Item {
                x: 6,
                y: 5,
                kind: ItemKind::HealthPotion,
            });
        }
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.ground_items.is_empty());
        assert_eq!(gs.inventory.len(), 1); // stacked
        assert_eq!(gs.inventory.get(0).unwrap().count, 3);
    }

    #[test]
    fn auto_pickup_stops_when_inventory_full() {
        let mut gs = test_game();
        gs.auto_pickup = true;
        for _ in 0..rules_items::MAX_INVENTORY {
            gs.inventory.add(ItemKind::ShortSword);
        }
        gs.ground_items.push(Item {
            x: 6,
            y: 5,
            kind: ItemKind::HealthPotion,
        });
        gs.step(GameCommand::Move(Direction::East));
        assert_eq!(gs.ground_items.len(), 1); // potion stays
        // Should still notify about the item
        assert!(gs.log.recent(5).iter().any(|m| m.contains("Health Potion")));
    }

    #[test]
    fn auto_pickup_off_by_default() {
        let gs = test_game();
        assert!(!gs.auto_pickup);
    }

    #[test]
    fn use_potion_heals_from_inventory() {
        let mut gs = test_game();
        gs.entities[0].hp = 20; // injured (max 30)
        gs.inventory.add(ItemKind::HealthPotion);
        let result = gs.step(GameCommand::UseItem(0));
        assert!(result.action_taken);
        assert_eq!(gs.entities[0].hp, 30);
        assert!(gs.inventory.is_empty());
    }

    #[test]
    fn use_on_empty_slot_no_action() {
        let mut gs = test_game();
        let result = gs.step(GameCommand::UseItem(0));
        assert!(!result.action_taken);
    }

    #[test]
    fn drop_puts_item_on_ground() {
        let mut gs = test_game();
        gs.inventory.add(ItemKind::ShortSword);
        let result = gs.step(GameCommand::DropItem(0));
        assert!(result.action_taken);
        assert!(gs.inventory.is_empty());
        assert_eq!(gs.ground_items.len(), 1);
        assert_eq!(gs.ground_items[0].kind, ItemKind::ShortSword);
        assert_eq!(gs.ground_items[0].x, gs.entities[0].x);
        assert_eq!(gs.ground_items[0].y, gs.entities[0].y);
    }

    #[test]
    fn equip_from_inventory() {
        let mut gs = test_game();
        gs.inventory.add(ItemKind::ShortSword);
        assert!(gs.equipment.weapon.is_none());
        let result = gs.step(GameCommand::EquipItem(0));
        assert!(result.action_taken);
        assert_eq!(gs.equipment.weapon, Some(ItemKind::ShortSword));
        assert!(gs.inventory.is_empty());
    }

    #[test]
    fn equip_swaps_old_to_inventory() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        gs.inventory.add(ItemKind::ShortSword); // second sword
        let result = gs.step(GameCommand::EquipItem(0));
        assert!(result.action_taken);
        assert_eq!(gs.equipment.weapon, Some(ItemKind::ShortSword));
        // Old weapon goes back into inventory.
        assert_eq!(gs.inventory.len(), 1);
    }

    #[test]
    fn equip_armor_from_inventory() {
        let mut gs = test_game();
        gs.inventory.add(ItemKind::LeatherArmor);
        assert!(gs.equipment.armor.is_none());
        let result = gs.step(GameCommand::EquipItem(0));
        assert!(result.action_taken);
        assert_eq!(gs.equipment.armor, Some(ItemKind::LeatherArmor));
        assert!(gs.inventory.is_empty());
    }

    #[test]
    fn unequip_weapon_returns_to_inventory() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        assert!(gs.inventory.is_empty());
        let result = gs.step(GameCommand::UnequipWeapon);
        assert!(result.action_taken);
        assert!(gs.equipment.weapon.is_none());
        assert_eq!(gs.inventory.len(), 1);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn unequip_armor_returns_to_inventory() {
        let mut gs = test_game();
        gs.equipment.armor = Some(ItemKind::LeatherArmor);
        let result = gs.step(GameCommand::UnequipArmor);
        assert!(result.action_taken);
        assert!(gs.equipment.armor.is_none());
        assert_eq!(gs.inventory.len(), 1);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::LeatherArmor);
    }

    #[test]
    fn unequip_nothing_no_action() {
        let mut gs = test_game();
        let result = gs.step(GameCommand::UnequipWeapon);
        assert!(!result.action_taken);
        let result = gs.step(GameCommand::UnequipArmor);
        assert!(!result.action_taken);
    }

    #[test]
    fn unequip_weapon_full_inventory_fails() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        // Fill all 26 inventory slots.
        for _ in 0..26 {
            gs.inventory.add(ItemKind::ShortSword);
        }
        assert!(gs.inventory.is_full());
        let result = gs.step(GameCommand::UnequipWeapon);
        assert!(!result.action_taken);
        // Weapon stays equipped.
        assert_eq!(gs.equipment.weapon, Some(ItemKind::ShortSword));
    }

    #[test]
    fn drop_equipped_weapon_to_ground() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        let result = gs.step(GameCommand::DropEquippedWeapon);
        assert!(result.action_taken);
        assert!(gs.equipment.weapon.is_none());
        assert!(gs.inventory.is_empty()); // bypasses inventory
        assert_eq!(gs.ground_items.len(), 1);
        assert_eq!(gs.ground_items[0].kind, ItemKind::ShortSword);
    }

    #[test]
    fn drop_equipped_armor_to_ground() {
        let mut gs = test_game();
        gs.equipment.armor = Some(ItemKind::LeatherArmor);
        let result = gs.step(GameCommand::DropEquippedArmor);
        assert!(result.action_taken);
        assert!(gs.equipment.armor.is_none());
        assert_eq!(gs.ground_items.len(), 1);
        assert_eq!(gs.ground_items[0].kind, ItemKind::LeatherArmor);
    }

    #[test]
    fn drop_equipped_works_with_full_inventory() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        for _ in 0..26 {
            gs.inventory.add(ItemKind::ShortSword);
        }
        assert!(gs.inventory.is_full());
        let result = gs.step(GameCommand::DropEquippedWeapon);
        assert!(result.action_taken); // works even with full inventory
        assert!(gs.equipment.weapon.is_none());
        assert_eq!(gs.ground_items.len(), 1);
    }

    #[test]
    fn drop_equipped_nothing_no_action() {
        let mut gs = test_game();
        let result = gs.step(GameCommand::DropEquippedWeapon);
        assert!(!result.action_taken);
    }

    #[test]
    fn inventory_persists_across_descent() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.inventory.add(ItemKind::HealthPotion);
        gs.inventory.add(ItemKind::ShortSword);
        // Find stairs and descend.
        let stairs_pos = gs
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs.map.width, i as i32 / gs.map.width))
            .unwrap();
        gs.entities[0].x = stairs_pos.0;
        gs.entities[0].y = stairs_pos.1;
        gs.update_fov();
        gs.descend();
        assert_eq!(gs.inventory.len(), 2);
        assert_eq!(gs.inventory.get(0).unwrap().kind, ItemKind::HealthPotion);
        assert_eq!(gs.inventory.get(1).unwrap().kind, ItemKind::ShortSword);
    }

    #[test]
    fn effective_attack_includes_weapon() {
        let mut gs = test_game();
        let base_atk = gs.entities[0].attack;
        assert_eq!(gs.effective_attack(), base_atk);
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        gs.equipment.weapon_props = rules_items::default_properties(ItemKind::ShortSword);
        assert_eq!(gs.effective_attack(), base_atk + 3);
    }

    #[test]
    fn effective_defense_includes_armor() {
        let mut gs = test_game();
        let base_def = gs.entities[0].defense;
        assert_eq!(gs.effective_defense(), base_def);
        gs.equipment.armor = Some(ItemKind::LeatherArmor);
        gs.equipment.armor_props = rules_items::default_properties(ItemKind::LeatherArmor);
        assert_eq!(gs.effective_defense(), base_def + 2);
    }

    #[test]
    fn glyph_at_shows_item_on_floor() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 3,
            y: 3,
            kind: ItemKind::HealthPotion,
        });
        gs.update_fov();
        assert_eq!(gs.glyph_at(3, 3), Some('!'));
    }

    #[test]
    fn entity_glyph_hides_item_beneath() {
        let mut gs = test_game();
        // Place item under player
        gs.ground_items.push(Item {
            x: 5,
            y: 5,
            kind: ItemKind::ShortSword,
        });
        gs.update_fov();
        // Player glyph takes priority
        assert_eq!(gs.glyph_at(5, 5), Some('@'));
    }

    #[test]
    fn look_at_shows_item() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 3,
            y: 3,
            kind: ItemKind::HealthPotion,
        });
        gs.update_fov();
        let info = gs.look_at(3, 3);
        assert_eq!(info.items.len(), 1);
        assert_eq!(info.items[0].name, "Health Potion");
    }

    #[test]
    fn look_at_hides_items_outside_fov() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 3,
            y: 3,
            kind: ItemKind::HealthPotion,
        });
        gs.update_fov();
        gs.visible.remove(&(3, 3));
        let info = gs.look_at(3, 3);
        assert!(info.items.is_empty());
    }

    #[test]
    fn observe_includes_visible_items() {
        let mut gs = test_game();
        gs.ground_items.push(Item {
            x: 3,
            y: 3,
            kind: ItemKind::HealthPotion,
        });
        gs.update_fov();
        let obs = gs.observe();
        assert!(obs.visible_items.iter().any(|i| i.name == "Health Potion"));
    }

    #[test]
    fn observe_includes_equipment() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        gs.equipment.armor = Some(ItemKind::LeatherArmor);
        gs.update_fov();
        let obs = gs.observe();
        assert_eq!(obs.weapon, Some("Short Sword".to_string()));
        assert_eq!(obs.armor, Some("Leather Armor".to_string()));
        assert_eq!(obs.player_atk, gs.effective_attack());
        assert_eq!(obs.player_def, gs.effective_defense());
    }

    #[test]
    fn seeded_game_spawns_items_deterministically() {
        let gs1 = GameState::with_seed(80, 40, 12345);
        let gs2 = GameState::with_seed(80, 40, 12345);
        assert_eq!(gs1.ground_items.len(), gs2.ground_items.len());
        for (a, b) in gs1.ground_items.iter().zip(gs2.ground_items.iter()) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.y, b.y);
            assert_eq!(a.kind, b.kind);
        }
    }

    #[test]
    fn save_load_preserves_items_and_equipment() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        let json = gs.save_to_json().unwrap();
        let loaded = GameState::load_from_json(&json).unwrap();
        assert_eq!(gs.ground_items.len(), loaded.ground_items.len());
        for (a, b) in gs.ground_items.iter().zip(loaded.ground_items.iter()) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.y, b.y);
            assert_eq!(a.kind, b.kind);
        }
        assert_eq!(loaded.equipment.weapon, Some(ItemKind::ShortSword));
    }

    #[test]
    fn combat_uses_equipment_bonuses() {
        let mut gs = test_game();
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        gs.equipment.weapon_props = rules_items::default_properties(ItemKind::ShortSword);
        gs.equipment.armor = Some(ItemKind::LeatherArmor);
        gs.equipment.armor_props = rules_items::default_properties(ItemKind::LeatherArmor);
        let goblin = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(goblin);
        gs.update_fov();
        // Attack the goblin
        gs.step(GameCommand::Move(Direction::East));
        // Player ATK 5+3=8, Goblin DEF 0 → 8 dmg, kills in 1 hit (HP 6)
        assert!(!gs.entities[1].alive);
    }

    // --- seeded RNG tests ---

    #[test]
    fn same_seed_produces_identical_games() {
        let seed = 12345;
        let gs1 = GameState::with_seed(80, 40, seed);
        let gs2 = GameState::with_seed(80, 40, seed);

        // Identical map tiles
        assert_eq!(gs1.map.tiles, gs2.map.tiles);
        assert_eq!(gs1.map.rooms.len(), gs2.map.rooms.len());

        // Identical entity count, positions, and types
        assert_eq!(gs1.entities.len(), gs2.entities.len());
        for (e1, e2) in gs1.entities.iter().zip(gs2.entities.iter()) {
            assert_eq!(e1.x, e2.x);
            assert_eq!(e1.y, e2.y);
            assert_eq!(e1.name, e2.name);
            assert_eq!(e1.glyph, e2.glyph);
            assert_eq!(e1.hp, e2.hp);
        }

        // Seed stored correctly
        assert_eq!(gs1.seed, seed);
        assert_eq!(gs2.seed, seed);
    }

    #[test]
    fn different_seeds_produce_different_maps() {
        let gs1 = GameState::with_seed(80, 40, 1);
        let gs2 = GameState::with_seed(80, 40, 2);

        // Maps should differ (extremely unlikely to collide)
        assert_ne!(gs1.map.tiles, gs2.map.tiles);
    }

    #[test]
    fn seed_appears_in_observation() {
        let seed = 99999;
        let gs = GameState::with_seed(80, 40, seed);
        let obs = gs.observe();
        assert_eq!(obs.seed, seed);
    }

    // --- save/load tests ---

    #[test]
    fn save_load_round_trip() {
        let gs = GameState::with_seed(80, 40, 42);
        let json = gs.save_to_json().expect("save failed");
        let loaded = GameState::load_from_json(&json).expect("load failed");

        // Map tiles and rooms
        assert_eq!(gs.map.tiles, loaded.map.tiles);
        assert_eq!(gs.map.width, loaded.map.width);
        assert_eq!(gs.map.height, loaded.map.height);
        assert_eq!(gs.map.rooms.len(), loaded.map.rooms.len());

        // Entities
        assert_eq!(gs.entities.len(), loaded.entities.len());
        for (e1, e2) in gs.entities.iter().zip(loaded.entities.iter()) {
            assert_eq!(e1.x, e2.x);
            assert_eq!(e1.y, e2.y);
            assert_eq!(e1.name, e2.name);
            assert_eq!(e1.glyph, e2.glyph);
            assert_eq!(e1.hp, e2.hp);
            assert_eq!(e1.color, e2.color);
        }

        // Explored set
        assert_eq!(gs.explored, loaded.explored);

        // Scalar fields
        assert_eq!(gs.seed, loaded.seed);
        assert_eq!(gs.turn_count, loaded.turn_count);
        assert_eq!(gs.game_over, loaded.game_over);

        // Visible is recomputed from FOV, should match
        assert_eq!(gs.visible, loaded.visible);
    }

    #[test]
    fn save_load_mid_game() {
        let mut gs = GameState::with_seed(80, 40, 42);
        // Play 5 turns
        for _ in 0..5 {
            gs.step(GameCommand::Move(Direction::East));
        }
        let px = gs.entities[0].x;
        let py = gs.entities[0].y;
        let turn = gs.turn_count;

        let json = gs.save_to_json().expect("save failed");
        let loaded = GameState::load_from_json(&json).expect("load failed");

        assert_eq!(loaded.entities[0].x, px);
        assert_eq!(loaded.entities[0].y, py);
        assert_eq!(loaded.turn_count, turn);
    }

    #[test]
    fn load_invalid_json_returns_error() {
        let result = GameState::load_from_json("not valid json");
        assert!(result.is_err());
    }

    // --- extract_metadata tests ---

    #[test]
    fn extract_metadata_matches_observe() {
        let mut gs = test_game();
        // Play a few turns to change state.
        for _ in 0..3 {
            gs.step(GameCommand::Move(Direction::East));
        }
        let obs = gs.observe();
        let meta = gs.extract_metadata();
        assert_eq!(meta.turn_count, gs.turn_count);
        assert_eq!(meta.player_hp, obs.player_hp);
        assert_eq!(meta.player_max_hp, obs.player_max_hp);
        assert_eq!(meta.explored_pct, obs.explored_pct);
    }

    // --- dirty flag tests ---

    #[test]
    fn step_sets_dirty_on_action() {
        let mut gs = test_game();
        assert!(!gs.dirty);
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.dirty);
    }

    #[test]
    fn step_does_not_set_dirty_on_wall() {
        let mut gs = test_game();
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        gs.step(GameCommand::Move(Direction::West));
        assert!(!gs.dirty);
    }

    #[test]
    fn save_to_json_does_not_clear_dirty() {
        let mut gs = test_game();
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.dirty);
        let _ = gs.save_to_json();
        assert!(gs.dirty);
    }

    #[test]
    fn dirty_not_serialized() {
        let mut gs = test_game();
        gs.step(GameCommand::Move(Direction::East));
        assert!(gs.dirty);
        let json = gs.save_to_json().unwrap();
        let loaded = GameState::load_from_json(&json).unwrap();
        assert!(!loaded.dirty);
    }

    // --- look_at() tests ---

    #[test]
    fn look_at_player_position() {
        let mut gs = test_game();
        gs.update_fov();
        let info = gs.look_at(5, 5);
        assert_eq!(info.terrain, "Floor");
        assert!(info.visible);
        assert!(info.explored);
        assert_eq!(info.glyph, '@');
        let ent = info.entity.unwrap();
        assert_eq!(ent.name, "Player");
        assert!(ent.alive);
    }

    #[test]
    fn look_at_visible_monster() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let info = gs.look_at(6, 5);
        assert_eq!(info.terrain, "Floor");
        assert!(info.visible);
        assert_eq!(info.glyph, 'g');
        let ent = info.entity.unwrap();
        assert_eq!(ent.name, "Goblin");
        assert!(ent.alive);
        assert_eq!(ent.hp, 6);
    }

    #[test]
    fn look_at_empty_floor() {
        let mut gs = test_game();
        gs.update_fov();
        let info = gs.look_at(3, 3);
        assert_eq!(info.terrain, "Floor");
        assert!(info.visible);
        assert!(info.explored);
        assert_eq!(info.glyph, '.');
        assert!(info.entity.is_none());
    }

    #[test]
    fn look_at_wall() {
        let mut gs = test_game();
        gs.update_fov();
        // (0,0) is a wall and should be explored (visible from player at 5,5 with radius 8)
        // but walls may not be visible; use a wall we know is in FOV
        gs.explored.insert((0, 5));
        let info = gs.look_at(0, 5);
        assert_eq!(info.terrain, "Wall");
        assert_eq!(info.glyph, '#');
    }

    #[test]
    fn look_at_unexplored() {
        let gs = test_game();
        // (19, 19) is far from player, never explored
        let info = gs.look_at(19, 19);
        assert_eq!(info.terrain, "Unknown");
        assert!(!info.visible);
        assert!(!info.explored);
        assert_eq!(info.glyph, ' ');
        assert!(info.entity.is_none());
    }

    #[test]
    fn look_at_out_of_bounds() {
        let gs = test_game();
        let info = gs.look_at(-1, -1);
        assert_eq!(info.terrain, "Out of bounds");
        assert!(!info.visible);
        assert!(!info.explored);
        assert_eq!(info.glyph, ' ');
        assert!(info.entity.is_none());
    }

    #[test]
    fn look_at_explored_not_visible_hides_entity() {
        let mut gs = test_game();
        // Place monster at a position, mark it explored but remove from visible
        let monster = Entity::from_template(data::goblin(), 3, 3);
        gs.entities.push(monster);
        gs.explored.insert((3, 3));
        gs.visible.remove(&(3, 3));
        let info = gs.look_at(3, 3);
        assert!(info.explored);
        assert!(!info.visible);
        // Entity should NOT be shown when not visible
        assert!(info.entity.is_none());
        // Glyph should show terrain, not entity
        assert_eq!(info.glyph, '.');
    }

    #[test]
    fn look_at_corpse() {
        let mut gs = test_game();
        let mut corpse = Entity::from_template(data::goblin(), 6, 5);
        corpse.alive = false;
        gs.entities.push(corpse);
        gs.update_fov();
        let info = gs.look_at(6, 5);
        assert_eq!(info.glyph, '%');
        let ent = info.entity.unwrap();
        assert!(!ent.alive);
        assert_eq!(ent.name, "Goblin");
    }

    #[test]
    fn handle_command_look_does_not_consume_turn() {
        let mut gs = test_game();
        let acted = gs.handle_command(GameCommand::Look);
        assert!(!acted);
    }

    #[test]
    fn look_at_with_reveal_shows_monster_outside_fov() {
        let mut gs = test_game();
        // Place monster outside FOV.
        let monster = Entity::from_template(data::goblin(), 3, 3);
        gs.entities.push(monster);
        gs.explored.insert((3, 3));
        gs.visible.remove(&(3, 3));
        // Without reveal: no entity.
        let info = gs.look_at(3, 3);
        assert!(info.entity.is_none());
        assert_eq!(info.glyph, '.');
        // With reveal: entity shown.
        let opts = LookOptions {
            reveal_monsters: true,
        };
        let info = gs.look_at_with(3, 3, &opts);
        assert!(info.entity.is_some());
        let ent = info.entity.unwrap();
        assert_eq!(ent.name, "Goblin");
        assert!(ent.alive);
        assert_eq!(info.glyph, 'g');
    }

    #[test]
    fn look_at_with_reveal_does_not_show_corpse_outside_fov() {
        let mut gs = test_game();
        // Place dead monster outside FOV.
        let mut corpse = Entity::from_template(data::goblin(), 3, 3);
        corpse.alive = false;
        corpse.hp = 0;
        gs.entities.push(corpse);
        gs.explored.insert((3, 3));
        gs.visible.remove(&(3, 3));
        // Even with reveal: corpses are not shown (only alive monsters).
        let opts = LookOptions {
            reveal_monsters: true,
        };
        let info = gs.look_at_with(3, 3, &opts);
        assert!(info.entity.is_none());
        assert_eq!(info.glyph, '.');
    }

    #[test]
    fn look_at_with_default_opts_same_as_look_at() {
        let mut gs = test_game();
        let monster = Entity::from_template(data::goblin(), 3, 3);
        gs.entities.push(monster);
        gs.explored.insert((3, 3));
        gs.visible.remove(&(3, 3));
        let normal = gs.look_at(3, 3);
        let with_defaults = gs.look_at_with(3, 3, &LookOptions::default());
        assert_eq!(normal.entity.is_some(), with_defaults.entity.is_some());
        assert_eq!(normal.glyph, with_defaults.glyph);
    }

    // --- Stairs & depth tests ---

    #[test]
    fn generated_map_has_stairs() {
        let gs = GameState::with_seed(80, 40, 42);
        let has_stairs = gs.map.tiles.contains(&Tile::StairsDown);
        assert!(has_stairs, "Generated map should have stairs down");
    }

    #[test]
    fn descend_fails_when_not_on_stairs() {
        let mut gs = test_game();
        let result = gs.handle_command(GameCommand::Descend);
        assert!(!result, "Descend should fail when not on stairs");
        assert_eq!(gs.depth, 1);
    }

    #[test]
    fn descend_succeeds_on_stairs() {
        let mut gs = GameState::with_seed(80, 40, 42);
        // Find the stairs tile and move the player there.
        let stairs_pos = gs
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs.map.width, i as i32 / gs.map.width))
            .unwrap();
        gs.entities[0].x = stairs_pos.0;
        gs.entities[0].y = stairs_pos.1;
        gs.update_fov();

        let old_map_tiles = gs.map.tiles.clone();
        let hp_before = gs.entities[0].hp;

        let result = gs.descend();
        assert!(result, "Descend should succeed on stairs");
        assert_eq!(gs.depth, 2);
        assert!(!gs.game_won);
        // Player HP should be preserved.
        assert_eq!(gs.entities[0].hp, hp_before);
        // Map should be regenerated (different tiles).
        assert_ne!(
            gs.map.tiles, old_map_tiles,
            "Map should change after descend"
        );
        // New map should also have stairs.
        let has_stairs = gs.map.tiles.contains(&Tile::StairsDown);
        assert!(has_stairs, "New floor should have stairs down");
    }

    #[test]
    fn descend_preserves_equipment() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.equipment.weapon = Some(ItemKind::ShortSword);
        gs.equipment.armor = Some(ItemKind::LeatherArmor);

        // Move to stairs.
        let stairs_pos = gs
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs.map.width, i as i32 / gs.map.width))
            .unwrap();
        gs.entities[0].x = stairs_pos.0;
        gs.entities[0].y = stairs_pos.1;

        gs.descend();
        assert_eq!(gs.equipment.weapon, Some(ItemKind::ShortSword));
        assert_eq!(gs.equipment.armor, Some(ItemKind::LeatherArmor));
    }

    #[test]
    fn victory_triggers_past_target_depth() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.target_depth = 2;

        // Descend twice to reach depth 3 (past target 2).
        for _ in 0..2 {
            let stairs_pos = gs
                .map
                .tiles
                .iter()
                .enumerate()
                .find(|(_, t)| **t == Tile::StairsDown)
                .map(|(i, _)| (i as i32 % gs.map.width, i as i32 / gs.map.width))
                .unwrap();
            gs.entities[0].x = stairs_pos.0;
            gs.entities[0].y = stairs_pos.1;
            gs.descend();
        }

        assert!(
            gs.game_won,
            "Game should be won after descending past target depth"
        );
        assert_eq!(gs.depth, 3);
    }

    #[test]
    fn depth_scaled_monsters_have_boosted_stats() {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.depth_scaling.monster_hp_per_floor = 2;
        gs.depth_scaling.monster_atk_per_floor = 1;
        gs.depth_scaling.depth_scale_interval = 1;

        // Move to stairs and descend to depth 2.
        let stairs_pos = gs
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs.map.width, i as i32 / gs.map.width))
            .unwrap();
        gs.entities[0].x = stairs_pos.0;
        gs.entities[0].y = stairs_pos.1;
        gs.descend();

        // Check that monsters on floor 2 have bonus stats.
        // interval=1, depth=2, so steps=(2-1)/1=1, bonus=1*scaling.
        let base_goblin_hp = data::goblin().hp;
        let base_goblin_atk = data::goblin().attack;
        for e in gs.entities.iter().skip(1) {
            if e.name == "Goblin" {
                assert_eq!(e.hp, base_goblin_hp + 2, "Goblin HP should be boosted by 2");
                assert_eq!(
                    e.attack,
                    base_goblin_atk + 1,
                    "Goblin ATK should be boosted by 1"
                );
            }
        }
    }

    #[test]
    fn floor_seed_is_deterministic() {
        // Same seed + same depth = same map layout.
        let mut gs1 = GameState::with_seed(80, 40, 99);
        let stairs1 = gs1
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs1.map.width, i as i32 / gs1.map.width))
            .unwrap();
        gs1.entities[0].x = stairs1.0;
        gs1.entities[0].y = stairs1.1;
        gs1.descend();
        let tiles1 = gs1.map.tiles.clone();

        let mut gs2 = GameState::with_seed(80, 40, 99);
        let stairs2 = gs2
            .map
            .tiles
            .iter()
            .enumerate()
            .find(|(_, t)| **t == Tile::StairsDown)
            .map(|(i, _)| (i as i32 % gs2.map.width, i as i32 / gs2.map.width))
            .unwrap();
        gs2.entities[0].x = stairs2.0;
        gs2.entities[0].y = stairs2.1;
        gs2.descend();
        let tiles2 = gs2.map.tiles.clone();

        assert_eq!(tiles1, tiles2, "Same seed+depth should produce same map");
    }

    #[test]
    fn observe_includes_depth_fields() {
        let gs = GameState::with_seed(80, 40, 42);
        let obs = gs.observe();
        assert_eq!(obs.depth, 1);
        assert_eq!(obs.target_depth, balance::TARGET_DEPTH as Stat);
        assert!(!obs.game_won);
    }

    #[test]
    fn explored_pct_counts_stairs_tiles() {
        let mut gs = test_game();
        // Place stairs at (3, 3) which is already a floor tile.
        let idx = gs.map.idx(3, 3);
        gs.map.tiles[idx] = Tile::StairsDown;
        // Stairs should still count in floor_count and explored_pct.
        let pct = gs.explored_pct();
        assert!(pct > 0, "explored_pct should count stairs tiles");
    }

    #[test]
    fn look_at_stairs_shows_terrain() {
        let mut gs = test_game();
        let idx = gs.map.idx(3, 3);
        gs.map.tiles[idx] = Tile::StairsDown;
        gs.visible.insert((3, 3));
        gs.explored.insert((3, 3));
        let info = gs.look_at(3, 3);
        assert_eq!(info.terrain, "Stairs down");
        assert_eq!(info.glyph, '>');
    }

    // ── Combine tests ─────────────────────────────────────────────────

    #[test]
    fn combine_self_rejected() {
        let mut gs = test_game();
        gs.inventory.add(ItemKind::ShortSword);
        let result = gs.step(GameCommand::Combine(0, 0));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_empty_slot_rejected() {
        let mut gs = test_game();
        gs.inventory.add(ItemKind::ShortSword);
        let result = gs.step(GameCommand::Combine(0, 5));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_no_effect_when_no_rules_match() {
        let mut gs = test_game();
        // Two swords: same material properties, no active ingredients
        gs.inventory.add(ItemKind::ShortSword);
        gs.inventory.add(ItemKind::ShortSword);
        let result = gs.step(GameCommand::Combine(0, 1));
        assert!(!result.action_taken);
    }

    #[test]
    fn combine_consumes_consumable_source() {
        use crate::rules::properties::{self, Property};
        let mut gs = test_game();
        // Sword (slot 0) has METAL:8. Potion of Strength (slot 1) has HOT:4.
        // MTL+HOT → BoostA(HARD) — tempering should fire.
        gs.inventory.add(ItemKind::ShortSword);
        gs.inventory.add(ItemKind::StrengthPotion);
        let sword_hard_before =
            properties::get(&gs.inventory.get(0).unwrap().props, Property::Hard);
        let result = gs.step(GameCommand::Combine(0, 1));
        assert!(result.action_taken);
        // Source (consumable) should be consumed
        assert!(gs.inventory.get(1).is_none());
        // Sword's HARD should have increased (tempering)
        let sword_hard_after = properties::get(&gs.inventory.get(0).unwrap().props, Property::Hard);
        assert!(
            sword_hard_after > sword_hard_before,
            "HARD should increase from tempering: before={}, after={}",
            sword_hard_before,
            sword_hard_after,
        );
    }

    #[test]
    fn combine_destroys_source_with_dead_material() {
        use crate::rules::items::default_properties;
        use crate::rules::properties::{self, Property};
        let mut gs = test_game();
        // Target sword with COLD:5 so the CLD+HOT Cancel rule fires.
        let mut target_props = default_properties(ItemKind::ShortSword);
        properties::set(&mut target_props, Property::Cold, 5);
        gs.inventory
            .add_with_props(ItemKind::ShortSword, target_props);
        // Source sword with METAL zeroed (simulates prior corrosion) and
        // HOT:5 to trigger the Cancel rule and make properties change.
        let mut source_props = default_properties(ItemKind::ShortSword);
        properties::set(&mut source_props, Property::Metal, 0);
        properties::set(&mut source_props, Property::Hot, 5);
        gs.inventory
            .add_with_props(ItemKind::ShortSword, source_props);

        let result = gs.step(GameCommand::Combine(0, 1));
        assert!(result.action_taken);
        // Source should be destroyed: METAL was 0 on an item that starts with METAL:8.
        assert!(
            gs.inventory.get(1).is_none(),
            "non-consumable source with dead material should be destroyed"
        );
    }
}
