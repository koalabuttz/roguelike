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
use roguelike_core::seed_code;
use roguelike_core::types::{Coord, Pos};

use crate::spectate::SpectatorWriter;

/// Per-session state: game state plus configuration set at `new_game` time.
struct GameSession {
    state: GameState,
    /// Omit ASCII map from observations to reduce response size.
    compact: bool,
    /// Cached hash of the exploration graph inputs. When `None`, the graph has
    /// never been sent. When `Some(h)`, `h` is the fingerprint from the last
    /// time a full graph was injected.
    last_graph_hash: Option<u64>,
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
    spectator: Arc<SpectatorWriter>,
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
            spectator: Arc::new(SpectatorWriter::new()),
            game_data: Arc::new(data::load_game_data()),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Start a new roguelike game. Returns the initial game state observation. Call this before any other tool."
    )]
    async fn new_game(
        &self,
        Parameters(params): Parameters<NewGameParams>,
    ) -> Result<CallToolResult, McpError> {
        let compact = params.compact.unwrap_or(false);

        let gd = &self.game_data;
        let mut state = if let Some(ref code) = params.seed_code {
            let decoded = seed_code::decode(code)
                .map_err(|e| McpError::invalid_params(format!("Invalid seed code: {e}"), None))?;

            if decoded.width < 20 || decoded.height < 15 {
                return Err(McpError::invalid_params(
                    "Map must be at least 20x15 tiles",
                    None,
                ));
            }

            if let Some(preset) = decoded.preset {
                GameState::with_preset_data(decoded.width, decoded.height, decoded.seed, preset, gd)
            } else {
                GameState::with_data(decoded.width, decoded.height, decoded.seed, gd)
            }
        } else {
            let width = params.width.unwrap_or(80);
            let height = params.height.unwrap_or(40);

            if width < 20 || height < 15 {
                return Err(McpError::invalid_params(
                    "Map must be at least 20x15 tiles",
                    None,
                ));
            }

            match params.seed {
                Some(seed) => GameState::with_data(width, height, seed, gd),
                None => GameState::new_with_data(width, height, gd),
            }
        };
        state.update_fov();
        let observation = state.observe();
        let mut json_value = serde_json::to_value(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let mut last_graph_hash = None;
        inject_exploration_graph_delta(&mut json_value, &state, &mut last_graph_hash, true);
        self.spectator.write_frame(&state);
        *self.session.lock().await = Some(GameSession {
            state,
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
    async fn observe(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let observation = session.state.observe();
        let json = serialize_observation(
            &observation,
            session.compact,
            &session.state,
            &mut session.last_graph_hash,
        )?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Take an action in the game. Valid actions: 'move_north', 'move_south', 'move_east', 'move_west', 'move_northeast', 'move_northwest', 'move_southeast', 'move_southwest', 'wait'. Moving into a monster attacks it. Returns the resulting game state after the action and any monster turns. Also supports autorun: 'autorun_north', 'autorun_south', 'autorun_east', 'autorun_west', 'autorun_northeast', 'autorun_northwest', 'autorun_southeast', 'autorun_southwest'. Autorun keeps moving in that direction until hitting a wall, spotting a new monster, taking damage, or reaching a corridor junction/room entrance. Use autorun to traverse long corridors efficiently. Also supports 'auto_fight' to resolve combat with an adjacent monster in one call — fights the weakest adjacent monster to the death. Response includes game stats: kills, rooms_found, explored."
    )]
    async fn act(
        &self,
        Parameters(params): Parameters<ActParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.state.game_over {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;

        // Auto-fight: resolve adjacent combat in one call.
        if params.action == "auto_fight" {
            let fight_result = session
                .state
                .auto_fight()
                .map_err(|e| McpError::invalid_request(e, None))?;
            self.spectator.write_frame(&session.state);
            let observation = session.state.observe();
            let json = format_auto_fight_response(
                &observation,
                &fight_result,
                compact,
                &session.state,
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
                     move_southeast, move_southwest, wait, \
                     autorun_north, autorun_south, autorun_east, autorun_west, \
                     autorun_northeast, autorun_northwest, autorun_southeast, \
                     autorun_southwest, auto_fight",
                    params.action
                ),
                None,
            )
        })?;

        // Autorun: loop internally and return final state with metadata.
        if let GameCommand::Autorun { dx, dy } = cmd {
            let autorun_result = session.state.autorun(dx, dy);
            self.spectator.write_frame(&session.state);
            let observation = session.state.observe();
            let frontiers = session.state.frontier_tiles();
            let json = format_response(
                &observation,
                &autorun_result,
                &frontiers,
                compact,
                &session.state,
                &mut session.last_graph_hash,
            )
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let explored_before = session.state.explored.len() as i32;
        let step_result = session.state.step(cmd);
        self.spectator.write_frame(&session.state);
        let new_tiles_revealed = session.state.explored.len() as i32 - explored_before;
        let observation = session.state.observe();
        let frontiers = session.state.frontier_tiles();
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
            &session.state,
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
    async fn get_explored_map(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let map_lines = session.state.explored_map();
        let player = &session.state.entities[0];
        let frontier_tiles = session.state.frontier_tiles();
        let frontier_exits: Vec<serde_json::Value> = frontier_tiles
            .iter()
            .map(|&(x, y)| serde_json::json!({"x": x, "y": y}))
            .collect();
        let mut response = serde_json::json!({
            "explored_map": map_lines,
            "x": player.x,
            "y": player.y,
            "frontier_exits": frontier_exits,
        });
        inject_exploration_graph_delta(
            &mut response,
            &session.state,
            &mut session.last_graph_hash,
            true,
        );
        let json = serde_json::to_string(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Pathfind to a target tile using A*. The player automatically walks the shortest path through explored tiles, stopping for monsters, damage, or reaching the target. Use this instead of multiple move commands to navigate to a visible or previously-explored location."
    )]
    async fn pathfind_to(
        &self,
        Parameters(params): Parameters<PathfindParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.state.game_over {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;
        let pathfind_result = session
            .state
            .pathfind_to(params.x, params.y)
            .map_err(|e| McpError::invalid_request(e, None))?;
        self.spectator.write_frame(&session.state);
        let observation = session.state.observe();
        let frontiers = session.state.frontier_tiles();
        let json = format_response(
            &observation,
            &pathfind_result,
            &frontiers,
            compact,
            &session.state,
            &mut session.last_graph_hash,
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Automatically explore the dungeon. Finds the nearest frontier tile (edge of explored area) and pathfinds to it. Equivalent to get_explored_map + pathfind_to in one call. Stops for monsters, damage, or when the frontier is reached. Returns observation with frontiers count, new_tiles revealed, and target_x/target_y explore coordinates."
    )]
    async fn auto_explore(&self) -> Result<CallToolResult, McpError> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if session.state.game_over {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let compact = session.compact;
        let explore_result = session
            .state
            .auto_explore()
            .map_err(|e| McpError::invalid_request(e, None))?;
        self.spectator.write_frame(&session.state);
        let observation = session.state.observe();
        let frontier_count = session.state.frontier_tiles().len() as i32;
        let json = format_auto_explore_response(
            &observation,
            &explore_result,
            frontier_count,
            compact,
            &session.state,
            &mut session.last_graph_hash,
        )
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Save the current game state. Stores the game in an in-memory save slot (one slot, overwrites previous save). Returns turn count, HP, seed, and save size."
    )]
    async fn save_game(&self) -> Result<CallToolResult, McpError> {
        // Lock session, serialize, drop lock before acquiring save_slot lock.
        let json = {
            let guard = self.session.lock().await;
            let session = guard.as_ref().ok_or_else(|| {
                McpError::invalid_request("No game in progress. Call new_game first.", None)
            })?;
            let state = &session.state;
            let json = state.save_to_json().map_err(|e| {
                McpError::internal_error(format!("Serialization failed: {e}"), None)
            })?;
            let player = &state.entities[0];
            let info = serde_json::json!({
                "saved": true,
                "turn_count": state.turn_count,
                "hp": player.hp,
                "max_hp": player.max_hp,
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
    async fn load_game(&self) -> Result<CallToolResult, McpError> {
        // Lock save_slot, clone JSON, drop save_slot lock before acquiring session lock.
        let save_json = {
            let guard = self.save_slot.lock().await;
            guard.as_ref().cloned().ok_or_else(|| {
                McpError::invalid_request("No saved game. Call save_game first.", None)
            })?
        };

        let loaded = GameState::load_from_json(&save_json)
            .map_err(|e| McpError::internal_error(format!("Deserialization failed: {e}"), None))?;
        let observation = loaded.observe();
        let mut value = serde_json::to_value(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert("loaded".into(), serde_json::Value::Bool(true));
        }
        let mut last_graph_hash = None;
        inject_exploration_graph_delta(&mut value, &loaded, &mut last_graph_hash, true);
        self.spectator.write_frame(&loaded);
        // Preserve compact setting from the current session.
        let mut guard = self.session.lock().await;
        let compact = guard.as_ref().map(|s| s.compact).unwrap_or(false);
        *guard = Some(GameSession {
            state: loaded,
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
    async fn get_rules(&self) -> Result<CallToolResult, McpError> {
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
             ## Strategy Tips\n\
             - Fight in corridors to face one monster at a time.\n\
             - You regenerate 1 HP every {regen} turns. Retreat and move to recover.\n\
             \n\
             ## Available Tools\n\
             - **act** — move, wait, autorun, or auto_fight (see below)\n\
             - **observe** — see current FOV, stats, and nearby entities. \
             Rarely needed since act, pathfind_to, auto_explore, and auto_fight \
             already return observations.\n\
             - **auto_explore** — find nearest frontier and walk to it in one call. \
             Best way to explore the dungeon. Returns frontier_exits for next move.\n\
             - **pathfind_to(x, y)** — walk shortest path to any explored tile; \
             stops for monsters, damage, or on arrival\n\
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
    match action {
        "move_north" => Some(GameCommand::Move { dx: 0, dy: -1 }),
        "move_south" => Some(GameCommand::Move { dx: 0, dy: 1 }),
        "move_east" => Some(GameCommand::Move { dx: 1, dy: 0 }),
        "move_west" => Some(GameCommand::Move { dx: -1, dy: 0 }),
        "move_northeast" => Some(GameCommand::Move { dx: 1, dy: -1 }),
        "move_northwest" => Some(GameCommand::Move { dx: -1, dy: -1 }),
        "move_southeast" => Some(GameCommand::Move { dx: 1, dy: 1 }),
        "move_southwest" => Some(GameCommand::Move { dx: -1, dy: 1 }),
        "autorun_north" => Some(GameCommand::Autorun { dx: 0, dy: -1 }),
        "autorun_south" => Some(GameCommand::Autorun { dx: 0, dy: 1 }),
        "autorun_east" => Some(GameCommand::Autorun { dx: 1, dy: 0 }),
        "autorun_west" => Some(GameCommand::Autorun { dx: -1, dy: 0 }),
        "autorun_northeast" => Some(GameCommand::Autorun { dx: 1, dy: -1 }),
        "autorun_northwest" => Some(GameCommand::Autorun { dx: -1, dy: -1 }),
        "autorun_southeast" => Some(GameCommand::Autorun { dx: 1, dy: 1 }),
        "autorun_southwest" => Some(GameCommand::Autorun { dx: -1, dy: 1 }),
        "wait" => Some(GameCommand::Wait),
        _ => None,
    }
}

/// Build a JSON response that merges the observation with auto-fight metadata.
fn format_auto_fight_response(
    observation: &roguelike_core::game::GameObservation,
    fight: &AutoFightResult,
    compact: bool,
    state: &GameState,
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
    state: &GameState,
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
    state: &GameState,
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
    state: &GameState,
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
/// When `force` is true, always builds and injects the full graph.
/// When `force` is false, compares the fingerprint to the cached hash:
/// - Same → injects `"exploration_unchanged": true` (skips expensive A*)
/// - Different → builds full graph and updates the hash
fn inject_exploration_graph_delta(
    value: &mut serde_json::Value,
    state: &GameState,
    last_hash: &mut Option<u64>,
    force: bool,
) {
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

    #[test]
    fn parse_all_valid_actions() {
        assert_eq!(
            parse_action("move_north"),
            Some(GameCommand::Move { dx: 0, dy: -1 })
        );
        assert_eq!(
            parse_action("move_south"),
            Some(GameCommand::Move { dx: 0, dy: 1 })
        );
        assert_eq!(
            parse_action("move_east"),
            Some(GameCommand::Move { dx: 1, dy: 0 })
        );
        assert_eq!(
            parse_action("move_west"),
            Some(GameCommand::Move { dx: -1, dy: 0 })
        );
        assert_eq!(
            parse_action("move_northeast"),
            Some(GameCommand::Move { dx: 1, dy: -1 })
        );
        assert_eq!(
            parse_action("move_northwest"),
            Some(GameCommand::Move { dx: -1, dy: -1 })
        );
        assert_eq!(
            parse_action("move_southeast"),
            Some(GameCommand::Move { dx: 1, dy: 1 })
        );
        assert_eq!(
            parse_action("move_southwest"),
            Some(GameCommand::Move { dx: -1, dy: 1 })
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
            Some(GameCommand::Autorun { dx: 0, dy: -1 })
        );
        assert_eq!(
            parse_action("autorun_south"),
            Some(GameCommand::Autorun { dx: 0, dy: 1 })
        );
        assert_eq!(
            parse_action("autorun_east"),
            Some(GameCommand::Autorun { dx: 1, dy: 0 })
        );
        assert_eq!(
            parse_action("autorun_west"),
            Some(GameCommand::Autorun { dx: -1, dy: 0 })
        );
        assert_eq!(
            parse_action("autorun_northeast"),
            Some(GameCommand::Autorun { dx: 1, dy: -1 })
        );
        assert_eq!(
            parse_action("autorun_northwest"),
            Some(GameCommand::Autorun { dx: -1, dy: -1 })
        );
        assert_eq!(
            parse_action("autorun_southeast"),
            Some(GameCommand::Autorun { dx: 1, dy: 1 })
        );
        assert_eq!(
            parse_action("autorun_southwest"),
            Some(GameCommand::Autorun { dx: -1, dy: 1 })
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
                matches!(parse_action(dir), Some(GameCommand::Autorun { .. })),
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
        };
        state.update_fov();

        let fight = state.auto_fight().unwrap();
        let obs = state.observe();
        let mut hash = None;
        let json_str = format_auto_fight_response(&obs, &fight, false, &state, &mut hash).unwrap();
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
        inject_exploration_graph_delta(&mut value, &gs, &mut hash, true);
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
        inject_exploration_graph_delta(&mut v1, &gs, &mut hash, true);
        assert!(v1.get("exploration").is_some());

        // Second call: same state, not forced → should skip.
        let mut v2 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v2, &gs, &mut hash, false);
        assert!(v2.get("exploration_unchanged").is_some());
        assert!(v2.get("exploration").is_none());
    }

    #[test]
    fn delta_rebuilds_graph_after_state_change() {
        let mut gs = make_test_state();
        let mut hash = None;
        // First call: force to set the hash.
        let mut v1 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v1, &gs, &mut hash, true);

        // Mutate state: kill a monster.
        if let Some(e) = gs.entities.iter_mut().skip(1).find(|e| e.alive) {
            e.alive = false;
        }

        // Third call: state changed, not forced → should rebuild.
        let mut v2 = serde_json::json!({});
        inject_exploration_graph_delta(&mut v2, &gs, &mut hash, false);
        assert!(v2.get("exploration").is_some());
        assert!(v2.get("exploration_unchanged").is_none());
    }
}
