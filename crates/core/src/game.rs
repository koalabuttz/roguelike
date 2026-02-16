use std::collections::HashSet;

use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::combat;
use crate::command::GameCommand;
use crate::data;
use crate::entity::{Entity, EntityKind};
use crate::fov;
use crate::map;
use crate::message_log::MessageLog;
use crate::pathfinding;
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
}

/// Result of an autorun sequence — multiple steps collapsed into one call.
#[derive(Debug, Serialize)]
pub struct AutorunResult {
    /// How many tiles the player moved.
    pub steps_taken: i32,
    /// Why the run stopped.
    pub stop_reason: AutorunStopReason,
    /// All messages generated during the run.
    pub messages: Vec<String>,
    /// How many new tiles were added to the explored set during this run.
    pub new_tiles_revealed: i32,
}

/// A snapshot of the visible game state, suitable for serialization.
#[derive(Serialize)]
pub struct GameObservation {
    #[serde(rename = "hp")]
    pub player_hp: Stat,
    #[serde(rename = "max_hp")]
    pub player_max_hp: Stat,
    #[serde(rename = "x")]
    pub player_x: Coord,
    #[serde(rename = "y")]
    pub player_y: Coord,
    #[serde(rename = "map")]
    pub map_ascii: Vec<String>,
    #[serde(rename = "entities")]
    pub visible_entities: Vec<EntityInfo>,
    #[serde(rename = "messages")]
    pub recent_messages: Vec<String>,
    pub game_over: bool,
    // --- game stats ---
    pub kills: i32,
    pub rooms_found: i32,
    #[serde(rename = "explored")]
    pub explored_pct: i32,
    pub seed: u64,
}

/// Result of an auto-fight sequence — combat resolved in one call.
#[derive(Debug, Serialize)]
pub struct AutoFightResult {
    /// How many rounds (full turns) the fight lasted.
    pub rounds: i32,
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
    Directional { dx: Coord, dy: Coord },
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
    steps_taken: i32,
    max_steps: i32,
    all_messages: Vec<String>,
    explored_before: i32,
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

        // Compute dx, dy from mode.
        let (dx, dy) = match &self.mode {
            StepperMode::Directional { dx, dy } => (*dx, *dy),
            StepperMode::FollowPath { path, index } => {
                if *index >= path.len() {
                    return self.finish(state, AutorunStopReason::PathComplete);
                }
                let (nx, ny) = path[*index];
                let cx = state.entities[0].x;
                let cy = state.entities[0].y;
                (nx - cx, ny - cy)
            }
        };

        // Snapshot before step.
        let hp_before = state.entities[0].hp;
        let visible_monsters_before = state.visible_monster_ids();

        let result = state.step(GameCommand::Move { dx, dy });
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

        // Mode-specific post-step logic.
        match &self.mode {
            StepperMode::Directional { dx, dy } => {
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
            new_tiles_revealed: state.explored.len() as i32 - self.explored_before,
        })
    }
}

/// Info about a visible entity (monster or corpse).
#[derive(Serialize)]
pub struct EntityInfo {
    pub name: String,
    pub glyph: char,
    pub x: Coord,
    pub y: Coord,
    pub hp: Stat,
    pub max_hp: Stat,
    pub alive: bool,
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
    pub turn_count: i32,
    /// The seed used to generate this game. Enables reproducible dungeons,
    /// seed sharing, and deterministic replay.
    pub seed: u64,
    #[serde(skip)]
    pub dirty: bool,
}

impl GameState {
    /// Create a new game with a random seed.
    pub fn new(width: Coord, height: Coord) -> Self {
        Self::with_seed(width, height, rand::random::<u64>())
    }

    /// Create a new game using a named map preset.
    ///
    /// Presets produce deterministic topologies for testing and development.
    /// Monster spawning still uses the seed for placement within rooms.
    pub fn with_preset(width: Coord, height: Coord, seed: u64, preset: map::MapPreset) -> Self {
        let cfg = &data::CONFIG;
        let mut master = StdRng::seed_from_u64(seed);
        let mut map_rng = StdRng::from_rng(&mut master).unwrap();
        let mut spawn_rng = StdRng::from_rng(&mut master).unwrap();

        let mut map = map::Map::new(width, height);
        let (px, py) = map.from_preset(preset, &mut map_rng);
        map.compute_structural_walls();

        let mut entities = vec![Entity::player(px, py)];
        let monsters = spawn::spawn_monsters(
            &map,
            data::SPAWN_TABLE,
            cfg.max_monsters_per_room,
            &mut spawn_rng,
        );
        entities.extend(monsters);

        let visible = fov::compute_fov(&map, px, py, cfg.fov_radius);
        let explored = visible.clone();

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
            dirty: false,
        }
    }

    /// Create a new game with a specific seed for reproducible dungeons.
    ///
    /// The seed determines map layout and monster placement. Separate RNG
    /// streams ensure that changes to one system (e.g., spawn weights)
    /// don't alter another (e.g., map layout) for the same seed.
    pub fn with_seed(width: Coord, height: Coord, seed: u64) -> Self {
        let cfg = &data::CONFIG;

        // Derive independent RNG streams from the master seed.
        let mut master = StdRng::seed_from_u64(seed);
        let mut map_rng = StdRng::from_rng(&mut master).unwrap();
        let mut spawn_rng = StdRng::from_rng(&mut master).unwrap();

        let mut map = map::Map::new(width, height);
        let (px, py) = map.generate(
            cfg.max_rooms,
            cfg.room_size_min,
            cfg.room_size_max,
            &mut map_rng,
        );
        map.compute_structural_walls();

        let mut entities = vec![Entity::player(px, py)];
        let monsters = spawn::spawn_monsters(
            &map,
            data::SPAWN_TABLE,
            cfg.max_monsters_per_room,
            &mut spawn_rng,
        );
        entities.extend(monsters);

        let visible = fov::compute_fov(&map, px, py, cfg.fov_radius);
        let explored = visible.clone();

        let mut log = MessageLog::new();
        log.add("Welcome to the dungeon! Prepare yourself.");

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
            dirty: false,
        }
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
            combat::melee_attack(&mut self.entities, 0, target_idx, &mut self.log);
            return true;
        }

        if self.map.is_walkable(new_x, new_y) {
            self.entities[0].x = new_x;
            self.entities[0].y = new_y;
            return true;
        }

        false
    }

    /// Dispatch a game command. Returns `true` if the player took an action
    /// (i.e. a turn was consumed), `false` otherwise.
    pub fn handle_command(&mut self, cmd: GameCommand) -> bool {
        match cmd {
            GameCommand::Move { dx, dy } => self.player_move_or_attack(dx, dy),
            GameCommand::Wait => true,
            // Autorun and AutoExplore are handled at a higher level (main loop / MCP act).
            GameCommand::Autorun { .. } | GameCommand::AutoExplore | GameCommand::Quit => false,
        }
    }

    /// Heal the player by 1 HP if enough turns have passed (NetHack-style regen).
    fn apply_regen(&mut self) {
        let player = &mut self.entities[0];
        if player.alive
            && player.hp < player.max_hp
            && self.turn_count % data::CONFIG.regen_interval == 0
        {
            player.hp += 1;
        }
    }

    /// Execute one complete game step: player command, FOV update, monster turns.
    ///
    /// This is the atomic turn operation used by the MCP server and any other
    /// non-terminal consumer. It bundles the logic that `main.rs` performs
    /// across multiple calls into a single method.
    pub fn step(&mut self, cmd: GameCommand) -> StepResult {
        let msg_count_before = self.log.len();
        let action_taken = self.handle_command(cmd);

        if action_taken {
            self.dirty = true;
            self.update_fov();
            if ai::run_monster_turns(&mut self.entities, &self.map, &self.visible, &mut self.log) {
                self.game_over = true;
            }
            self.turn_count += 1;
            self.apply_regen();
        }

        StepResult {
            action_taken,
            new_messages: self.log.messages_since(msg_count_before),
            game_over: self.game_over,
        }
    }

    /// Create a stepper for directional autorun.
    pub fn start_autorun(&self, dx: Coord, dy: Coord) -> AutorunStepper {
        AutorunStepper {
            mode: StepperMode::Directional { dx, dy },
            steps_taken: 0,
            max_steps: data::CONFIG.max_autorun_steps,
            all_messages: Vec::new(),
            explored_before: self.explored.len() as i32,
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
            max_steps: data::CONFIG.max_autorun_steps,
            all_messages: Vec::new(),
            explored_before: self.explored.len() as i32,
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
    pub fn autorun(&mut self, dx: Coord, dy: Coord) -> AutorunResult {
        let stepper = self.start_autorun(dx, dy);
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
            let dx = (tx - self.entities[0].x).signum();
            let dy = (ty - self.entities[0].y).signum();

            // Target moved out of melee range — stop
            if (tx - self.entities[0].x).abs() > 1 || (ty - self.entities[0].y).abs() > 1 {
                break;
            }

            let result = self.step(GameCommand::Move { dx, dy });
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

        // Build ASCII map — only rows with visible content
        let mut map_lines = Vec::new();
        for y in 0..self.map.height {
            let mut line = String::with_capacity(self.map.width as usize);
            let mut has_content = false;

            for x in 0..self.map.width {
                if self.visible.contains(&(x, y)) {
                    has_content = true;
                    // Check for entities (alive first, then dead — matching render order)
                    if let Some(glyph) = self.glyph_at(x, y) {
                        line.push(glyph);
                    } else {
                        match self.map.tiles[self.map.idx(x, y)] {
                            map::Tile::Floor => line.push('.'),
                            map::Tile::Wall => line.push('#'),
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

        // --- game stats ---
        let kills = self.kill_count();
        let rooms_found = self
            .map
            .rooms
            .iter()
            .filter(|r| self.explored.contains(&r.center()))
            .count() as i32;
        let explored_pct = self.explored_pct();

        GameObservation {
            player_hp: player.hp,
            player_max_hp: player.max_hp,
            player_x: player.x,
            player_y: player.y,
            map_ascii: map_lines,
            visible_entities,
            recent_messages: self.log.recent(10).to_vec(),
            game_over: self.game_over,
            kills,
            rooms_found,
            explored_pct,
            seed: self.seed,
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
        }
    }

    /// Number of dead (non-player) entities.
    pub fn kill_count(&self) -> i32 {
        self.entities.iter().skip(1).filter(|e| !e.alive).count() as i32
    }

    /// Percentage of floor tiles the player has explored (0–100).
    pub fn explored_pct(&self) -> i32 {
        let floor_count = self.map.known_floor_count();
        if floor_count == 0 {
            return 0;
        }
        let explored_floors = self
            .explored
            .iter()
            .filter(|&&(x, y)| {
                self.map.in_bounds(x, y) && self.map.tiles[self.map.idx(x, y)] == map::Tile::Floor
            })
            .count() as i32;
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
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Entity, EntityKind};
    use crate::map::{Map, Tile};

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
            dirty: false,
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.entity_at(6, 5), Some(1));
    }

    #[test]
    fn entity_at_ignores_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::ORC, 6, 5);
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
        let acted = gs.handle_command(GameCommand::Move { dx: 1, dy: 0 });
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
        let result = gs.step(GameCommand::Move { dx: 1, dy: 0 });
        assert!(result.action_taken);
        assert!(!result.game_over);
        assert_eq!(gs.entities[0].x, 6);
    }

    #[test]
    fn step_into_wall_does_not_advance() {
        let mut gs = test_game();
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        let result = gs.step(GameCommand::Move { dx: -1, dy: 0 });
        assert!(!result.action_taken);
        assert_eq!(gs.entities[0].x, 1);
    }

    #[test]
    fn step_includes_monster_turn() {
        let mut gs = test_game();
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 19, 19);
        gs.entities.push(monster);
        gs.update_fov();
        let obs = gs.observe();
        assert!(obs.visible_entities.is_empty());
    }

    #[test]
    fn observe_shows_corpses() {
        let mut gs = test_game();
        let mut corpse = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('g'));
    }

    #[test]
    fn glyph_at_returns_corpse_for_dead() {
        let mut gs = test_game();
        let mut monster = Entity::from_template(&data::GOBLIN, 6, 5);
        monster.alive = false;
        gs.entities.push(monster);
        assert_eq!(gs.glyph_at(6, 5), Some('%'));
    }

    #[test]
    fn glyph_at_alive_over_dead() {
        let mut gs = test_game();
        let mut dead = Entity::from_template(&data::GOBLIN, 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(&data::ORC, 6, 5);
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
            dirty: false,
        }
    }

    #[test]
    fn autorun_stops_at_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), corridor ends at x=18. Running east should reach x=18
        // and stop because the tile ahead (x=19) is a wall.
        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::WallReached);
        assert_eq!(gs.entities[0].x, 18);
        assert_eq!(result.steps_taken, 13);
    }

    #[test]
    fn autorun_stops_when_monster_spotted() {
        let mut gs = corridor_game();
        // Place a goblin at x=14, just outside FOV radius of 8 from (5,5).
        // After moving east a few tiles, the goblin enters FOV.
        let monster = Entity::from_template(&data::GOBLIN, 14, 5);
        gs.entities.push(monster);
        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::MonsterSpotted);
        assert!(gs.entities[0].x < 14); // stopped before reaching monster
    }

    #[test]
    fn autorun_stops_when_adjacent_to_monster() {
        let mut gs = corridor_game();
        // Place a goblin adjacent at (6, 5). Autorun should stop immediately
        // because a monster is right next to us — don't auto-attack.
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let result = gs.autorun(1, 0);
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
            dirty: false,
        };

        let result = gs.autorun(1, 0);
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
            dirty: false,
        };

        let result = gs.autorun(1, 0);
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
            dirty: false,
        };

        let result = gs.autorun(1, 0);
        assert_eq!(result.stop_reason, AutorunStopReason::MaxSteps);
        assert_eq!(result.steps_taken, data::CONFIG.max_autorun_steps);
    }

    #[test]
    fn autorun_zero_steps_into_wall() {
        let mut gs = corridor_game();
        // Player at (5,5), run north into wall
        let result = gs.autorun(0, -1);
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
        let result = gs.autorun(1, 0);
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
            dirty: false,
        };

        let result = gs.autorun(1, 0);
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
            dirty: false,
        };

        let result = gs.autorun(1, 0);
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
        let mut dead = Entity::from_template(&data::GOBLIN, 6, 5);
        dead.alive = false;
        gs.entities.push(dead);
        let alive = Entity::from_template(&data::ORC, 7, 5);
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
            gs.step(GameCommand::Move { dx: 1, dy: 0 });
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
        let monster = Entity::from_template(&data::GOBLIN, 3, 3);
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
        let goblin = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let orc = Entity::from_template(&data::ORC, 6, 5);
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
        let orc = Entity::from_template(&data::ORC, 4, 5);
        gs.entities.push(orc);
        let goblin = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let troll = Entity::from_template(&data::TROLL, 6, 5);
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
        gs.turn_count = data::CONFIG.regen_interval - 1;
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // turn_count is now regen_interval, so regen fires
        assert_eq!(gs.entities[0].hp, 21);
    }

    #[test]
    fn regen_does_not_heal_between_intervals() {
        let mut gs = test_game();
        gs.entities[0].hp = 20;
        gs.turn_count = data::CONFIG.regen_interval; // just healed
        let result = gs.step(GameCommand::Wait);
        assert!(result.action_taken);
        // turn_count is regen_interval + 1, not a multiple — no heal
        assert_eq!(gs.entities[0].hp, 20);
    }

    #[test]
    fn regen_does_not_exceed_max_hp() {
        let mut gs = test_game();
        // Already at full HP
        gs.turn_count = data::CONFIG.regen_interval - 1;
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
        gs.turn_count = data::CONFIG.regen_interval - 1;
        // Place a monster that will kill the player
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let interval = data::CONFIG.regen_interval;
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let monster = Entity::from_template(&data::GOBLIN, 7, 5);
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
        let result = gs.autorun(1, 0);
        assert!(result.new_tiles_revealed > 0);
    }

    #[test]
    fn autorun_into_wall_reveals_zero_tiles() {
        let mut gs = corridor_game();
        // Running north into a wall from (5,5) — no movement, no new tiles.
        let result = gs.autorun(0, -1);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
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
        let result_convenience = gs1.autorun(1, 0);
        let stepper = gs2.start_autorun(1, 0);
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
        let mut stepper = gs.start_autorun(1, 0);
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
        let monster = Entity::from_template(&data::GOBLIN, 6, 5);
        gs.entities.push(monster);
        gs.update_fov();
        let mut stepper = gs.start_autorun(1, 0);
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
            gs.step(GameCommand::Move { dx: 1, dy: 0 });
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
            gs.step(GameCommand::Move { dx: 1, dy: 0 });
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
        gs.step(GameCommand::Move { dx: 1, dy: 0 });
        assert!(gs.dirty);
    }

    #[test]
    fn step_does_not_set_dirty_on_wall() {
        let mut gs = test_game();
        gs.entities[0].x = 1;
        gs.entities[0].y = 1;
        gs.step(GameCommand::Move { dx: -1, dy: 0 });
        assert!(!gs.dirty);
    }

    #[test]
    fn save_to_json_does_not_clear_dirty() {
        let mut gs = test_game();
        gs.step(GameCommand::Move { dx: 1, dy: 0 });
        assert!(gs.dirty);
        let _ = gs.save_to_json();
        assert!(gs.dirty);
    }

    #[test]
    fn dirty_not_serialized() {
        let mut gs = test_game();
        gs.step(GameCommand::Move { dx: 1, dy: 0 });
        assert!(gs.dirty);
        let json = gs.save_to_json().unwrap();
        let loaded = GameState::load_from_json(&json).unwrap();
        assert!(!loaded.dirty);
    }
}
