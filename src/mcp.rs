use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use tokio::sync::Mutex;

use crate::data::CONFIG;
use crate::game::{AutorunResult, GameState};
use crate::input::GameCommand;

/// MCP server that wraps a roguelike game session.
///
/// Holds an `Option<GameState>` behind a mutex: `None` until `new_game` is
/// called, then `Some(state)` for the duration of the game. Calling `new_game`
/// again resets the state.
#[derive(Clone)]
pub struct RoguelikeMcpServer {
    state: Arc<Mutex<Option<GameState>>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewGameParams {
    /// Map width in tiles. Defaults to 80 if not specified.
    pub width: Option<i32>,
    /// Map height in tiles. Defaults to 40 if not specified.
    pub height: Option<i32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ActParams {
    /// The action to take. One of: "move_north", "move_south", "move_east",
    /// "move_west", "move_northeast", "move_northwest", "move_southeast",
    /// "move_southwest", "wait".
    pub action: String,
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
            state: Arc::new(Mutex::new(None)),
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
        let width = params.width.unwrap_or(80);
        let height = params.height.unwrap_or(40);

        if width < 20 || height < 15 {
            return Err(McpError::invalid_params(
                "Map must be at least 20x15 tiles",
                None,
            ));
        }

        let mut state = GameState::new(width, height);
        state.update_fov();
        let observation = state.observe();
        *self.state.lock().await = Some(state);

        let json = serde_json::to_string_pretty(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Observe the current visible game state. Returns player stats, an ASCII map of visible tiles, a list of visible monsters with their stats, and the recent message log."
    )]
    async fn observe(&self) -> Result<CallToolResult, McpError> {
        let guard = self.state.lock().await;
        let state = guard.as_ref().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        let observation = state.observe();
        let json = serde_json::to_string_pretty(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Take an action in the game. Valid actions: 'move_north', 'move_south', 'move_east', 'move_west', 'move_northeast', 'move_northwest', 'move_southeast', 'move_southwest', 'wait'. Moving into a monster attacks it. Returns the resulting game state after the action and any monster turns. Also supports autorun: 'autorun_north', 'autorun_south', 'autorun_east', 'autorun_west', 'autorun_northeast', 'autorun_northwest', 'autorun_southeast', 'autorun_southwest'. Autorun keeps moving in that direction until hitting a wall, spotting a new monster, taking damage, or reaching a corridor junction/room entrance. Use autorun to traverse long corridors efficiently."
    )]
    async fn act(
        &self,
        Parameters(params): Parameters<ActParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut guard = self.state.lock().await;
        let state = guard.as_mut().ok_or_else(|| {
            McpError::invalid_request("No game in progress. Call new_game first.", None)
        })?;

        if state.game_over {
            return Err(McpError::invalid_request(
                "Game is over. Call new_game to start a new game.",
                None,
            ));
        }

        let cmd = parse_action(&params.action).ok_or_else(|| {
            McpError::invalid_params(
                format!(
                    "Unknown action '{}'. Valid actions: move_north, move_south, \
                     move_east, move_west, move_northeast, move_northwest, \
                     move_southeast, move_southwest, wait, \
                     autorun_north, autorun_south, autorun_east, autorun_west, \
                     autorun_northeast, autorun_northwest, autorun_southeast, \
                     autorun_southwest",
                    params.action
                ),
                None,
            )
        })?;

        // Autorun: loop internally and return final state with metadata.
        if let GameCommand::Autorun { dx, dy } = cmd {
            let autorun_result = state.autorun(dx, dy);
            let observation = state.observe();
            let json = format_autorun_response(&observation, &autorun_result)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let _step_result = state.step(cmd);
        let observation = state.observe();
        let json = serde_json::to_string_pretty(&observation)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the rules and mechanics of the roguelike game. Explains combat, movement, monsters, and field of view. Call this to understand the game before playing."
    )]
    async fn get_rules(&self) -> Result<CallToolResult, McpError> {
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
             - '%%' = corpse (walkable, dead monster)\n\
             - '@' = you (the player)\n\
             - Lowercase letters = monsters (g=goblin, o=orc)\n\
             - Uppercase letters = dangerous monsters (T=troll)\n\
             \n\
             ## Field of View\n\
             You can only see tiles within radius {}. Monsters beyond your FOV\n\
             are hidden. Monsters only chase you once they enter your FOV.\n\
             \n\
             ## Combat\n\
             Damage = attacker's ATK - defender's DEF (minimum 0).\n\
             If damage > 0, defender loses that many HP. At 0 HP, entity dies.\n\
             \n\
             ## Your Stats\n\
             HP: 30, ATK: 5, DEF: 2\n\
             \n\
             ## Monsters\n\
             - Goblin (g): HP 6, ATK 3, DEF 0 -- weak, common\n\
             - Orc (o): HP 12, ATK 4, DEF 1 -- moderate threat\n\
             - Troll (T): HP 20, ATK 6, DEF 3 -- dangerous, very tanky\n\
             \n\
             ## Strategy Tips\n\
             - Fight in corridors to face one monster at a time.\n\
             - Goblins deal 1 dmg/turn to you. You kill them in 2 hits.\n\
             - Orcs deal 2 dmg/turn. 3 hits to kill.\n\
             - Trolls deal 4 dmg/turn but take 10 hits to kill. Avoid if low HP.\n\
             - You have no way to heal yet. Every point of HP matters.",
            CONFIG.fov_radius
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

/// Build a JSON response that merges the observation with autorun metadata.
fn format_autorun_response(
    observation: &crate::game::GameObservation,
    autorun: &AutorunResult,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(observation)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert(
            "autorun_steps".into(),
            serde_json::Value::Number(autorun.steps_taken.into()),
        );
        map.insert(
            "autorun_stop_reason".into(),
            serde_json::to_value(autorun.stop_reason)?,
        );
    }
    serde_json::to_string_pretty(&value)
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
}
