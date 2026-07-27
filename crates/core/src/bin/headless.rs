//! Headless game runner for automated playtesting and statistics gathering.
//!
//! Runs games without rendering, using auto-explore + auto-fight to play
//! through dungeons automatically. Outputs structured JSON stats.
//!
//! # Usage
//!
//! ```sh
//! # Run 100 games with random seeds, 80x40 map:
//! cargo run --bin headless -- --games 100 --width 80 -H 40
//!
//! # Run with a specific seed:
//! cargo run --bin headless -- --seed 42 --games 1
//!
//! # Run with a map preset:
//! cargo run --bin headless -- --preset arena --games 50
//!
//! # Replay a recorded game:
//! cargo run --bin headless -- --replay replay.json
//!
//! # Save a replay of each game:
//! cargo run --bin headless -- --games 10 --save-replays
//!
//! # Collect per-game analytics:
//! cargo run --bin headless -- --games 10 --analytics
//!
//! # Run a parameter sweep:
//! cargo run --bin headless -- --sweep sweep.json
//!
//! # Save a golden replay:
//! cargo run --bin headless -- --save-golden golden.json --seed 42 --games 1
//!
//! # Regenerate all golden replays in a directory:
//! cargo run --bin headless -- --regenerate-goldens tests/golden_replays/
//! ```

use std::collections::HashSet;

use roguelike_core::analytics::{
    self, ConfigOverrides, DamageFlow, GameAnalytics, SweepConfig, SweepPoint,
};
use roguelike_core::command::GameCommand;
use roguelike_core::data::{self, GameData};
use roguelike_core::dev_tools::{
    BatchRunStats, DevSession, GoldenReplay, Replay, ReplayResult, after_step, golden_from_session,
};
use roguelike_core::game::GameState;
use roguelike_core::map::MapPreset;
use roguelike_core::types::{Coord, Pos, Stat};

/// Common configuration for batch and single-game runs.
struct RunConfig {
    games: Stat,
    width: Coord,
    height: Coord,
    seed: Option<u64>,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replays: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut games: Stat = 10;
    let mut width: Coord = 80;
    let mut height: Coord = 40;
    let mut seed: Option<u64> = None;
    let mut preset: Option<MapPreset> = None;
    let mut max_turns: Stat = 500;
    let mut replay_path: Option<String> = None;
    let mut save_replays = false;
    let mut analytics_enabled = false;
    let mut sweep_path: Option<String> = None;
    let mut save_golden_path: Option<String> = None;
    let mut regenerate_goldens_dir: Option<String> = None;
    let mut analysis_enabled = false;
    let mut report_path: Option<String> = None;
    let mut validate_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--validate" => {
                validate_mode = true;
            }
            "--games" | "-n" => {
                i += 1;
                games = args[i].parse().expect("invalid --games value");
            }
            "--width" | "-w" => {
                i += 1;
                width = args[i].parse().expect("invalid --width value");
            }
            "--height" | "-H" => {
                i += 1;
                height = args[i].parse().expect("invalid --height value");
            }
            "--seed" | "-s" => {
                i += 1;
                seed = Some(args[i].parse().expect("invalid --seed value"));
            }
            "--preset" | "-p" => {
                i += 1;
                preset = Some(parse_preset(&args[i]));
            }
            "--max-turns" | "-t" => {
                i += 1;
                max_turns = args[i].parse().expect("invalid --max-turns value");
            }
            "--replay" | "-r" => {
                i += 1;
                replay_path = Some(args[i].clone());
            }
            "--save-replays" => {
                save_replays = true;
            }
            "--analytics" => {
                analytics_enabled = true;
            }
            "--sweep" => {
                i += 1;
                sweep_path = Some(args[i].clone());
            }
            "--save-golden" => {
                i += 1;
                save_golden_path = Some(args[i].clone());
            }
            "--regenerate-goldens" => {
                i += 1;
                regenerate_goldens_dir = Some(args[i].clone());
            }
            "--analysis" => {
                analysis_enabled = true;
            }
            "--report" => {
                i += 1;
                report_path = Some(args[i].clone());
            }
            "--help" => {
                print_help();
                return;
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Mode 0: Validate game data and exit.
    if validate_mode {
        let game_data = data::load_game_data();
        let warnings = data::validate_game_data(&game_data);
        if warnings.is_empty() {
            eprintln!("No warnings.");
            std::process::exit(0);
        } else {
            for w in &warnings {
                eprintln!("Warning: {}", w);
            }
            eprintln!("{} warning(s) found.", warnings.len());
            std::process::exit(1);
        }
    }

    let game_data = data::load_game_data();

    // Mode 1: Replay a recorded game.
    if let Some(path) = replay_path {
        run_replay(&path);
        return;
    }

    // Mode 2: Regenerate golden replays.
    if let Some(dir) = regenerate_goldens_dir {
        regenerate_goldens(&dir, max_turns);
        return;
    }

    // Mode 3: Parameter sweep.
    if let Some(path) = sweep_path {
        run_sweep(
            &path,
            analytics_enabled,
            analysis_enabled,
            report_path.as_deref(),
            &game_data,
        );
        return;
    }

    // Mode 4: Save golden replay (single game).
    if let Some(golden_path) = save_golden_path {
        let game_seed = seed.unwrap_or_else(rand::random::<u64>);
        run_and_save_golden(
            width,
            height,
            game_seed,
            preset,
            max_turns,
            &golden_path,
            &game_data,
        );
        return;
    }

    let config = RunConfig {
        games,
        width,
        height,
        seed,
        preset,
        max_turns,
        save_replays,
    };

    // Mode 5: Batch run (with optional analytics).
    if analytics_enabled {
        run_batch_with_analytics(
            &config,
            analysis_enabled,
            report_path.as_deref(),
            &game_data,
        );
    } else {
        run_batch(&config, &game_data);
    }
}

/// Original batch run — no analytics overhead.
fn run_batch(config: &RunConfig, game_data: &GameData) {
    let mut stats = BatchRunStats {
        games_played: 0,
        games_won: 0,
        games_lost: 0,
        total_turns: 0,
        total_kills: 0,
        avg_turns_per_game: 0.0,
        avg_kills_per_game: 0.0,
        seeds_used: Vec::new(),
    };

    for game_num in 0..config.games {
        let game_seed = config.seed.unwrap_or_else(rand::random::<u64>) + game_num as u64;
        stats.seeds_used.push(game_seed);

        let (result, _gs) = run_single_game(
            config.width,
            config.height,
            game_seed,
            config.preset,
            config.max_turns,
            config.save_replays,
            &ConfigOverrides::default(),
            game_data,
        );

        stats.games_played += 1;
        stats.total_turns += result.turns_played;
        stats.total_kills += result.kills;
        if result.game_over {
            stats.games_lost += 1;
        } else {
            stats.games_won += 1;
        }

        eprint!(
            "\rGame {}/{}: seed={} turns={} kills={} {}",
            game_num + 1,
            config.games,
            game_seed,
            result.turns_played,
            result.kills,
            if result.game_over { "DIED" } else { "SURVIVED" }
        );
    }
    eprintln!();

    if stats.games_played > 0 {
        stats.avg_turns_per_game = stats.total_turns as f64 / stats.games_played as f64;
        stats.avg_kills_per_game = stats.total_kills as f64 / stats.games_played as f64;
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&stats).expect("failed to serialize stats")
    );
}

/// Batch run with per-game analytics collection.
fn run_batch_with_analytics(
    config: &RunConfig,
    analysis_enabled: bool,
    report_path: Option<&str>,
    game_data: &GameData,
) {
    let mut all_analytics: Vec<GameAnalytics> = Vec::new();

    for game_num in 0..config.games {
        let game_seed = config.seed.unwrap_or_else(rand::random::<u64>) + game_num as u64;

        let game_analytics = run_single_game_tracked(
            config.width,
            config.height,
            game_seed,
            config.preset,
            config.max_turns,
            config.save_replays,
            &ConfigOverrides::default(),
            game_data,
        );

        eprint!(
            "\rGame {}/{}: seed={} turns={} kills={} {}",
            game_num + 1,
            config.games,
            game_seed,
            game_analytics.turns,
            game_analytics.kills_by_type.values().sum::<Stat>(),
            if game_analytics.game_over {
                "DIED"
            } else {
                "SURVIVED"
            }
        );

        all_analytics.push(game_analytics);
    }
    eprintln!();

    let batch_stats = analytics::aggregate(&all_analytics);
    println!(
        "{}",
        serde_json::to_string_pretty(&batch_stats).expect("failed to serialize stats")
    );

    let analysis_data = if analysis_enabled {
        let preset_name = config
            .preset
            .map(|p| format!("{:?}", p))
            .unwrap_or_else(|| "default".to_string());
        let difficulty = analytics::preset_difficulty(&preset_name, &all_analytics);
        let correlations = analytics::monster_correlations(&all_analytics);
        let flow = analytics::damage_flow(&all_analytics);

        eprintln!("\n--- Analysis ---");
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&difficulty).expect("failed to serialize difficulty")
        );
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&correlations).expect("failed to serialize correlations")
        );
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&flow).expect("failed to serialize flow")
        );

        Some((correlations, flow))
    } else {
        None
    };

    if let Some(path) = report_path {
        let (correlations, flow) =
            analysis_data.unwrap_or_else(|| (Vec::new(), analytics::damage_flow(&[])));
        generate_html_report(path, &batch_stats, &correlations, &flow, None);
    }
}

/// Run a single game with analytics tracking (snapshot/diff each step).
#[allow(clippy::too_many_arguments)]
fn run_single_game_tracked(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replay: bool,
    overrides: &ConfigOverrides,
    game_data: &GameData,
) -> GameAnalytics {
    let mut gs = match preset {
        Some(p) => GameState::with_preset_data(width, height, seed, p, game_data),
        None => GameState::with_data(width, height, seed, game_data),
    };

    analytics::apply_overrides(&mut gs, overrides);

    let mut session = DevSession {
        recording: save_replay,
        ..DevSession::default()
    };

    let mut game_analytics = analytics::new_analytics(seed);
    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && !gs.game_won && gs.turn_count < max_turns {
        let before = analytics::snapshot_entities(&gs);

        let cmd = if gs.has_adjacent_monster() {
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
        } else {
            path.clear();
            path_idx = 0;
            if let Some(p) = next_explore_path(&gs) {
                path = p;
            }
            if !path.is_empty() {
                let (nx, ny) = path[0];
                path_idx = 1;
                GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
            } else {
                GameCommand::Wait
            }
        };

        let result = gs.step(cmd);
        after_step(&mut gs, &mut session, cmd);

        if result.action_taken {
            analytics::diff_combat(&before, &gs, gs.turn_count, &mut game_analytics);
        }
    }

    if save_replay {
        let replay = Replay::from_session(&gs, &session, preset);
        let filename = format!("replay_{}.json", seed);
        if let Ok(json) = serde_json::to_string_pretty(&replay) {
            let _ = std::fs::write(&filename, json);
            eprintln!("  Saved replay to {}", filename);
        }
    }

    analytics::finalize_analytics(&mut game_analytics, &gs);
    game_analytics
}

#[allow(clippy::too_many_arguments)]
fn run_single_game(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replay: bool,
    overrides: &ConfigOverrides,
    game_data: &GameData,
) -> (ReplayResult, GameState) {
    let mut gs = match preset {
        Some(p) => GameState::with_preset_data(width, height, seed, p, game_data),
        None => GameState::with_data(width, height, seed, game_data),
    };

    analytics::apply_overrides(&mut gs, overrides);

    let mut session = DevSession {
        recording: save_replay,
        ..DevSession::default()
    };

    // Current exploration path (recomputed when exhausted or interrupted).
    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && !gs.game_won && gs.turn_count < max_turns {
        let cmd = if gs.has_adjacent_monster() {
            // Attack weakest adjacent monster.
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            // Follow current exploration path.
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
        } else {
            // Compute new path to nearest frontier.
            path.clear();
            path_idx = 0;
            if let Some(p) = next_explore_path(&gs) {
                path = p;
            }
            if !path.is_empty() {
                let (nx, ny) = path[0];
                path_idx = 1;
                GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
            } else {
                GameCommand::Wait
            }
        };

        gs.step(cmd);
        after_step(&mut gs, &mut session, cmd);
    }

    if save_replay {
        let replay = Replay::from_session(&gs, &session, preset);
        let filename = format!("replay_{}.json", seed);
        if let Ok(json) = serde_json::to_string_pretty(&replay) {
            let _ = std::fs::write(&filename, json);
            eprintln!("  Saved replay to {}", filename);
        }
    }

    let kills = gs.entities.iter().skip(1).filter(|e| !e.alive).count() as Stat;
    let result = ReplayResult {
        turns_played: gs.turn_count,
        game_over: gs.game_over,
        final_hp: gs.entities[0].hp,
        final_turn: gs.turn_count,
        kills,
    };
    (result, gs)
}

/// Run a single game and save it as a golden replay.
///
/// For standard tier seeds: uses A* pathfinding + direct GameState access.
/// For micro/compact tier seeds: uses auto_explore() + auto_fight() via
/// the GameStep adapter, recording individual step commands.
fn run_and_save_golden(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    output_path: &str,
    game_data: &GameData,
) {
    use roguelike_core::game_step::{self, CompactGameStateAdapter, MicroGameStateAdapter};
    use roguelike_core::seed_code;

    let tier = seed_code::tier_from_seed(seed);

    match tier {
        seed_code::Tier::Standard => {
            // Standard tier: use direct GameState access with A* pathfinding.
            run_and_save_golden_standard(
                width,
                height,
                seed,
                preset,
                max_turns,
                output_path,
                game_data,
            );
        }
        _ => {
            // Micro/Compact tier: drive game one step at a time using
            // pathfinding to decide direction each turn.
            let mut game = game_step::create_game(seed, width, height, preset, game_data)
                .expect("failed to create game for golden replay");

            let mut commands: Vec<GameCommand> = Vec::new();

            while !game.is_terminal() && (game.turn_count() as u32) < max_turns as u32 {
                let cmd = if let Some(adapter) =
                    game.as_any_mut().downcast_mut::<CompactGameStateAdapter>()
                {
                    pick_compact_command(adapter)
                } else if let Some(adapter) =
                    game.as_any_mut().downcast_mut::<MicroGameStateAdapter>()
                {
                    pick_micro_command(adapter)
                } else {
                    GameCommand::Wait
                };
                let _result = game.step(cmd);
                commands.push(cmd);
            }

            // Build golden replay: re-execute to get expected result.
            let replay = Replay {
                seed,
                width,
                height,
                commands,
                preset,
            };
            let expected = replay.execute();

            let preset_name = preset
                .map(|p| format!("{:?}", p))
                .unwrap_or_else(|| "default".to_string());
            let name = format!("seed_{}_{}", seed, preset_name.to_lowercase());
            let description = format!(
                "Seed {}, {}x{}, preset={}, max_turns={}",
                seed, width, height, preset_name, max_turns
            );

            let golden = GoldenReplay {
                name,
                description,
                replay,
                expected,
                dev_modified: false,
            };

            let json = serde_json::to_string_pretty(&golden).expect("failed to serialize");
            std::fs::write(output_path, json).expect("failed to write golden replay file");
            eprintln!("Saved golden replay to {}", output_path);
            eprintln!(
                "  Result: turns={} kills={} {}",
                golden.expected.turns_played,
                golden.expected.kills,
                if golden.expected.game_over {
                    "DIED"
                } else {
                    "SURVIVED"
                }
            );
        }
    }
}

/// Pick the next command for a compact tier game using direct state access.
fn pick_compact_command(
    adapter: &mut roguelike_core::game_step::CompactGameStateAdapter,
) -> GameCommand {
    use roguelike_core::tier_compact::autorun::has_adjacent_monster;
    use roguelike_core::tier_compact::map::TILE_STAIRS_DOWN;
    use roguelike_core::tier_compact::pathfinding;
    use roguelike_core::tier_compact::types::PLAYER_IDX;

    let g = &adapter.game;
    let pi = PLAYER_IDX as usize;
    let px = g.entities.x[pi];
    let py = g.entities.y[pi];

    // On stairs → descend.
    if g.map.tile_at(px, py) == TILE_STAIRS_DOWN {
        return GameCommand::Descend;
    }

    // Adjacent monster → attack it.
    if has_adjacent_monster(&g.entities) {
        // Find the adjacent monster and move toward it.
        for i in 1..g.entities.count as usize {
            if g.entities.alive[i] {
                let dx = (g.entities.x[i] - px).abs();
                let dy = (g.entities.y[i] - py).abs();
                if dx <= 1 && dy <= 1 {
                    return GameCommand::move_or_wait(g.entities.x[i] - px, g.entities.y[i] - py);
                }
            }
        }
    }

    // Item at feet → pick up.
    if g.items.item_at(px, py) != roguelike_core::tier_compact::types::NO_ITEM {
        return GameCommand::Pickup;
    }

    // Find nearest frontier and step toward it.
    let mut buf = pathfinding::BfsBuffers::new();
    if let Some((fx, fy)) = pathfinding::find_nearest_frontier(px, py, &g.map, &g.fov, &mut buf)
        && let Some(dir) = pathfinding::find_first_step(px, py, fx, fy, &g.map, &g.fov, &mut buf)
    {
        return GameCommand::Move(dir);
    }

    // No frontier → try to navigate to stairs if explored.
    for y in 0..g.map.height {
        for x in 0..g.map.width {
            if g.fov.is_explored(x, y) && g.map.tile_at(x, y) == TILE_STAIRS_DOWN {
                let mut buf2 = pathfinding::BfsBuffers::new();
                if let Some(dir) =
                    pathfinding::find_first_step(px, py, x, y, &g.map, &g.fov, &mut buf2)
                {
                    return GameCommand::Move(dir);
                }
            }
        }
    }

    GameCommand::Wait
}

/// Pick the next command for a micro tier game using direct state access.
fn pick_micro_command(
    adapter: &mut roguelike_core::game_step::MicroGameStateAdapter,
) -> GameCommand {
    use roguelike_core::tier_micro::autorun::has_adjacent_monster;
    use roguelike_core::tier_micro::map::TILE_STAIRS_DOWN;
    use roguelike_core::tier_micro::pathfinding;
    use roguelike_core::tier_micro::types::PLAYER_IDX;

    let g = &adapter.game;
    let pi = PLAYER_IDX as usize;
    let px = g.entities.x[pi];
    let py = g.entities.y[pi];

    if g.map.tile_at(px, py) == TILE_STAIRS_DOWN {
        return GameCommand::Descend;
    }

    if has_adjacent_monster(&g.entities) {
        for i in 1..g.entities.count as usize {
            if g.entities.is_alive(i) {
                let dx = g.entities.x[i].abs_diff(px);
                let dy = g.entities.y[i].abs_diff(py);
                if dx <= 1 && dy <= 1 {
                    return GameCommand::move_or_wait(
                        g.entities.x[i] as i32 - px as i32,
                        g.entities.y[i] as i32 - py as i32,
                    );
                }
            }
        }
    }

    use roguelike_core::tier_micro::item_store::NO_ITEM;
    if g.items.item_at(px, py) != NO_ITEM {
        return GameCommand::Pickup;
    }

    let mut buf = pathfinding::BfsBuffers::new();
    if let Some((fx, fy)) = pathfinding::find_nearest_frontier(px, py, &g.map, &g.fov, &mut buf)
        && let Some(dir) = pathfinding::find_first_step(px, py, fx, fy, &g.map, &g.fov, &mut buf)
    {
        return GameCommand::Move(dir);
    }

    for y in 0..g.map.height {
        for x in 0..g.map.width {
            if g.fov.is_explored(x, y) && g.map.tile_at(x, y) == TILE_STAIRS_DOWN {
                let mut buf2 = pathfinding::BfsBuffers::new();
                if let Some(dir) =
                    pathfinding::find_first_step(px, py, x, y, &g.map, &g.fov, &mut buf2)
                {
                    return GameCommand::Move(dir);
                }
            }
        }
    }

    GameCommand::Wait
}

/// Standard tier golden generation (original logic using A* pathfinding).
fn run_and_save_golden_standard(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    output_path: &str,
    game_data: &GameData,
) {
    let mut gs = match preset {
        Some(p) => GameState::with_preset_data(width, height, seed, p, game_data),
        None => GameState::with_data(width, height, seed, game_data),
    };

    let mut session = DevSession {
        recording: true,
        ..DevSession::default()
    };

    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && !gs.game_won && gs.turn_count < max_turns {
        let cmd = if gs.has_adjacent_monster() {
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
        } else {
            path.clear();
            path_idx = 0;
            if let Some(p) = next_explore_path(&gs) {
                path = p;
            }
            if !path.is_empty() {
                let (nx, ny) = path[0];
                path_idx = 1;
                GameCommand::move_or_wait(nx - gs.entities[0].x, ny - gs.entities[0].y)
            } else {
                GameCommand::Wait
            }
        };

        gs.step(cmd);
        after_step(&mut gs, &mut session, cmd);
    }

    let preset_name = preset
        .map(|p| format!("{:?}", p))
        .unwrap_or_else(|| "default".to_string());
    let name = format!("seed_{}_{}", seed, preset_name.to_lowercase());
    let description = format!(
        "Seed {}, {}x{}, preset={}, max_turns={}",
        seed, width, height, preset_name, max_turns
    );

    let golden = golden_from_session(&name, &description, &gs, &session, preset);
    let json = serde_json::to_string_pretty(&golden).expect("failed to serialize golden replay");
    std::fs::write(output_path, json).expect("failed to write golden replay file");
    eprintln!("Saved golden replay to {}", output_path);
    eprintln!(
        "  Result: turns={} kills={} {}",
        golden.expected.turns_played,
        golden.expected.kills,
        if golden.expected.game_over {
            "DIED"
        } else {
            "SURVIVED"
        }
    );
}

/// Regenerate all golden replays in a directory.
fn regenerate_goldens(dir: &str, max_turns: Stat) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to read directory '{}': {}", dir, err);
            std::process::exit(1);
        }
    };

    let mut count = 0;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(err) => {
                    eprintln!("  Skipping {}: {}", path.display(), err);
                    continue;
                }
            };
            let mut golden: GoldenReplay = match serde_json::from_str(&json) {
                Ok(g) => g,
                Err(err) => {
                    eprintln!("  Skipping {}: {}", path.display(), err);
                    continue;
                }
            };

            // Re-execute and update expected result.
            let _ = max_turns; // max_turns is part of the replay itself
            let new_result = golden.replay.execute();
            golden.expected = new_result;

            let updated_json = serde_json::to_string_pretty(&golden).expect("failed to serialize");
            std::fs::write(&path, updated_json).expect("failed to write");
            eprintln!(
                "  Regenerated: {} (turns={}, kills={}, {})",
                path.display(),
                golden.expected.turns_played,
                golden.expected.kills,
                if golden.expected.game_over {
                    "DIED"
                } else {
                    "SURVIVED"
                }
            );
            count += 1;
        }
    }
    eprintln!("Regenerated {} golden replays.", count);
}

/// Run a parameter sweep from a JSON config file.
fn run_sweep(
    path: &str,
    analytics_enabled: bool,
    analysis_enabled: bool,
    report_path: Option<&str>,
    game_data: &GameData,
) {
    let json = std::fs::read_to_string(path).expect("failed to read sweep config");
    let config: SweepConfig = serde_json::from_str(&json).expect("failed to parse sweep config");

    let combos = analytics::sweep_combinations(&config);
    eprintln!(
        "Sweep: {} parameter combinations x {} games each",
        combos.len(),
        config.games_per_point
    );

    let mut results: Vec<SweepPoint> = Vec::new();

    for (ci, overrides) in combos.iter().enumerate() {
        let mut point_analytics: Vec<GameAnalytics> = Vec::new();

        for game_num in 0..config.games_per_point {
            let game_seed = (ci as u64 * 1000) + game_num as u64;

            if analytics_enabled {
                // Full per-turn combat tracking (snapshot + diff each step).
                let ga = run_single_game_tracked(
                    config.width,
                    config.height,
                    game_seed,
                    config.preset,
                    config.max_turns,
                    false,
                    overrides,
                    game_data,
                );
                point_analytics.push(ga);
            } else {
                // Lightweight: run game without per-turn tracking, then
                // build minimal analytics from the final game state.
                let (_result, gs) = run_single_game(
                    config.width,
                    config.height,
                    game_seed,
                    config.preset,
                    config.max_turns,
                    false,
                    overrides,
                    game_data,
                );
                point_analytics.push(analytics::from_game_state(&gs, game_seed));
            }
        }

        let stats = analytics::aggregate(&point_analytics);
        eprint!(
            "\rSweep point {}/{}: win_rate={:.1}% avg_turns={:.0}",
            ci + 1,
            combos.len(),
            stats.win_rate * 100.0,
            stats.avg_turns,
        );

        results.push(SweepPoint {
            overrides: overrides.clone(),
            stats,
        });
    }
    eprintln!();

    println!(
        "{}",
        serde_json::to_string_pretty(&results).expect("failed to serialize sweep results")
    );

    if analysis_enabled {
        // Aggregate all game analytics for cross-sweep analysis.
        let all_analytics: Vec<GameAnalytics> = results
            .iter()
            .flat_map(|_| Vec::<GameAnalytics>::new())
            .collect();
        if !all_analytics.is_empty() {
            let correlations = analytics::monster_correlations(&all_analytics);
            eprintln!(
                "\n{}",
                serde_json::to_string_pretty(&correlations)
                    .expect("failed to serialize correlations")
            );
        }
    }

    if let Some(report) = report_path {
        generate_html_report(
            report,
            &analytics::aggregate(&[]),
            &[],
            &analytics::damage_flow(&[]),
            Some(&results),
        );
    }
}

/// Pick the move command to attack the weakest adjacent monster.
fn fight_command(gs: &GameState) -> GameCommand {
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    gs.entities
        .iter()
        .enumerate()
        .filter(|(i, e)| *i != 0 && e.alive && (e.x - px).abs() <= 1 && (e.y - py).abs() <= 1)
        .min_by_key(|(_, e)| e.hp)
        .map(|(_, e)| GameCommand::move_or_wait(e.x - px, e.y - py))
        .unwrap_or(GameCommand::Wait)
}

/// Compute an A* path to the nearest exploration frontier.
fn next_explore_path(gs: &GameState) -> Option<Vec<Pos>> {
    let frontiers = gs.frontier_tiles();
    if frontiers.is_empty() {
        return None;
    }
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    let frontier_set: HashSet<Pos> = frontiers.into_iter().collect();
    let (tx, ty) =
        roguelike_core::pathfinding::nearest_by_cost(&gs.map, px, py, &frontier_set, &gs.explored)?;
    roguelike_core::pathfinding::find_path(&gs.map, px, py, tx, ty, &gs.explored)
}

fn run_replay(path: &str) {
    let json = std::fs::read_to_string(path).expect("failed to read replay file");
    let replay: Replay = serde_json::from_str(&json).expect("failed to parse replay");
    eprintln!(
        "Replaying: seed={}, {}x{}, {} commands",
        replay.seed,
        replay.width,
        replay.height,
        replay.commands.len()
    );
    let result = replay.execute();
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("failed to serialize result")
    );
}

fn parse_preset(s: &str) -> MapPreset {
    match s.to_lowercase().as_str() {
        "arena" => MapPreset::Arena,
        "corridor" => MapPreset::Corridor,
        "labyrinth" => MapPreset::Labyrinth,
        "single_room" | "single-room" | "singleroom" => MapPreset::SingleRoom,
        "open_field" | "open-field" | "openfield" => MapPreset::OpenField,
        _ => {
            eprintln!(
                "Unknown preset: '{}'. Options: arena, corridor, labyrinth, single_room, open_field",
                s
            );
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// HTML Report Generation
// ---------------------------------------------------------------------------

struct Insight {
    category: &'static str,
    title: String,
    detail: String,
}

fn generate_insights(
    stats: &analytics::EnhancedBatchStats,
    correlations: &[analytics::MonsterCorrelation],
    flow: &DamageFlow,
    sweep: Option<&[SweepPoint]>,
) -> Vec<Insight> {
    let mut insights = Vec::new();

    // Balance assessment.
    let assessment = if stats.win_rate >= 0.7 {
        "easy"
    } else if stats.win_rate >= 0.4 {
        "balanced"
    } else {
        "hard"
    };
    if stats.games > 0 {
        insights.push(Insight {
            category: "balance",
            title: "Balance Assessment".to_string(),
            detail: format!(
                "Difficulty is {} -- win rate {:.0}% ({} games)",
                assessment,
                stats.win_rate * 100.0,
                stats.games,
            ),
        });
    }

    // Most dangerous monster.
    if let Some(worst) = correlations.iter().max_by(|a, b| {
        a.death_rate_when_encountered
            .partial_cmp(&b.death_rate_when_encountered)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        insights.push(Insight {
            category: "danger",
            title: "Most Dangerous Monster".to_string(),
            detail: format!(
                "{} -- {:.0}% death rate, avg {:.1} damage",
                worst.monster_type,
                worst.death_rate_when_encountered * 100.0,
                worst.avg_damage_dealt,
            ),
        });
    }

    // Most killed monster.
    if let Some((name, count)) = stats
        .kills_by_type
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
    {
        insights.push(Insight {
            category: "efficiency",
            title: "Most Killed Monster".to_string(),
            detail: format!("{} -- avg {:.1} kills/game", name, count),
        });
    }

    // Damage efficiency.
    let total_dealt: f64 = stats.damage_dealt_by_type.values().sum();
    let total_taken: f64 = stats.damage_taken_by_type.values().sum();
    if total_taken > 0.0 {
        let ratio = total_dealt / total_taken;
        insights.push(Insight {
            category: "efficiency",
            title: "Damage Efficiency".to_string(),
            detail: format!("Player deals {:.1}x more damage than received", ratio),
        });
    }

    // Actionable suggestion from correlations.
    if let Some(worst) = correlations
        .iter()
        .filter(|c| c.death_rate_when_encountered > 0.7)
        .max_by(|a, b| {
            a.death_rate_when_encountered
                .partial_cmp(&b.death_rate_when_encountered)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        insights.push(Insight {
            category: "danger",
            title: "Actionable Suggestion".to_string(),
            detail: format!(
                "Consider reducing {} HP -- {:.0}% death rate when encountered",
                worst.monster_type,
                worst.death_rate_when_encountered * 100.0,
            ),
        });
    }

    // Sweep threshold.
    if let Some(sweep_points) = sweep {
        for pt in sweep_points {
            let overrides = &pt.overrides;
            let wr = pt.stats.win_rate;
            let params: Vec<String> = [
                overrides.player_hp.map(|v| format!("player_hp={}", v)),
                overrides
                    .player_attack
                    .map(|v| format!("player_attack={}", v)),
                overrides
                    .player_defense
                    .map(|v| format!("player_defense={}", v)),
            ]
            .into_iter()
            .flatten()
            .collect();
            if (0.48..=0.52).contains(&wr) && !params.is_empty() {
                insights.push(Insight {
                    category: "threshold",
                    title: "Survivability Threshold".to_string(),
                    detail: format!(
                        "Win rate crosses 50% at {} (actual: {:.0}%)",
                        params.join(", "),
                        wr * 100.0,
                    ),
                });
                break;
            }
        }
    }

    // Damage flow insight.
    if let Some(top) = flow.flows.first() {
        insights.push(Insight {
            category: "efficiency",
            title: "Highest Damage Flow".to_string(),
            detail: format!(
                "{} -> {} ({} total damage)",
                top.attacker, top.defender, top.total_damage,
            ),
        });
    }

    insights
}

fn generate_html_report(
    path: &str,
    stats: &analytics::EnhancedBatchStats,
    correlations: &[analytics::MonsterCorrelation],
    flow: &DamageFlow,
    sweep: Option<&[SweepPoint]>,
) {
    let insights = generate_insights(stats, correlations, flow, sweep);

    let stats_json = serde_json::to_string(stats).expect("failed to serialize stats");
    let correlations_json =
        serde_json::to_string(correlations).expect("failed to serialize correlations");
    let flow_json = serde_json::to_string(flow).expect("failed to serialize flow");
    let sweep_json = sweep
        .map(|s| serde_json::to_string(s).expect("failed to serialize sweep"))
        .unwrap_or_else(|| "null".to_string());

    let mut insights_html = String::new();
    for insight in &insights {
        let icon = match insight.category {
            "danger" => "&#9760;",     // skull
            "balance" => "&#9878;",    // scales
            "efficiency" => "&#9889;", // lightning
            "threshold" => "&#9733;",  // star
            _ => "&#8226;",            // bullet
        };
        insights_html.push_str(&format!(
            r#"<div class="insight-card"><span class="insight-icon">{}</span><div><strong>{}</strong><br><span class="insight-detail">{}</span></div></div>"#,
            icon, insight.title, insight.detail,
        ));
    }

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Roguelike Analytics Report</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            background: #1a1a2e;
            color: #e0e0e0;
            font-family: "Courier New", "Consolas", monospace;
            padding: 20px;
            max-width: 1200px;
            margin: 0 auto;
        }}
        h1 {{
            text-align: center;
            color: #e94560;
            margin-bottom: 8px;
            font-size: 28px;
        }}
        .subtitle {{
            text-align: center;
            color: #888;
            margin-bottom: 24px;
            font-size: 12px;
        }}
        .summary-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
            gap: 12px;
            margin-bottom: 24px;
        }}
        .summary-card {{
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 16px;
            text-align: center;
        }}
        .summary-card .value {{
            font-size: 32px;
            font-weight: bold;
            color: #53d8fb;
        }}
        .summary-card .label {{
            font-size: 12px;
            color: #888;
            margin-top: 4px;
        }}
        .insights-panel {{
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 16px;
            margin-bottom: 24px;
        }}
        .insights-panel h2 {{
            color: #f5a623;
            margin-bottom: 12px;
            font-size: 18px;
        }}
        .insight-card {{
            display: flex;
            align-items: flex-start;
            gap: 10px;
            padding: 8px 0;
            border-bottom: 1px solid #0f3460;
        }}
        .insight-card:last-child {{ border-bottom: none; }}
        .insight-icon {{ font-size: 20px; min-width: 28px; text-align: center; }}
        .insight-detail {{ color: #aaa; font-size: 13px; }}
        .chart-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(500px, 1fr));
            gap: 16px;
            margin-bottom: 24px;
        }}
        .chart-container {{
            background: #16213e;
            border: 1px solid #0f3460;
            border-radius: 8px;
            padding: 16px;
        }}
        .chart-container h3 {{
            color: #53d8fb;
            margin-bottom: 12px;
            font-size: 14px;
        }}
        canvas {{ max-width: 100%; }}
        footer {{
            text-align: center;
            color: #555;
            font-size: 11px;
            margin-top: 24px;
        }}
    </style>
</head>
<body>
    <h1>Roguelike Analytics Report</h1>
    <div class="subtitle">Generated by headless runner</div>

    <div class="summary-grid">
        <div class="summary-card">
            <div class="value" id="win-rate">--</div>
            <div class="label">Win Rate</div>
        </div>
        <div class="summary-card">
            <div class="value" id="avg-turns">--</div>
            <div class="label">Avg Turns</div>
        </div>
        <div class="summary-card">
            <div class="value" id="avg-kills">--</div>
            <div class="label">Avg Kills</div>
        </div>
        <div class="summary-card">
            <div class="value" id="avg-explored">--</div>
            <div class="label">Avg Explored</div>
        </div>
    </div>

    <div class="insights-panel">
        <h2>Insights</h2>
        {insights_html}
    </div>

    <div class="chart-grid">
        <div class="chart-container">
            <h3>Kills by Monster Type</h3>
            <canvas id="killsByType"></canvas>
        </div>
        <div class="chart-container">
            <h3>Damage Dealt vs Taken</h3>
            <canvas id="damageComparison"></canvas>
        </div>
        <div class="chart-container" id="dangerContainer" style="display:none">
            <h3>Monster Danger Ranking</h3>
            <canvas id="monsterDanger"></canvas>
        </div>
        <div class="chart-container" id="flowContainer" style="display:none">
            <h3>Damage Flow (Attacker -> Defender)</h3>
            <canvas id="damageFlow"></canvas>
        </div>
        <div class="chart-container" id="sweepWinRateContainer" style="display:none">
            <h3>Win Rate vs Parameter</h3>
            <canvas id="sweepWinRate"></canvas>
        </div>
        <div class="chart-container" id="sweepTurnsContainer" style="display:none">
            <h3>Avg Turns vs Parameter</h3>
            <canvas id="sweepTurns"></canvas>
        </div>
    </div>

    <footer>Roguelike Analytics &mdash; headless runner report</footer>

    <script>
        const STATS = {stats_json};
        const CORRELATIONS = {correlations_json};
        const FLOW = {flow_json};
        const SWEEP = {sweep_json};

        const COLORS = ['#e94560', '#0f3460', '#53d8fb', '#f5a623', '#a29bfe', '#6c5ce7'];
        const chartDefaults = {{
            responsive: true,
            plugins: {{
                legend: {{ labels: {{ color: '#e0e0e0', font: {{ family: 'monospace' }} }} }},
            }},
            scales: {{
                x: {{ ticks: {{ color: '#e0e0e0' }}, grid: {{ color: '#0f3460' }} }},
                y: {{ ticks: {{ color: '#e0e0e0' }}, grid: {{ color: '#0f3460' }} }},
            }},
        }};

        // Summary cards
        document.getElementById('win-rate').textContent =
            (STATS.win_rate * 100).toFixed(0) + '%';
        document.getElementById('avg-turns').textContent =
            STATS.avg_turns.toFixed(0);
        document.getElementById('avg-kills').textContent =
            STATS.avg_kills.toFixed(1);
        document.getElementById('avg-explored').textContent =
            STATS.avg_explored_pct.toFixed(0) + '%';

        // Kills by type
        if (STATS.kills_by_type && Object.keys(STATS.kills_by_type).length > 0) {{
            const labels = Object.keys(STATS.kills_by_type).sort();
            new Chart(document.getElementById('killsByType'), {{
                type: 'bar',
                data: {{
                    labels: labels,
                    datasets: [{{
                        label: 'Avg Kills/Game',
                        data: labels.map(l => STATS.kills_by_type[l]),
                        backgroundColor: labels.map((_, i) => COLORS[i % COLORS.length]),
                    }}],
                }},
                options: chartDefaults,
            }});
        }}

        // Damage comparison
        const allTypes = [...new Set([
            ...Object.keys(STATS.damage_dealt_by_type || {{}}),
            ...Object.keys(STATS.damage_taken_by_type || {{}}),
        ])].sort();
        if (allTypes.length > 0) {{
            new Chart(document.getElementById('damageComparison'), {{
                type: 'bar',
                data: {{
                    labels: allTypes,
                    datasets: [
                        {{
                            label: 'Dealt to',
                            data: allTypes.map(t => (STATS.damage_dealt_by_type || {{}})[t] || 0),
                            backgroundColor: '#e94560',
                        }},
                        {{
                            label: 'Taken from',
                            data: allTypes.map(t => (STATS.damage_taken_by_type || {{}})[t] || 0),
                            backgroundColor: '#53d8fb',
                        }},
                    ],
                }},
                options: chartDefaults,
            }});
        }}

        // Monster danger scatter
        if (CORRELATIONS && CORRELATIONS.length > 0) {{
            document.getElementById('dangerContainer').style.display = '';
            new Chart(document.getElementById('monsterDanger'), {{
                type: 'scatter',
                data: {{
                    datasets: CORRELATIONS.map((m, i) => ({{
                        label: m.monster_type,
                        data: [{{ x: m.avg_damage_dealt, y: m.death_rate_when_encountered * 100 }}],
                        backgroundColor: COLORS[i % COLORS.length],
                        pointRadius: 8,
                    }})),
                }},
                options: {{
                    ...chartDefaults,
                    scales: {{
                        x: {{ ...chartDefaults.scales.x, title: {{ display: true, text: 'Avg Damage', color: '#e0e0e0' }} }},
                        y: {{ ...chartDefaults.scales.y, title: {{ display: true, text: 'Death Rate %', color: '#e0e0e0' }} }},
                    }},
                }},
            }});
        }}

        // Damage flow heatmap (as bar chart since Chart.js doesn't have native heatmap)
        if (FLOW && FLOW.flows && FLOW.flows.length > 0) {{
            document.getElementById('flowContainer').style.display = '';
            const flowLabels = FLOW.flows.map(f => f.attacker + ' -> ' + f.defender);
            new Chart(document.getElementById('damageFlow'), {{
                type: 'bar',
                data: {{
                    labels: flowLabels,
                    datasets: [{{
                        label: 'Total Damage',
                        data: FLOW.flows.map(f => f.total_damage),
                        backgroundColor: FLOW.flows.map((_, i) => COLORS[i % COLORS.length]),
                    }}],
                }},
                options: {{
                    ...chartDefaults,
                    indexAxis: 'y',
                }},
            }});
        }}

        // Sweep charts
        if (SWEEP && SWEEP.length > 0) {{
            // Group by parameter
            const axes = {{}};
            SWEEP.forEach(pt => {{
                const ov = pt.overrides || {{}};
                for (const [param, value] of Object.entries(ov)) {{
                    if (value !== null) {{
                        if (!axes[param]) axes[param] = [];
                        axes[param].push({{ value, stats: pt.stats }});
                    }}
                }}
            }});

            if (Object.keys(axes).length > 0) {{
                document.getElementById('sweepWinRateContainer').style.display = '';
                document.getElementById('sweepTurnsContainer').style.display = '';

                const wrDatasets = [];
                const turnsDatasets = [];
                let colorIdx = 0;
                for (const [param, entries] of Object.entries(axes)) {{
                    entries.sort((a, b) => a.value - b.value);
                    const c = COLORS[colorIdx++ % COLORS.length];
                    wrDatasets.push({{
                        label: param,
                        data: entries.map(e => ({{ x: e.value, y: e.stats.win_rate * 100 }})),
                        borderColor: c,
                        backgroundColor: c,
                        fill: false,
                    }});
                    turnsDatasets.push({{
                        label: param,
                        data: entries.map(e => ({{ x: e.value, y: e.stats.avg_turns }})),
                        borderColor: c,
                        backgroundColor: c,
                        fill: false,
                    }});
                }}

                new Chart(document.getElementById('sweepWinRate'), {{
                    type: 'line',
                    data: {{ datasets: wrDatasets }},
                    options: {{
                        ...chartDefaults,
                        scales: {{
                            x: {{ ...chartDefaults.scales.x, type: 'linear', title: {{ display: true, text: 'Parameter Value', color: '#e0e0e0' }} }},
                            y: {{ ...chartDefaults.scales.y, title: {{ display: true, text: 'Win Rate %', color: '#e0e0e0' }} }},
                        }},
                    }},
                }});

                new Chart(document.getElementById('sweepTurns'), {{
                    type: 'line',
                    data: {{ datasets: turnsDatasets }},
                    options: {{
                        ...chartDefaults,
                        scales: {{
                            x: {{ ...chartDefaults.scales.x, type: 'linear', title: {{ display: true, text: 'Parameter Value', color: '#e0e0e0' }} }},
                            y: {{ ...chartDefaults.scales.y, title: {{ display: true, text: 'Avg Turns', color: '#e0e0e0' }} }},
                        }},
                    }},
                }});
            }}
        }}
    </script>
</body>
</html>"##,
        insights_html = insights_html,
        stats_json = stats_json,
        correlations_json = correlations_json,
        flow_json = flow_json,
        sweep_json = sweep_json,
    );

    std::fs::write(path, html).expect("failed to write HTML report");
    eprintln!("Report written to {}", path);
}

fn print_help() {
    eprintln!(
        "headless - automated roguelike playtester

USAGE:
    headless [OPTIONS]

OPTIONS:
    -n, --games N              Number of games to run (default: 10)
    -w, --width N              Map width (default: 80)
    -H, --height N             Map height (default: 40)
    -s, --seed N               Starting seed (increments per game)
    -p, --preset NAME          Map preset: arena, corridor, labyrinth, single_room, open_field
    -t, --max-turns N          Max turns per game (default: 500)
    -r, --replay FILE          Replay a recorded game from JSON file
        --save-replays         Save replay JSON for each game
        --analytics            Collect per-game analytics (snapshot/diff each step)
        --sweep FILE           Run parameter sweep from JSON config
        --save-golden FILE     Save run as golden replay JSON
        --regenerate-goldens DIR  Re-execute all goldens in dir, update expected outcomes
        --analysis             With --analytics, compute correlations/difficulty metrics
        --report FILE          Generate self-contained HTML report with charts
        --validate             Validate game.toml and exit (0=ok, 1=warnings)
        --help                 Show this help message"
    );
}
