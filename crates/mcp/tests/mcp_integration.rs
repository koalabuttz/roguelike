//! Deterministic integration tests for MCP tool methods.
//!
//! Each test creates a fresh `RoguelikeMcpServer`, calls tool methods directly,
//! and verifies JSON responses — the same surface an MCP client sees.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use roguelike_mcp::mcp_server::{
    ActParams, LookAtParams, NewGameParams, PathfindParams, RoguelikeMcpServer,
};

const SEED: u64 = 42;

/// Start a game with a fixed seed and default 80x40 map.
async fn start_game(server: &RoguelikeMcpServer) -> serde_json::Value {
    start_game_with_seed(server, SEED).await
}

async fn start_game_with_seed(server: &RoguelikeMcpServer, seed: u64) -> serde_json::Value {
    let result = server
        .new_game(Parameters(NewGameParams {
            width: Some(80),
            height: Some(40),
            seed: Some(seed),
            compact: None,
            seed_code: None,
        }))
        .await
        .expect("new_game should succeed");
    extract_json(&result)
}

/// Extract JSON value from a CallToolResult's first text content.
fn extract_json(result: &CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.as_str())
        .expect("expected text content in response");
    serde_json::from_str(text).expect("response should be valid JSON")
}

/// Helper to call act with a given action string.
async fn do_act(
    server: &RoguelikeMcpServer,
    action: &str,
) -> Result<CallToolResult, rmcp::ErrorData> {
    server
        .act(Parameters(ActParams {
            action: action.to_string(),
        }))
        .await
}

// ─────────────────────────────────────────────────────────────────────
// Tool response tests
// ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn new_game_returns_valid_observation() {
    let server = RoguelikeMcpServer::new();
    let json = start_game(&server).await;

    assert!(json.get("hp").is_some(), "missing hp");
    assert!(json.get("max_hp").is_some(), "missing max_hp");
    assert!(json.get("x").is_some(), "missing x");
    assert!(json.get("y").is_some(), "missing y");
    assert!(json.get("map").is_some(), "missing map");
    assert!(json.get("entities").is_some(), "missing entities");
    assert!(json.get("game_over").is_some(), "missing game_over");
    assert!(json.get("seed").is_some(), "missing seed");
    assert!(json.get("seed_code").is_some(), "missing seed_code");

    assert_eq!(json["game_over"], false);
    assert_eq!(json["seed"], SEED);
    assert!(json["hp"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn new_game_with_seed_code() {
    let server = RoguelikeMcpServer::new();

    // First, start a game to get its seed_code.
    let json1 = start_game(&server).await;
    let seed_code = json1["seed_code"].as_str().unwrap().to_string();

    // Start a new game using that seed_code.
    let result = server
        .new_game(Parameters(NewGameParams {
            width: None,
            height: None,
            seed: None,
            compact: None,
            seed_code: Some(seed_code),
        }))
        .await
        .expect("seed_code game should succeed");
    let json2 = extract_json(&result);

    // Same seed should produce same starting position.
    assert_eq!(json1["seed"], json2["seed"]);
    assert_eq!(json1["x"], json2["x"]);
    assert_eq!(json1["y"], json2["y"]);
}

#[tokio::test]
async fn new_game_compact_mode_omits_map() {
    let server = RoguelikeMcpServer::new();
    let result = server
        .new_game(Parameters(NewGameParams {
            width: Some(80),
            height: Some(40),
            seed: Some(SEED),
            compact: Some(true),
            seed_code: None,
        }))
        .await
        .expect("compact new_game should succeed");
    let json = extract_json(&result);

    assert!(json.get("map").is_none(), "compact mode should omit map");
    assert!(json.get("hp").is_some(), "should still have hp");
}

#[tokio::test]
async fn new_game_rejects_undersized_maps() {
    let server = RoguelikeMcpServer::new();

    let err = server
        .new_game(Parameters(NewGameParams {
            width: Some(10),
            height: Some(10),
            seed: Some(SEED),
            compact: None,
            seed_code: None,
        }))
        .await;
    assert!(err.is_err(), "undersized map should be rejected");
}

#[tokio::test]
async fn observe_returns_observation_after_game_start() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    let result = server.observe().await.expect("observe should succeed");
    let json = extract_json(&result);

    assert!(json.get("hp").is_some());
    assert!(json.get("map").is_some());
    assert!(json.get("game_over").is_some());
}

#[tokio::test]
async fn act_move_changes_position_or_hits_wall() {
    let server = RoguelikeMcpServer::new();
    let initial = start_game(&server).await;
    let x0 = initial["x"].as_i64().unwrap();
    let y0 = initial["y"].as_i64().unwrap();

    // Try all four cardinal directions — at least one should succeed
    // (player starts in a room, so some direction must be walkable).
    let mut moved = false;
    for action in &["move_north", "move_south", "move_east", "move_west"] {
        // Re-start to get consistent initial position.
        start_game(&server).await;
        let result = do_act(&server, action).await.expect("act should succeed");
        let json = extract_json(&result);
        let x = json["x"].as_i64().unwrap();
        let y = json["y"].as_i64().unwrap();
        if x != x0 || y != y0 {
            moved = true;
            break;
        }
    }
    assert!(
        moved,
        "player should be able to move in at least one direction"
    );
}

#[tokio::test]
async fn act_autorun_returns_steps_and_stop_reason() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    // Try autorun in multiple directions until one actually takes steps.
    for dir in &[
        "autorun_north",
        "autorun_south",
        "autorun_east",
        "autorun_west",
    ] {
        start_game(&server).await;
        let result = do_act(&server, dir).await.expect("autorun should succeed");
        let json = extract_json(&result);
        assert!(json.get("steps").is_some(), "autorun should have steps");
        assert!(
            json.get("stop_reason").is_some(),
            "autorun should have stop_reason"
        );
    }
}

#[tokio::test]
async fn act_auto_fight_returns_fight_metadata() {
    let server = RoguelikeMcpServer::new();

    // Use a seed loop to find one where a monster is adjacent after some moves.
    // We try different seeds because monster placement varies.
    let mut found_fight = false;
    for seed in 1..100u64 {
        start_game_with_seed(&server, seed).await;

        // Auto-explore to get near monsters, then try auto_fight.
        for _ in 0..10 {
            let _ = server.auto_explore().await;
        }
        if let Ok(result) = do_act(&server, "auto_fight").await {
            let json = extract_json(&result);
            if json.get("fight_rounds").is_some() {
                assert!(json.get("fight_target").is_some());
                assert!(json.get("fight_target_killed").is_some());
                assert!(json.get("fight_hp_lost").is_some());
                found_fight = true;
                break;
            }
        }
    }
    assert!(found_fight, "should find a seed where auto_fight works");
}

#[tokio::test]
async fn act_invalid_action_returns_error() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    let err = do_act(&server, "fly_away").await;
    assert!(err.is_err(), "invalid action should return error");
}

#[tokio::test]
async fn pathfind_to_returns_autorun_result() {
    let server = RoguelikeMcpServer::new();
    let initial = start_game(&server).await;
    let px = initial["x"].as_i64().unwrap() as i32;
    let py = initial["y"].as_i64().unwrap() as i32;

    // Pathfind to a nearby tile (offset by 1 in a direction that should be floor).
    // Try a few offsets since the player is in a room.
    for (dx, dy) in &[(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let result = server
            .pathfind_to(Parameters(PathfindParams {
                x: px + dx,
                y: py + dy,
            }))
            .await;
        if let Ok(r) = result {
            let json = extract_json(&r);
            assert!(json.get("steps").is_some(), "pathfind should return steps");
            assert!(
                json.get("stop_reason").is_some(),
                "pathfind should return stop_reason"
            );
            return;
        }
    }
    panic!("pathfind should succeed to at least one adjacent tile");
}

#[tokio::test]
async fn auto_explore_returns_target_and_frontiers() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    let result = server
        .auto_explore()
        .await
        .expect("auto_explore should succeed");
    let json = extract_json(&result);

    assert!(json.get("target_x").is_some(), "missing target_x");
    assert!(json.get("target_y").is_some(), "missing target_y");
    assert!(json.get("frontiers").is_some(), "missing frontiers");
    assert!(json.get("steps").is_some(), "missing steps");
}

#[tokio::test]
async fn get_explored_map_returns_map_and_frontiers() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    let result = server
        .get_explored_map()
        .await
        .expect("get_explored_map should succeed");
    let json = extract_json(&result);

    assert!(json.get("explored_map").is_some(), "missing explored_map");
    assert!(
        json.get("frontier_exits").is_some(),
        "missing frontier_exits"
    );
}

#[tokio::test]
async fn look_at_returns_terrain_and_visibility() {
    let server = RoguelikeMcpServer::new();
    let initial = start_game(&server).await;
    let px = initial["x"].as_i64().unwrap() as i32;
    let py = initial["y"].as_i64().unwrap() as i32;

    let result = server
        .look_at(Parameters(LookAtParams { x: px, y: py }))
        .await
        .expect("look_at should succeed");
    let json = extract_json(&result);

    assert!(json.get("terrain").is_some(), "missing terrain");
    assert!(json.get("visible").is_some(), "missing visible");
    assert!(json.get("description").is_some(), "missing description");
}

#[tokio::test]
async fn save_load_roundtrip_preserves_state() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;

    // Take a few actions to change state.
    let _ = do_act(&server, "wait").await;
    let _ = do_act(&server, "wait").await;

    // Observe state before save.
    let before = extract_json(&server.observe().await.unwrap());
    let hp_before = before["hp"].as_i64().unwrap();
    let x_before = before["x"].as_i64().unwrap();
    let y_before = before["y"].as_i64().unwrap();

    // Save.
    let save_result = server.save_game().await.expect("save should succeed");
    let save_json = extract_json(&save_result);
    assert_eq!(save_json["saved"], true);

    // Take more actions (might change HP if monster nearby).
    for _ in 0..5 {
        let _ = do_act(&server, "wait").await;
    }

    // Load.
    let load_result = server.load_game().await.expect("load should succeed");
    let load_json = extract_json(&load_result);
    assert_eq!(load_json["loaded"], true);

    // State should match pre-save.
    assert_eq!(load_json["hp"].as_i64().unwrap(), hp_before);
    assert_eq!(load_json["x"].as_i64().unwrap(), x_before);
    assert_eq!(load_json["y"].as_i64().unwrap(), y_before);
}

#[tokio::test]
async fn get_rules_returns_rules_text() {
    let server = RoguelikeMcpServer::new();
    let result = server.get_rules().await.expect("get_rules should succeed");

    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.as_str())
        .expect("get_rules should return text");
    assert!(
        text.contains("Roguelike Game Rules"),
        "rules should contain header"
    );
    assert!(text.contains("Combat"), "rules should mention combat");
}

// ─────────────────────────────────────────────────────────────────────
// Error path tests
// ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn observe_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server.observe().await;
    assert!(err.is_err(), "observe without game should error");
}

#[tokio::test]
async fn act_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = do_act(&server, "move_north").await;
    assert!(err.is_err(), "act without game should error");
}

#[tokio::test]
async fn get_explored_map_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server.get_explored_map().await;
    assert!(err.is_err(), "get_explored_map without game should error");
}

#[tokio::test]
async fn look_at_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server
        .look_at(Parameters(LookAtParams { x: 0, y: 0 }))
        .await;
    assert!(err.is_err(), "look_at without game should error");
}

#[tokio::test]
async fn pathfind_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server
        .pathfind_to(Parameters(PathfindParams { x: 5, y: 5 }))
        .await;
    assert!(err.is_err(), "pathfind_to without game should error");
}

#[tokio::test]
async fn auto_explore_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server.auto_explore().await;
    assert!(err.is_err(), "auto_explore without game should error");
}

#[tokio::test]
async fn save_without_game_returns_error() {
    let server = RoguelikeMcpServer::new();
    let err = server.save_game().await;
    assert!(err.is_err(), "save without game should error");
}

#[tokio::test]
async fn load_without_save_returns_error() {
    let server = RoguelikeMcpServer::new();
    start_game(&server).await;
    let err = server.load_game().await;
    assert!(err.is_err(), "load without prior save should error");
}

#[tokio::test]
async fn act_after_game_over_returns_error() {
    let server = RoguelikeMcpServer::new();

    // Find a seed where the player dies quickly by actively exploring into monsters.
    let mut found_game_over = false;
    for seed in 1..50u64 {
        start_game_with_seed(&server, seed).await;

        // Actively explore to find and fight monsters.
        for _ in 0..200 {
            // Try auto_explore to move toward unexplored areas (and monsters).
            let _ = server.auto_explore().await;

            // Try moving in all directions to bump into monsters.
            for dir in &["move_north", "move_south", "move_east", "move_west"] {
                match do_act(&server, dir).await {
                    Err(_) => {
                        // Game over — verify subsequent act fails.
                        let err = do_act(&server, "move_north").await;
                        assert!(err.is_err(), "act after game over should error");
                        found_game_over = true;
                        break;
                    }
                    Ok(r) => {
                        let json = extract_json(&r);
                        if json["game_over"] == true {
                            let err = do_act(&server, "move_north").await;
                            assert!(err.is_err(), "act after game over should error");
                            found_game_over = true;
                            break;
                        }
                    }
                }
            }
            if found_game_over {
                break;
            }
        }
        if found_game_over {
            break;
        }
    }
    assert!(found_game_over, "should find a seed where the player dies");
}

// ─────────────────────────────────────────────────────────────────────
// Session lifecycle tests
// ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn new_game_resets_session() {
    let server = RoguelikeMcpServer::new();

    // Start game with seed 1.
    let json1 = start_game_with_seed(&server, 1).await;
    // Take some actions.
    let _ = do_act(&server, "wait").await;

    // Start a completely new game with different seed.
    let json2 = start_game_with_seed(&server, 99999).await;

    // The seeds should differ.
    assert_ne!(json1["seed"], json2["seed"]);
    // Fresh game should not be game_over.
    assert_eq!(json2["game_over"], false);
}

#[tokio::test]
async fn save_persists_across_new_game() {
    let server = RoguelikeMcpServer::new();

    // Start and save game 1.
    start_game_with_seed(&server, 1).await;
    let obs1 = extract_json(&server.observe().await.unwrap());
    server.save_game().await.expect("save should succeed");

    // Start a completely different game.
    start_game_with_seed(&server, 99999).await;

    // Load should restore game 1.
    let loaded = extract_json(&server.load_game().await.unwrap());
    assert_eq!(loaded["seed"].as_u64().unwrap(), 1);
    assert_eq!(loaded["x"], obs1["x"]);
    assert_eq!(loaded["y"], obs1["y"]);
}
