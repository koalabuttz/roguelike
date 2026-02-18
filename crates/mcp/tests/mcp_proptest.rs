//! Property-based tests for the MCP session layer.
//!
//! Generates random sequences of MCP tool calls and verifies that
//! game invariants hold through the JSON interface. Tests mutex logic,
//! serialization, and error handling that the core invariant tests don't cover.

use proptest::prelude::*;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use roguelike_mcp::mcp_server::{
    ActParams, LookAtParams, NewGameParams, PathfindParams, RoguelikeMcpServer,
};

const WIDTH: i32 = 80;
const HEIGHT: i32 = 40;

/// MCP tool actions that the property tests randomly select from.
#[derive(Debug, Clone)]
enum McpAction {
    Act(String),
    Observe,
    AutoExplore,
    PathfindTo(i32, i32),
    GetExploredMap,
    LookAt(i32, i32),
    SaveGame,
    LoadGame,
}

/// Generate a random action string (valid act commands).
fn arb_act_action() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => Just("move_north".to_string()),
        3 => Just("move_south".to_string()),
        3 => Just("move_east".to_string()),
        3 => Just("move_west".to_string()),
        1 => Just("move_northeast".to_string()),
        1 => Just("move_northwest".to_string()),
        1 => Just("move_southeast".to_string()),
        1 => Just("move_southwest".to_string()),
        2 => Just("wait".to_string()),
        2 => Just("autorun_north".to_string()),
        2 => Just("autorun_south".to_string()),
        2 => Just("autorun_east".to_string()),
        2 => Just("autorun_west".to_string()),
        1 => Just("auto_fight".to_string()),
    ]
}

/// Generate a random MCP action with realistic weights.
fn arb_mcp_action() -> impl Strategy<Value = McpAction> {
    prop_oneof![
        8 => arb_act_action().prop_map(McpAction::Act),
        1 => Just(McpAction::Observe),
        3 => Just(McpAction::AutoExplore),
        2 => (0..WIDTH, 0..HEIGHT).prop_map(|(x, y)| McpAction::PathfindTo(x, y)),
        1 => Just(McpAction::GetExploredMap),
        1 => (0..WIDTH, 0..HEIGHT).prop_map(|(x, y)| McpAction::LookAt(x, y)),
        1 => Just(McpAction::SaveGame),
        1 => Just(McpAction::LoadGame),
    ]
}

/// Actions that don't affect the save slot — for use between save and load.
fn arb_mcp_action_no_save_load() -> impl Strategy<Value = McpAction> {
    prop_oneof![
        8 => arb_act_action().prop_map(McpAction::Act),
        1 => Just(McpAction::Observe),
        3 => Just(McpAction::AutoExplore),
        2 => (0..WIDTH, 0..HEIGHT).prop_map(|(x, y)| McpAction::PathfindTo(x, y)),
        1 => Just(McpAction::GetExploredMap),
        1 => (0..WIDTH, 0..HEIGHT).prop_map(|(x, y)| McpAction::LookAt(x, y)),
    ]
}

/// Execute an MCP action on the server. Returns Ok(json) or Err(error_string).
async fn execute_action(
    server: &RoguelikeMcpServer,
    action: &McpAction,
) -> Result<serde_json::Value, String> {
    let result: Result<CallToolResult, _> = match action {
        McpAction::Act(a) => {
            server
                .act(Parameters(ActParams { action: a.clone() }))
                .await
        }
        McpAction::Observe => server.observe().await,
        McpAction::AutoExplore => server.auto_explore().await,
        McpAction::PathfindTo(x, y) => {
            server
                .pathfind_to(Parameters(PathfindParams { x: *x, y: *y }))
                .await
        }
        McpAction::GetExploredMap => server.get_explored_map().await,
        McpAction::LookAt(x, y) => {
            server
                .look_at(Parameters(LookAtParams { x: *x, y: *y }))
                .await
        }
        McpAction::SaveGame => server.save_game().await,
        McpAction::LoadGame => server.load_game().await,
    };

    match result {
        Ok(r) => {
            let text = r
                .content
                .first()
                .and_then(|c| c.as_text())
                .map(|t| t.text.clone())
                .ok_or_else(|| "no text content".to_string())?;
            // get_rules returns non-JSON text, so handle that case.
            serde_json::from_str(&text).map_err(|_| "not json".to_string())
        }
        Err(e) => Err(format!("{:?}", e)),
    }
}

/// Start a game and return the initial observation.
async fn start_game(server: &RoguelikeMcpServer, seed: u64) -> serde_json::Value {
    let result = server
        .new_game(Parameters(NewGameParams {
            width: Some(WIDTH),
            height: Some(HEIGHT),
            seed: Some(seed),
            compact: None,
            seed_code: None,
        }))
        .await
        .expect("new_game should succeed");
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.as_str())
        .unwrap();
    serde_json::from_str(text).unwrap()
}

/// Tracks game invariants across a sequence of MCP tool calls.
struct InvariantTracker {
    max_explored_pct: i64,
    had_save: bool,
}

impl InvariantTracker {
    fn new() -> Self {
        Self {
            max_explored_pct: 0,
            had_save: false,
        }
    }

    fn reset_on_load(&mut self) {
        // After load, explored_pct might be lower than peak.
        self.max_explored_pct = 0;
    }

    fn check(&mut self, json: &serde_json::Value, action: &McpAction) {
        // Only check observation-like responses (those with hp/max_hp).
        // Responses from get_explored_map, look_at, save_game, etc. have
        // different schemas and are skipped.
        let hp = json.get("hp").and_then(|v| v.as_i64());
        let max_hp = json.get("max_hp").and_then(|v| v.as_i64());
        let game_over = json.get("game_over").and_then(|v| v.as_bool());
        let explored_pct = json.get("explored").and_then(|v| v.as_i64());

        // Handle save/load FIRST — load resets the explored_pct baseline.
        if matches!(action, McpAction::SaveGame) {
            self.had_save = true;
        }
        if matches!(action, McpAction::LoadGame) {
            self.reset_on_load();
            if let Some(pct) = explored_pct {
                self.max_explored_pct = pct;
            }
            // Skip monotonicity check — load legitimately reduces explored_pct.
            return;
        }

        // Check HP <= max_hp.
        if let (Some(hp), Some(max_hp)) = (hp, max_hp) {
            assert!(
                hp <= max_hp,
                "HP {} > max_hp {} after {:?}",
                hp,
                max_hp,
                action
            );
        }

        // Check game_over consistency with HP.
        if let (Some(hp), Some(game_over)) = (hp, game_over) {
            if game_over {
                assert!(hp <= 0, "game_over but HP is {} after {:?}", hp, action);
            }
            if hp <= 0 {
                assert!(game_over, "HP {} but not game_over after {:?}", hp, action);
            }
        }

        // Check explored_pct never decreases (except after load, handled above).
        if let Some(pct) = explored_pct {
            assert!(
                pct >= self.max_explored_pct,
                "explored_pct decreased from {} to {} after {:?}",
                self.max_explored_pct,
                pct,
                action,
            );
            self.max_explored_pct = pct;
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn random_tool_sequences_preserve_invariants(
        seed in any::<u64>(),
        actions in proptest::collection::vec(arb_mcp_action(), 20..=100),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = RoguelikeMcpServer::new();
            start_game(&server, seed).await;

            let mut tracker = InvariantTracker::new();

            for action in &actions {
                match execute_action(&server, action).await {
                    Ok(json) => {
                        tracker.check(&json, action);
                    }
                    Err(_) => {
                        // Errors are expected (game over, no path, etc.)
                        // The important thing is no panics.
                    }
                }
            }
        });
    }

    #[test]
    fn save_load_roundtrip_preserves_state(
        seed in any::<u64>(),
        pre_actions in proptest::collection::vec(arb_mcp_action_no_save_load(), 5..=30),
        post_actions in proptest::collection::vec(arb_mcp_action_no_save_load(), 5..=20),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = RoguelikeMcpServer::new();
            start_game(&server, seed).await;

            // Run some actions.
            for action in &pre_actions {
                let _ = execute_action(&server, action).await;
            }

            // Observe state, then save.
            let pre_save = execute_action(&server, &McpAction::Observe).await;
            let _ = execute_action(&server, &McpAction::SaveGame).await;

            // Run more actions (potentially changing state).
            for action in &post_actions {
                let _ = execute_action(&server, action).await;
            }

            // Load and verify state matches pre-save.
            if let Ok(loaded) = execute_action(&server, &McpAction::LoadGame).await {
                if let Ok(ref saved) = pre_save {
                    // HP and position should match.
                    if let (Some(saved_hp), Some(loaded_hp)) = (saved.get("hp"), loaded.get("hp")) {
                        assert_eq!(saved_hp, loaded_hp, "HP mismatch after load");
                    }
                    if let (Some(saved_x), Some(loaded_x)) = (saved.get("x"), loaded.get("x")) {
                        assert_eq!(saved_x, loaded_x, "X mismatch after load");
                    }
                    if let (Some(saved_y), Some(loaded_y)) = (saved.get("y"), loaded.get("y")) {
                        assert_eq!(saved_y, loaded_y, "Y mismatch after load");
                    }
                }
            }
        });
    }

    #[test]
    fn no_tool_call_panics(
        seed in any::<u64>(),
        skip_new_game in prop::bool::ANY,
        actions in proptest::collection::vec(arb_mcp_action(), 10..=50),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = RoguelikeMcpServer::new();

            // 1/3 of runs intentionally skip new_game.
            if !skip_new_game {
                start_game(&server, seed).await;
            }

            // Every tool call should either succeed or return an error — never panic.
            for action in &actions {
                let _ = execute_action(&server, action).await;
            }
        });
    }
}
