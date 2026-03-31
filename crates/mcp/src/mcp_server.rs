use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use tokio::sync::Mutex;

use roguelike_core::command::GameCommand;
use roguelike_core::data;
use roguelike_core::exploration_graph;
use roguelike_core::game::{
    AutoExploreResult, AutoFightResult, AutorunResult, AutorunStopReason, GameState,
};
use roguelike_core::game_step::{self, GameStep};
use roguelike_core::look;
use roguelike_core::seed_code;
use roguelike_core::types::{Coord, Pos};

use roguelike_core::spectate::FrameSink;

use crate::spectate::FileFrameSink;

/// Per-session state: game instance plus configuration set at `new_game` time.
///
/// Holds `Box<dyn GameStep>` so any capability tier (standard, micro) can be
/// driven through the uniform trait API. Standard-tier-only operations
/// (autorun, pathfinding, exploration graph) downcast to `&GameState`.
struct GameSession {
    game: Box<dyn GameStep>,
    /// Omit ASCII map from observations to reduce response size.
    compact: bool,
    /// Cached hash of the exploration graph inputs. When `None`, the graph has
    /// never been sent. When `Some(h)`, `h` is the fingerprint from the last
    /// time a full graph was injected.
    last_graph_hash: Option<u64>,
}

/// Try to downcast a `&dyn GameStep` to `&GameState`.
fn standard_state(game: &dyn GameStep) -> Option<&GameState> {
    game.as_any().downcast_ref::<GameState>()
}

/// Try to downcast a `&mut dyn GameStep` to `&mut GameState`.
fn standard_state_mut(game: &mut dyn GameStep) -> Option<&mut GameState> {
    game.as_any_mut().downcast_mut::<GameState>()
}

/// Require a standard-tier game, returning an MCP error if not.
fn require_standard(game: &dyn GameStep) -> Result<&GameState, McpError> {
    standard_state(game).ok_or_else(|| {
        McpError::invalid_request("This operation requires a standard-tier game", None)
    })
}

/// MCP server that wraps a roguelike game session.
///
/// Holds an `Option<GameSession>` behind a mutex: `None` until `new_game` is
/// called, then `Some(session)` for the duration of the game. Calling
/// `new_game` again resets the session.
#[derive(Clone)]
pub struct RoguelikeMcpServer {
    session: Arc<Mutex<Option<GameSession>>>,
    save_slot: Arc<Mutex<Option<String>>>,
    spectator: Arc<FileFrameSink>,
    game_data: Arc<data::GameData>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewGameParams {
    /// Map width in tiles. Defaults to 80 if not specified.
    pub width: Option<Coord>,
    /// Map height in tiles. Defaults to 40 if not specified.
    pub height: Option<Coord>,
    /// Random seed for reproducible dungeons. If not specified, a random seed
    /// is generated. Use the same seed to replay a dungeon with identical
    /// layout and monster placement.
    pub seed: Option<u64>,
    /// If true, omit the ASCII map from observations to reduce response size.
    /// Useful for LLM agents that only need stats and entity info.
    pub compact: Option<bool>,
    /// A seed code string (e.g. "r7z3kq" or "r7z3kq-120x60a") that encodes
    /// seed, dimensions, and optional preset. If provided, overrides seed,
    /// width, and height parameters.
    pub seed_code: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ActParams {
    /// The action to take. One of: "move_north", "move_south", "move_east",
    /// "move_west", "move_northeast", "move_northwest", "move_southeast",
    /// "move_southwest", "wait".
    pub action: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PathfindParams {
    /// Target X coordinate.
    pub x: Coord,
    /// Target Y coordinate.
    pub y: Coord,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LookAtParams {
    /// Target X coordinate.
    pub x: Coord,
    /// Target Y coordinate.
    pub y: Coord,
}

impl Default for RoguelikeMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl RoguelikeMcpServer {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Mutex::new(None)),
            save_slot: Arc::new(Mutex::new(None)),
            spectator: Arc::new(FileFrameSink::new()),
            game_data: Arc::new(data::load_game_data()),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Start a new roguelike game. Returns the initial game state observation. Call this before any other tool."
    )]
    pub async fn new_game(
        &self,
        Parameters(params): Parameters<NewGameParams>,
    ) -> Result<CallToolResult, McpError> {
        let compact = params.compact.unwrap_or(false);

        let gd = &self.game_data;

        // Resolve parameters and delegate to the shared factory (which
        // handles tier routing and dimension validation internally).
        let game: Box<dyn GameStep> = if let Some(ref code) = params.seed_code {
            let decoded = seed_code::decode(code)
                .map_err(|e| McpError::invalid_params(format!("Invalid seed code: {e}"), None))?;

            game_step::create_game(
                decoded.seed,
                decoded.width,
                decoded.height,
                decoded.preset,
                gd,
            )
            .map_err(|e| McpError::invalid_params(e, None))?
        } else {
            let width = params.width.unwrap_or(80);
            let height = params.height.unwrap_or(40);

            if let Some(s) = params.seed {
                game_step::create_game(s, width, height, None, gd)
                    .map_err(|e| McpError::invalid_params(e, None))?
            } else {
                game_step::create_random_game(width, height, gd)
                    .map_err(|e| McpError::invalid_params(e, None))?
            }
        };

        let observation = game.observe();
        let mut json_value = serde_json::to_value(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let mut last_graph_hash = None;
        inject_exploration_graph_delta(
            &mut json_value,
            standard_state(game.as_ref()),
            &mut last_graph_hash,
            true,
        );
        self.spectator.write_frame(&observation);
        *self.session.lock().await = Some(GameSession {
            game,
            compact,
            last_graph_hash,
        });

        if compact {
            strip_map(&mut json_value);
        }
        let json = serde_json::to_string(&json_value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Observe the current visible game state. Returns player stats, an ASCII map of visible tiles, a list of visible monsters with their stats, and the recent message log. Note: act, pathfind_to, auto_explore, and auto_fight already return observations. Use observe only to check state without taking an action."
    )]
    pub async fn observe(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let observation = session.game.observe();
        let json = serialize_observation(
            &observation,
            session.compact,
            standard_state(session.game.as_ref()),
            &mut session.last_graph_hash,
        )?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Take an action in the game. Valid actions: 'move_north', 'move_south', 'move_east', 'move_west', 'move_northeast', 'move_northwest', 'move_southeast', 'move_southwest', 'wait', 'descend'. Moving into a monster attacks it. Returns the resulting game state after the action and any monster turns. Use 'descend' when standing on stairs ('>') to go deeper. Also supports autorun: 'autorun_north', 'autorun_south', 'autorun_east', 'autorun_west', 'autorun_northeast', 'autorun_northwest', 'autorun_southeast', 'autorun_southwest'. Autorun keeps moving in that direction until hitting a wall, spotting a new monster, taking damage, or reaching a corridor junction/room entrance. Use autorun to traverse long corridors efficiently. Also supports 'auto_fight' to resolve combat with an adjacent monster in one call — fights the weakest adjacent monster to the death. Response includes game stats: kills, rooms_found, explored."
    )]
    pub async fn act(
        &self,
        Parameters(params): Parameters<ActParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.game.is_terminal() {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;

        // Auto-fight: resolve adjacent combat in one call.
        if params.action == "auto_fight" {
            let fight_result = if let Some(state) = standard_state_mut(session.game.as_mut()) {
                state
                    .auto_fight()
                    .map_err(|e| McpError::invalid_request(e, None))?
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::MicroGameStateAdapter>()
            {
                adapter
                    .auto_fight()
                    .map_err(|e| McpError::invalid_request(e, None))?
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::CompactGameStateAdapter>()
            {
                adapter
                    .auto_fight()
                    .map_err(|e| McpError::invalid_request(e, None))?
            } else {
                return Err(McpError::invalid_request(
                    "Auto-fight not supported for this game tier",
                    None,
                ));
            };
            let observation = session.game.observe();
            self.spectator.write_frame(&observation);
            let json = format_auto_fight_response(
                &observation,
                &fight_result,
                compact,
                standard_state(session.game.as_ref()),
                &mut session.last_graph_hash,
            )
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let cmd = parse_action(&params.action).ok_or_else(|| {
            McpError::invalid_params(
                format!(
                    "Unknown action '{}'. Valid actions: move_north, move_south, \
                     move_east, move_west, move_northeast, move_northwest, \
                     move_southeast, move_southwest, wait, descend, \
                     autorun_north, autorun_south, autorun_east, autorun_west, \
                     autorun_northeast, autorun_northwest, autorun_southeast, \
                     autorun_southwest, auto_fight, pickup, use_item_X, \
                     equip_item_X, drop_item_X, combine_X_Y (X,Y = inventory slots a-z)",
                    params.action
                ),
                None,
            )
        })?;

        // Autorun: loop internally and return final state with metadata.
        if let GameCommand::Autorun(dir) = cmd {
            let autorun_result = if let Some(state) = standard_state_mut(session.game.as_mut()) {
                state.autorun(dir)
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::MicroGameStateAdapter>()
            {
                adapter.autorun(dir)
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::CompactGameStateAdapter>()
            {
                adapter.autorun(dir)
            } else {
                return Err(McpError::invalid_request(
                    "Autorun not supported for this game tier",
                    None,
                ));
            };
            let frontiers: Vec<Pos> = standard_state(session.game.as_ref())
                .map(|s| s.frontier_tiles())
                .unwrap_or_default();
            let observation = session.game.observe();
            self.spectator.write_frame(&observation);
            let json = format_response(
                &observation,
                &autorun_result,
                &frontiers,
                compact,
                standard_state(session.game.as_ref()),
                &mut session.last_graph_hash,
            )
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let explored_before = standard_state(session.game.as_ref())
            .map(|s| s.explored.len() as i32)
            .unwrap_or(0);
        let step_result = session.game.step(cmd);
        let new_tiles_revealed = standard_state(session.game.as_ref())
            .map(|s| s.explored.len() as i32 - explored_before)
            .unwrap_or(0);
        let observation = session.game.observe();
        self.spectator.write_frame(&observation);
        let frontiers: Vec<Pos> = standard_state(session.game.as_ref())
            .map(|s| s.frontier_tiles())
            .unwrap_or_default();
        let mut value = serde_json::to_value(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "new_tiles".into(),
                serde_json::Value::Number(new_tiles_revealed.into()),
            );
        }
        replace_messages(&mut value, &step_result.new_messages);
        inject_frontier_exits(&mut value, &frontiers);
        inject_exploration_graph_delta(
            &mut value,
            standard_state(session.game.as_ref()),
            &mut session.last_graph_hash,
            false,
        );
        if compact {
            strip_map(&mut value);
        }
        let json = serde_json::to_string(&value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the full explored map — all tiles the player has ever seen. Unlike observe (which shows only current FOV), this shows the complete explored dungeon. Entity glyphs only appear at current positions if in FOV. Frontier tiles (explored floor adjacent to unexplored) are marked with '~' to show where further exploration is possible. The response includes frontier_exits coordinates for easy navigation with pathfind_to."
    )]
    pub async fn get_explored_map(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let (map_lines, frontier_tiles) = if let Some(state) = standard_state(session.game.as_ref())
        {
            (state.explored_map(), state.frontier_tiles())
        } else if let Some(adapter) = session
            .game
            .as_any()
            .downcast_ref::<game_step::CompactGameStateAdapter>()
        {
            (adapter.explored_map(), adapter.frontier_tiles())
        } else {
            return Err(McpError::invalid_request(
                "get_explored_map not supported for this game tier",
                None,
            ));
        };
        let (px, py) = session.game.player_xy();
        let frontier_exits: Vec<serde_json::Value> = frontier_tiles
            .iter()
            .map(|&(x, y)| serde_json::json!({"x": x, "y": y}))
            .collect();
        let mut response = serde_json::json!({
            "explored_map": map_lines,
            "x": px,
            "y": py,
            "frontier_exits": frontier_exits,
        });
        inject_exploration_graph_delta(
            &mut response,
            standard_state(session.game.as_ref()),
            &mut session.last_graph_hash,
            true,
        );
        let json = serde_json::to_string(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Look at a specific tile to get detailed information. Returns terrain, entity info, visibility. Does not consume a turn."
    )]
    pub async fn look_at(
        &self,
        Parameters(params): Parameters<LookAtParams>,
    ) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let session = guard.as_ref().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let tile_info = session.game.look_at(params.x, params.y);
        let description = look::format_look_description(&tile_info);
        let mut value = serde_json::to_value(&tile_info)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert("description".into(), serde_json::Value::String(description));
        }
        let json = serde_json::to_string(&value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Pathfind to a target tile using A*. The player automatically walks the shortest path through explored tiles, stopping for monsters, damage, or reaching the target. Use this instead of multiple move commands to navigate to a visible or previously-explored location."
    )]
    pub async fn pathfind_to(
        &self,
        Parameters(params): Parameters<PathfindParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.game.is_terminal() {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;
        let pathfind_result = if let Some(state) = standard_state_mut(session.game.as_mut()) {
            state
                .pathfind_to(params.x, params.y)
                .map_err(|e| McpError::invalid_request(e, None))?
        } else if let Some(adapter) = session
            .game
            .as_any_mut()
            .downcast_mut::<game_step::MicroGameStateAdapter>()
        {
            adapter
                .pathfind_to(params.x, params.y)
                .map_err(|e| McpError::invalid_request(e, None))?
        } else if let Some(adapter) = session
            .game
            .as_any_mut()
            .downcast_mut::<game_step::CompactGameStateAdapter>()
        {
            adapter
                .pathfind_to(params.x, params.y)
                .map_err(|e| McpError::invalid_request(e, None))?
        } else {
            return Err(McpError::invalid_request(
                "Pathfinding not supported for this game tier",
                None,
            ));
        };
        let frontiers: Vec<Pos> = standard_state(session.game.as_ref())
            .map(|s| s.frontier_tiles())
            .unwrap_or_default();
        let observation = session.game.observe();
        self.spectator.write_frame(&observation);
        let json = format_response(
            &observation,
            &pathfind_result,
            &frontiers,
            compact,
            standard_state(session.game.as_ref()),
            &mut session.last_graph_hash,
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Automatically explore the dungeon. Finds the nearest frontier tile (edge of explored area) and pathfinds to it. Equivalent to get_explored_map + pathfind_to in one call. Stops for monsters, damage, or when the frontier is reached. Returns observation with frontiers count, new_tiles revealed, and target_x/target_y explore coordinates."
    )]
    pub async fn auto_explore(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.game.is_terminal() {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;
        let (explore_result, frontier_count) =
            if let Some(state) = standard_state_mut(session.game.as_mut()) {
                let result = state
                    .auto_explore()
                    .map_err(|e| McpError::invalid_request(e, None))?;
                let fc = state.frontier_tiles().len() as i32;
                (result, fc)
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::MicroGameStateAdapter>()
            {
                let result = adapter
                    .auto_explore()
                    .map_err(|e| McpError::invalid_request(e, None))?;
                let fc = adapter.frontier_count();
                (result, fc)
            } else if let Some(adapter) = session
                .game
                .as_any_mut()
                .downcast_mut::<game_step::CompactGameStateAdapter>()
            {
                let result = adapter
                    .auto_explore()
                    .map_err(|e| McpError::invalid_request(e, None))?;
                let fc = adapter.frontier_count();
                (result, fc)
            } else {
                return Err(McpError::invalid_request(
                    "Auto-explore not supported for this game tier",
                    None,
                ));
            };
        let observation = session.game.observe();
        self.spectator.write_frame(&observation);
        let json = format_auto_explore_response(
            &observation,
            &explore_result,
            frontier_count,
            compact,
            standard_state(session.game.as_ref()),
            &mut session.last_graph_hash,
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Save the current game state. Stores the game in an in-memory save slot (one slot, overwrites previous save). Returns turn count, HP, seed, and save size."
    )]
    pub async fn save_game(&self) -> Result<CallToolResult, McpError> {
        // Lock session, serialize, drop lock before acquiring save_slot lock.
        let json = {
            let guard = self.session.lock().await;
            let session = guard.as_ref().ok_or_else(|| {
                McpError::invalid_request("No game in progress. Call new_game first.", None)
            })?;
            let state = require_standard(session.game.as_ref())?;
            let json = state.save_to_json().map_err(|e| {
                McpError::internal_error(format!("Serialization failed: {e}"), None)
            })?;
            let (hp, max_hp) = session.game.player_hp();
            let info = serde_json::json!({
                "saved": true,
                "turn_count": session.game.turn_count(),
                "hp": hp,
                "max_hp": max_hp,
                "seed": state.seed,
                "save_size_bytes": json.len(),
            });
            (json, info)
        };
        let (save_json, info) = json;

        *self.save_slot.lock().await = Some(save_json);

        let response = serde_json::to_string(&info)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(response)]))
    }

    #[tool(
        description = "Load a previously saved game state. Replaces the current game with the saved state. Returns the observation of the restored state."
    )]
    pub async fn load_game(&self) -> Result<CallToolResult, McpError> {
        // Lock save_slot, clone JSON, drop save_slot lock before acquiring session lock.
        let save_json = {
            let guard = self.save_slot.lock().await;
            guard.as_ref().cloned().ok_or_else(|| {
                McpError::invalid_request("No saved game. Call save_game first.", None)
            })?
        };

        let loaded: Box<dyn GameStep> =
            Box::new(GameState::load_from_json(&save_json).map_err(|e| {
                McpError::internal_error(format!("Deserialization failed: {e}"), None)
            })?);
        let observation = loaded.observe();
        let mut value = serde_json::to_value(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert("loaded".into(), serde_json::Value::Bool(true));
        }
        let mut last_graph_hash = None;
        inject_exploration_graph_delta(
            &mut value,
            standard_state(loaded.as_ref()),
            &mut last_graph_hash,
            true,
        );
        self.spectator.write_frame(&observation);
        // Preserve compact setting from the current session.
        let mut guard = self.session.lock().await;
        let compact = guard.as_ref().map(|s| s.compact).unwrap_or(false);
        *guard = Some(GameSession {
            game: loaded,
            compact,
            last_graph_hash,
        });

        if compact {
            strip_map(&mut value);
        }
        let json = serde_json::to_string(&value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the rules and mechanics of the roguelike game. Explains combat, movement, monsters, and field of view. Call this to understand the game before playing."
    )]
    pub async fn get_rules(&self) -> Result<CallToolResult, McpError> {
        let game_data = data::defaults();
        let cfg = &game_data.config;
        let player = &game_data.player;

        // Build monster table dynamically from data.
        let mut monster_lines = String::new();
        for m in &game_data.monsters {
            monster_lines.push_str(&format!(
                "             - {} ({}): HP {}, ATK {}, DEF {}\n",
                m.name, m.glyph, m.hp, m.attack, m.defense,
            ));
        }

        let rules = format!(
            "# Roguelike Game Rules\n\
             \n\
             ## Movement\n\
             You are '@'. Move in 8 directions (cardinal + diagonal) or wait.\n\
             Moving into a wall does nothing (turn is not consumed).\n\
             Moving into a monster attacks it (turn is consumed).\n\
             \n\
             ## Map Symbols\n\
             - '#' = wall (blocks movement and vision)\n\
             - '.' = floor (walkable)\n\
             - '~' = frontier (on explored map only: floor adjacent to unexplored area)\n\
             - '%%' = corpse (walkable, dead monster)\n\
             - '@' = you (the player)\n\
             - Lowercase letters = monsters (g=goblin, o=orc)\n\
             - Uppercase letters = dangerous monsters (T=troll)\n\
             - '!' = potion (walk over to use)\n\
             - '/' = weapon (walk over to equip if better)\n\
             - '[' = armor (walk over to equip if better)\n\
             - '>' = stairs down (stand on it and use 'descend' to go deeper)\n\
             \n\
             ## Field of View\n\
             You can only see tiles within radius {fov}. Monsters beyond your FOV\n\
             are hidden. Monsters only chase you once they enter your FOV.\n\
             \n\
             ## Combat\n\
             Damage = attacker's ATK - defender's DEF (minimum 0).\n\
             If damage > 0, defender loses that many HP. At 0 HP, entity dies.\n\
             \n\
             ## Your Stats\n\
             HP: {php}, ATK: {patk}, DEF: {pdef}\n\
             \n\
             ## Monsters\n\
{monsters}\
             \n\
             ## Items\n\
             Items spawn on the ground in rooms. Walk over them to see what's there.\n\
             - Use 'pickup' to pick up an item into your inventory (26 slots, a-z)\n\
             - Consumables (potions) stack; equipment takes one slot each\n\
             - Use 'use_item_X' to consume a potion from inventory slot X (a-z)\n\
             - Use 'equip_item_X' to equip a weapon/armor from slot X\n\
             - Use 'unequip_weapon' or 'unequip_armor' to unequip and return to inventory\n\
             - Use 'drop_item_X' to drop an item from slot X onto the ground\n\
             - Use 'combine_X_Y' to combine item X (target) with item Y (source)\n\
               Items have properties that interact: fire tempers metal, acid corrodes, etc.\n\
               Consumable sources are consumed; equipment sources are kept.\n\
             - Your inventory is shown in observations when non-empty\n\
             - Health Potion (!) — heals 10 HP\n\
             - Short Sword (/) — +3 ATK\n\
             - Leather Armor ([) — +2 DEF\n\
             Equipment bonuses are shown in observations as atk/def fields.\n\
             \n\
             ## Dungeon Depth & Win Condition\n\
             The dungeon has {target_depth} floors. Find the stairs down ('>') on each \
             floor and use 'descend' to go deeper. Descending past floor {target_depth} \
             wins the game. Monsters get stronger on deeper floors (+HP and +ATK per \
             floor). Previous floors are discarded — you cannot go back up. Explore \
             each floor for items and potions before descending.\n\
             \n\
             ## Strategy Tips\n\
             - Fight in corridors to face one monster at a time.\n\
             - You regenerate 1 HP every {regen} turns. Retreat and move to recover.\n\
             - Remember where health potions are — save them for when you need them.\n\
             \n\
             ## Available Tools\n\
             - **act** — move, wait, autorun, pickup, use/equip/drop items, or auto_fight\n\
             - **observe** — see current FOV, stats, and nearby entities. \
             Rarely needed since act, pathfind_to, auto_explore, and auto_fight \
             already return observations.\n\
             - **auto_explore** — find nearest frontier and walk to it in one call. \
             Best way to explore the dungeon. Returns frontier_exits for next move.\n\
             - **pathfind_to(x, y)** — walk shortest path to any explored tile; \
             stops for monsters, damage, or on arrival\n\
             - **look_at(x, y)** — examine a specific tile; returns terrain, entity \
             info, and visibility without consuming a turn\n\
             - **get_explored_map** — full map of everywhere you've been; '~' tiles \
             are frontiers adjacent to unexplored areas; includes frontier_exits \
             coordinates you can pass to pathfind_to\n\
             - **save_game** — save current state to an in-memory slot (one slot, \
             overwrites previous). Returns turn count, HP, seed, and save size.\n\
             - **load_game** — restore the saved game state. Replaces current game \
             with the saved snapshot.\n\
             - **get_rules** — this help text\n\
             \n\
             ## Autorun\n\
             Use autorun_<direction> (e.g. 'autorun_east') via the act tool to \
             travel in a straight line. Autorun crosses rooms and corridors freely, \
             stopping only when:\n\
             - Wall ahead (dead end)\n\
             - Wall ahead with 2+ alternative paths (decision point)\n\
             - Monster spotted or damage taken\n\
             - Game over or max steps\n\
             \n\
             ## Combat Shortcuts\n\
             - **auto_fight** (via act) — fights the weakest adjacent monster to \
             the death in one call. Use for trivial fights.\n\
             \n\
             ## Game Stats\n\
             The observe response includes: kills, rooms_found, explored_pct \
             (percentage of map explored), and seed (for reproducible dungeons).\n\
             \n\
             ## Seed Sharing\n\
             Every game has a seed_code (shown in observations) that encodes \
             the seed, dimensions, and optional preset into a compact string. \
             Pass seed_code to new_game to replay the exact same dungeon. \
             Format: base36_seed[-WxH][preset_char] where preset chars are \
             a=Arena, c=Corridor, l=Labyrinth, s=SingleRoom, f=OpenField. \
             Examples: 'r7z3kq', 'r7z3kq-120x60', 'r7z3kq-a', 'r7z3kq-120x60a'.",
            fov = cfg.fov_radius,
            php = player.hp,
            patk = player.attack,
            pdef = player.defense,
            monsters = monster_lines,
            regen = cfg.regen_interval,
            target_depth = cfg.target_depth,
        );
        Ok(CallToolResult::success(vec![Content::text(rules)]))
    }
}

#[tool_handler]
impl ServerHandler for RoguelikeMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "roguelike-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "A roguelike dungeon crawler. Use new_game to start, observe to see \
                 the current state, act to take actions, and get_rules to learn the \
                 mechanics. You are '@' navigating a dungeon of rooms and corridors. \
                 Kill monsters, avoid death. Good luck!"
                    .into(),
            ),
        }
    }
}

/// Parse a named action string into a GameCommand.
pub fn parse_action(action: &str) -> Option<GameCommand> {
    use roguelike_core::command::Direction::*;
    match action {
        "move_north" => Some(GameCommand::Move(North)),
        "move_south" => Some(GameCommand::Move(South)),
        "move_east" => Some(GameCommand::Move(East)),
        "move_west" => Some(GameCommand::Move(West)),
        "move_northeast" => Some(GameCommand::Move(NorthEast)),
        "move_northwest" => Some(GameCommand::Move(NorthWest)),
        "move_southeast" => Some(GameCommand::Move(SouthEast)),
        "move_southwest" => Some(GameCommand::Move(SouthWest)),
        "autorun_north" => Some(GameCommand::Autorun(North)),
        "autorun_south" => Some(GameCommand::Autorun(South)),
        "autorun_east" => Some(GameCommand::Autorun(East)),
        "autorun_west" => Some(GameCommand::Autorun(West)),
        "autorun_northeast" => Some(GameCommand::Autorun(NorthEast)),
        "autorun_northwest" => Some(GameCommand::Autorun(NorthWest)),
        "autorun_southeast" => Some(GameCommand::Autorun(SouthEast)),
        "autorun_southwest" => Some(GameCommand::Autorun(SouthWest)),
        "wait" => Some(GameCommand::Wait),
        "descend" => Some(GameCommand::Descend),
        "pickup" => Some(GameCommand::Pickup),
        _ if action.starts_with("use_item_") => {
            parse_slot_letter(action, "use_item_").map(GameCommand::UseItem)
        }
        _ if action.starts_with("drop_item_") => {
            parse_slot_letter(action, "drop_item_").map(GameCommand::DropItem)
        }
        _ if action.starts_with("equip_item_") => {
            parse_slot_letter(action, "equip_item_").map(GameCommand::EquipItem)
        }
        "unequip_weapon" => Some(GameCommand::UnequipWeapon),
        "unequip_armor" => Some(GameCommand::UnequipArmor),
        "drop_equipped_weapon" => Some(GameCommand::DropEquippedWeapon),
        "drop_equipped_armor" => Some(GameCommand::DropEquippedArmor),
        _ if action.starts_with("combine_") => parse_combine_slots(action),
        _ => None,
    }
}

/// Parse a slot letter suffix (a-z) from an action string with the given prefix.
fn parse_slot_letter(action: &str, prefix: &str) -> Option<u8> {
    let letter = action.strip_prefix(prefix)?.as_bytes().first()?;
    match letter {
        b'a'..=b'z' => Some(letter - b'a'),
        _ => None,
    }
}

/// Parse `combine_X_Y` → `GameCommand::Combine(target, source)`.
/// X is the target slot (a-z), Y is the source slot (a-z).
fn parse_combine_slots(action: &str) -> Option<GameCommand> {
    let rest = action.strip_prefix("combine_")?;
    let bytes = rest.as_bytes();
    if bytes.len() == 3 && bytes[1] == b'_' {
        let target = match bytes[0] {
            b'a'..=b'z' => bytes[0] - b'a',
            _ => return None,
        };
        let source = match bytes[2] {
            b'a'..=b'z' => bytes[2] - b'a',
            _ => return None,
        };
        Some(GameCommand::Combine(target, source))
    } else {
        None
    }
}

/// Build a JSON response that merges the observation with auto-fight metadata.
fn format_auto_fight_response(
    observation: &roguelike_core::game::GameObservation,
    fight: &AutoFightResult,
    compact: bool,
    state: Option<&GameState>,
    last_hash: &mut Option<u64>,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(observation)?;
    if let serde_json::Value::Object(ref mut map) = value {
        // Always remove map from auto_fight — combat doesn't move the player.
        map.remove("map");
        map.insert(
            "fight_rounds".into(),
            serde_json::Value::Number(fight.rounds.into()),
        );
        map.insert(
            "fight_target".into(),
            serde_json::Value::String(fight.target_name.clone()),
        );
        map.insert(
            "fight_target_killed".into(),
            serde_json::Value::Bool(fight.target_killed),
        );
        map.insert(
            "fight_hp_lost".into(),
            serde_json::Value::Number(fight.player_hp_lost.into()),
        );
    }
    replace_messages(&mut value, &fight.messages);
    inject_exploration_graph_delta(&mut value, state, last_hash, false);
    if compact {
        strip_map(&mut value);
    }
    serde_json::to_string(&value)
}

/// Build a JSON response that merges the observation with autorun metadata.
fn format_response(
    observation: &roguelike_core::game::GameObservation,
    autorun: &AutorunResult,
    frontier_tiles: &[Pos],
    compact: bool,
    state: Option<&GameState>,
    last_hash: &mut Option<u64>,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(observation)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "steps".into(),
            serde_json::Value::Number(autorun.steps_taken.into()),
        );
        map.insert(
            "stop_reason".into(),
            serde_json::to_value(autorun.stop_reason)?,
        );
        map.insert(
            "new_tiles".into(),
            serde_json::Value::Number(autorun.new_tiles_revealed.into()),
        );
    }
    replace_messages(&mut value, &autorun.messages);
    inject_frontier_exits(&mut value, frontier_tiles);
    inject_exploration_graph_delta(&mut value, state, last_hash, false);
    if compact {
        strip_map(&mut value);
    }
    serde_json::to_string(&value)
}

/// Build a JSON response for auto_explore: observation + autorun metadata + explore target.
///
/// Unlike other movement responses, auto_explore uses a compact `frontier_count`
/// instead of the full `frontier_exits` array. The LLM calls auto_explore in a
/// loop and the server picks the best frontier automatically — the coordinate
/// list is dead weight here. Use `get_explored_map` for the full list.
fn format_auto_explore_response(
    observation: &roguelike_core::game::GameObservation,
    explore: &AutoExploreResult,
    frontier_count: i32,
    compact: bool,
    state: Option<&GameState>,
    last_hash: &mut Option<u64>,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(observation)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "steps".into(),
            serde_json::Value::Number(explore.movement.steps_taken.into()),
        );
        map.insert(
            "stop_reason".into(),
            serde_json::to_value(explore.movement.stop_reason)?,
        );
        map.insert(
            "new_tiles".into(),
            serde_json::Value::Number(explore.movement.new_tiles_revealed.into()),
        );
        map.insert(
            "target_x".into(),
            serde_json::Value::Number(explore.target_x.into()),
        );
        map.insert(
            "target_y".into(),
            serde_json::Value::Number(explore.target_y.into()),
        );
        map.insert(
            "frontiers".into(),
            serde_json::Value::Number(frontier_count.into()),
        );
        // Flag dead ends: reached the frontier target but found nothing new beyond it.
        if explore.movement.stop_reason == AutorunStopReason::PathComplete
            && explore.movement.new_tiles_revealed <= 2
        {
            map.insert("dead_end".into(), serde_json::Value::Bool(true));
        }
    }
    replace_messages(&mut value, &explore.movement.messages);
    inject_exploration_graph_delta(&mut value, state, last_hash, false);
    if compact {
        strip_map(&mut value);
    }
    serde_json::to_string(&value)
}

/// Serialize a `GameObservation`, optionally stripping the map for compact mode.
fn serialize_observation(
    observation: &roguelike_core::game::GameObservation,
    compact: bool,
    state: Option<&GameState>,
    last_hash: &mut Option<u64>,
) -> Result<String, McpError> {
    let mut value = serde_json::to_value(observation)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    inject_exploration_graph_delta(&mut value, state, last_hash, false);
    if compact {
        strip_map(&mut value);
    }
    serde_json::to_string(&value).map_err(|e| McpError::internal_error(e.to_string(), None))
}

/// Remove the `map` field from a serialized observation to save tokens.
fn strip_map(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        map.remove("map");
    }
}

/// Replace `recent_messages` in a serialized observation with only the messages
/// generated during the current action. Avoids sending stale messages from
/// previous turns that waste tokens without adding information.
fn replace_messages(value: &mut serde_json::Value, messages: &[String]) {
    if let serde_json::Value::Object(map) = value {
        map.insert(
            "messages".into(),
            serde_json::to_value(messages).unwrap_or_default(),
        );
    }
}

/// Cheap fingerprint of the inputs that determine exploration graph changes.
///
/// Hashes current room index, explored room count, per-room alive monster
/// counts, and frontier tile count. This is ~100x cheaper than building the
/// full graph (no A* calls). Uses `DefaultHasher` from stdlib — no new deps.
fn exploration_graph_fingerprint(state: &GameState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();

    // Which room contains the player.
    let px = state.entities[0].x;
    let py = state.entities[0].y;
    let current_room = state
        .map
        .rooms
        .iter()
        .position(|r| r.contains_interior(px, py));
    current_room.hash(&mut hasher);

    // Count of explored rooms (rooms whose center is in state.explored).
    let explored_count = state
        .map
        .rooms
        .iter()
        .filter(|r| {
            let (cx, cy) = r.center();
            state.explored.contains(&(cx, cy))
        })
        .count();
    explored_count.hash(&mut hasher);

    // Per-room alive monster count (changes when monsters die).
    for room in &state.map.rooms {
        let alive = state
            .entities
            .iter()
            .skip(1)
            .filter(|e| e.alive && room.contains_interior(e.x, e.y))
            .count();
        alive.hash(&mut hasher);
    }

    // Frontier tile count (cheap proxy for corridor exploration changes).
    state.frontier_tiles().len().hash(&mut hasher);

    hasher.finish()
}

/// Inject the exploration graph with delta support.
///
/// When `state` is `None` (non-standard tier), this is a no-op.
/// When `force` is true, always builds and injects the full graph.
/// When `force` is false, compares the fingerprint to the cached hash:
/// - Same → injects `"exploration_unchanged": true` (skips expensive A*)
/// - Different → builds full graph and updates the hash
fn inject_exploration_graph_delta(
    value: &mut serde_json::Value,
    state: Option<&GameState>,
    last_hash: &mut Option<u64>,
    force: bool,
) {
    let Some(state) = state else { return };

    if state.map.rooms.len() < 2 {
        return;
    }

    let current_fp = exploration_graph_fingerprint(state);

    if !force
        && let Some(prev) = *last_hash
        && prev == current_fp
    {
        if let serde_json::Value::Object(map) = value {
            map.insert(
                "exploration_unchanged".into(),
                serde_json::Value::Bool(true),
            );
        }
        return;
    }

    // Build and inject the full graph.
    let graph = exploration_graph::build_exploration_graph(state);
    if let Ok(graph_value) = serde_json::to_value(&graph)
        && let serde_json::Value::Object(map) = value
    {
        map.insert("exploration".into(), graph_value);
    }
    *last_hash = Some(current_fp);
}

/// Inject `frontier_exits` array into an existing JSON object value.
fn inject_frontier_exits(value: &mut serde_json::Value, frontier_tiles: &[Pos]) {
    if let serde_json::Value::Object(map) = value {
        let exits: Vec<serde_json::Value> = frontier_tiles
            .iter()
            .map(|&(x, y)| serde_json::json!({"x": x, "y": y}))
            .collect();
        map.insert("frontier_exits".into(), serde_json::Value::Array(exits));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roguelike_core::command::Direction;

    #[test]
    fn parse_all_valid_actions() {
        assert_eq!(
            parse_action("move_north"),
            Some(GameCommand::Move(Direction::North))
        );
        assert_eq!(
            parse_action("move_south"),
            Some(GameCommand::Move(Direction::South))
        );
        assert_eq!(
            parse_action("move_east"),
            Some(GameCommand::Move(Direction::East))
        );
        assert_eq!(
            parse_action("move_west"),
            Some(GameCommand::Move(Direction::West))
        );
        assert_eq!(
            parse_action("move_northeast"),
            Some(GameCommand::Move(Direction::NorthEast))
        );
        assert_eq!(
            parse_action("move_northwest"),
            Some(GameCommand::Move(Direction::NorthWest))
        );
        assert_eq!(
            parse_action("move_southeast"),
            Some(GameCommand::Move(Direction::SouthEast))
        );
        assert_eq!(
            parse_action("move_southwest"),
            Some(GameCommand::Move(Direction::SouthWest))
        );
        assert_eq!(parse_action("wait"), Some(GameCommand::Wait));
    }

    #[test]
    fn parse_invalid_action_returns_none() {
        assert_eq!(parse_action("fly"), None);
        assert_eq!(parse_action(""), None);
        assert_eq!(parse_action("MOVE_NORTH"), None);
        assert_eq!(parse_action("move north"), None);
    }

    #[test]
    fn parse_covers_all_eight_directions() {
        let directions = [
            "move_north",
            "move_south",
            "move_east",
            "move_west",
            "move_northeast",
            "move_northwest",
            "move_southeast",
            "move_southwest",
        ];
        for dir in &directions {
            assert!(
                parse_action(dir).is_some(),
                "Expected valid action for '{}'",
                dir
            );
        }
    }

    #[test]
    fn parse_all_autorun_actions() {
        assert_eq!(
            parse_action("autorun_north"),
            Some(GameCommand::Autorun(Direction::North))
        );
        assert_eq!(
            parse_action("autorun_south"),
            Some(GameCommand::Autorun(Direction::South))
        );
        assert_eq!(
            parse_action("autorun_east"),
            Some(GameCommand::Autorun(Direction::East))
        );
        assert_eq!(
            parse_action("autorun_west"),
            Some(GameCommand::Autorun(Direction::West))
        );
        assert_eq!(
            parse_action("autorun_northeast"),
            Some(GameCommand::Autorun(Direction::NorthEast))
        );
        assert_eq!(
            parse_action("autorun_northwest"),
            Some(GameCommand::Autorun(Direction::NorthWest))
        );
        assert_eq!(
            parse_action("autorun_southeast"),
            Some(GameCommand::Autorun(Direction::SouthEast))
        );
        assert_eq!(
            parse_action("autorun_southwest"),
            Some(GameCommand::Autorun(Direction::SouthWest))
        );
    }

    #[test]
    fn parse_covers_all_autorun_directions() {
        let directions = [
            "autorun_north",
            "autorun_south",
            "autorun_east",
            "autorun_west",
            "autorun_northeast",
            "autorun_northwest",
            "autorun_southeast",
            "autorun_southwest",
        ];
        for dir in &directions {
            assert!(
                parse_action(dir).is_some(),
                "Expected valid action for '{}'",
                dir
            );
            assert!(
                matches!(parse_action(dir), Some(GameCommand::Autorun(..))),
                "Expected Autorun command for '{}'",
                dir
            );
        }
    }

    #[test]
    fn auto_fight_response_omits_map_ascii() {
        use roguelike_core::data;
        use roguelike_core::entity::Entity;
        use roguelike_core::fov;
        use roguelike_core::map::{Map, Tile};
        use roguelike_core::message_log::MessageLog;

        // Build a minimal game with a goblin adjacent to the player.
        let mut m = Map::new(20, 20);
        for y in 1..=10 {
            for x in 1..=10 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }
        let player = Entity::player(5, 5);
        let goblin = Entity::from_template(data::goblin(), 6, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        let mut state = GameState {
            map: m,
            entities: vec![player, goblin],
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
        state.update_fov();

        let fight = state.auto_fight().unwrap();
        let obs = state.observe();
        let mut hash = None;
        let json_str =
            format_auto_fight_response(&obs, &fight, false, Some(&state), &mut hash).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // map should be removed from auto_fight responses.
        assert!(parsed.get("map").is_none());
        // But combat metadata should be present.
        assert!(parsed.get("fight_rounds").is_some());
        assert!(parsed.get("fight_target").is_some());
        assert!(parsed.get("fight_target_killed").is_some());
        assert!(parsed.get("fight_hp_lost").is_some());
        // Player stats should still be present.
        assert!(parsed.get("hp").is_some());
    }

    // --- Exploration graph delta tests ---

    fn make_test_state() -> GameState {
        let mut gs = GameState::with_seed(80, 40, 42);
        gs.update_fov();
        gs
    }

    #[test]
    fn exploration_fingerprint_stable_for_same_state() {
        let gs = make_test_state();
        let fp1 = exploration_graph_fingerprint(&gs);
        let fp2 = exploration_graph_fingerprint(&gs);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn exploration_fingerprint_changes_on_room_discovery() {
        let mut gs = make_test_state();
        let fp_before = exploration_graph_fingerprint(&gs);
        // Simulate discovering a new room by adding its center to explored.
        if let Some(room) = gs.map.rooms.iter().find(|r| {
            let (cx, cy) = r.center();
            !gs.explored.contains(&(cx, cy))
        }) {
            let (cx, cy) = room.center();
            gs.explored.insert((cx, cy));
        }
        let fp_after = exploration_graph_fingerprint(&gs);
        assert_ne!(fp_before, fp_after);
    }

    #[test]
    fn exploration_fingerprint_changes_on_monster_death() {
        let mut gs = make_test_state();
        let fp_before = exploration_graph_fingerprint(&gs);
        // Kill an alive monster in a room.
        if let Some(e) = gs.entities.iter_mut().skip(1).find(|e| e.alive) {
            e.alive = false;
        }
        let fp_after = exploration_graph_fingerprint(&gs);
        assert_ne!(fp_before, fp_after);
    }

    #[test]
    fn delta_injects_full_graph_when_forced() {
        let gs = make_test_state();
        let mut hash = None;
        let mut value = serde_json::json!({});
        inject_exploration_graph_delta(&mut value, Some(&gs), &mut hash, true);
        assert!(value.get("exploration").is_some());
        assert!(value.get("exploration_unchanged").is_none());
        assert!(hash.is_some());
    }

    #[test]
    fn delta_skips_graph_when_unchanged() {
        let gs = make_test_state();
        let mut hash = None;
        // First call: force to set the hash.
        let mut v1 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v1, Some(&gs), &mut hash, true);
        assert!(v1.get("exploration").is_some());

        // Second call: same state, not forced → should skip.
        let mut v2 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v2, Some(&gs), &mut hash, false);
        assert!(v2.get("exploration_unchanged").is_some());
        assert!(v2.get("exploration").is_none());
    }

    #[test]
    fn delta_rebuilds_graph_after_state_change() {
        let mut gs = make_test_state();
        let mut hash = None;
        // First call: force to set the hash.
        let mut v1 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v1, Some(&gs), &mut hash, true);

        // Mutate state: kill a monster.
        if let Some(e) = gs.entities.iter_mut().skip(1).find(|e| e.alive) {
            e.alive = false;
        }

        // Third call: state changed, not forced → should rebuild.
        let mut v2 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v2, Some(&gs), &mut hash, false);
        assert!(v2.get("exploration").is_some());
        assert!(v2.get("exploration_unchanged").is_none());
    }

    #[test]
    fn parse_pickup_action() {
        assert_eq!(parse_action("pickup"), Some(GameCommand::Pickup));
    }

    #[test]
    fn parse_use_item_a() {
        assert_eq!(parse_action("use_item_a"), Some(GameCommand::UseItem(0)));
    }

    #[test]
    fn parse_use_item_z() {
        assert_eq!(parse_action("use_item_z"), Some(GameCommand::UseItem(25)));
    }

    #[test]
    fn parse_drop_item_b() {
        assert_eq!(parse_action("drop_item_b"), Some(GameCommand::DropItem(1)));
    }

    #[test]
    fn parse_equip_item_c() {
        assert_eq!(
            parse_action("equip_item_c"),
            Some(GameCommand::EquipItem(2))
        );
    }

    #[test]
    fn parse_unequip_weapon() {
        assert_eq!(
            parse_action("unequip_weapon"),
            Some(GameCommand::UnequipWeapon)
        );
    }

    #[test]
    fn parse_unequip_armor() {
        assert_eq!(
            parse_action("unequip_armor"),
            Some(GameCommand::UnequipArmor)
        );
    }

    #[test]
    fn parse_invalid_slot_letter() {
        assert_eq!(parse_action("use_item_A"), None);
        assert_eq!(parse_action("use_item_1"), None);
        assert_eq!(parse_action("use_item_"), None);
        assert_eq!(parse_action("drop_item_Z"), None);
        assert_eq!(parse_action("equip_item_"), None);
    }

    #[test]
    fn parse_combine_a_b() {
        assert_eq!(
            parse_action("combine_a_b"),
            Some(GameCommand::Combine(0, 1))
        );
    }

    #[test]
    fn parse_combine_z_a() {
        assert_eq!(
            parse_action("combine_z_a"),
            Some(GameCommand::Combine(25, 0))
        );
    }

    #[test]
    fn parse_combine_invalid() {
        assert_eq!(parse_action("combine_a"), None);
        assert_eq!(parse_action("combine_"), None);
        assert_eq!(parse_action("combine_A_b"), None);
        assert_eq!(parse_action("combine_ab"), None);
    }
}
