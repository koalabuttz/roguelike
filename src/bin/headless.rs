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

use roguelike::analytics::{
    self, ConfigOverrides, GameAnalytics, SweepConfig, SweepPoint,
};
use roguelike::dev_tools::{
    BatchRunStats, DevSession, GoldenReplay, Replay, ReplayResult, after_step, golden_from_session,
};
use roguelike::game::GameState;
use roguelike::input::GameCommand;
use roguelike::map::MapPreset;
use roguelike::types::{Coord, Pos, Stat};

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

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
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
        run_sweep(&path, analytics_enabled, analysis_enabled);
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
        );
        return;
    }

    // Mode 5: Batch run (with optional analytics).
    if analytics_enabled {
        run_batch_with_analytics(
            games,
            width,
            height,
            seed,
            preset,
            max_turns,
            save_replays,
            analysis_enabled,
        );
    } else {
        run_batch(games, width, height, seed, preset, max_turns, save_replays);
    }
}

/// Original batch run — no analytics overhead.
fn run_batch(
    games: Stat,
    width: Coord,
    height: Coord,
    seed: Option<u64>,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replays: bool,
) {
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

    for game_num in 0..games {
        let game_seed = seed.unwrap_or_else(rand::random::<u64>) + game_num as u64;
        stats.seeds_used.push(game_seed);

        let result = run_single_game(width, height, game_seed, preset, max_turns, save_replays);

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
            games,
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
    games: Stat,
    width: Coord,
    height: Coord,
    seed: Option<u64>,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replays: bool,
    analysis_enabled: bool,
) {
    let mut all_analytics: Vec<GameAnalytics> = Vec::new();

    for game_num in 0..games {
        let game_seed = seed.unwrap_or_else(rand::random::<u64>) + game_num as u64;

        let game_analytics = run_single_game_tracked(
            width,
            height,
            game_seed,
            preset,
            max_turns,
            save_replays,
            &ConfigOverrides::default(),
        );

        eprint!(
            "\rGame {}/{}: seed={} turns={} kills={} {}",
            game_num + 1,
            games,
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

    if analysis_enabled {
        let preset_name = preset.map(|p| format!("{:?}", p)).unwrap_or_else(|| "default".to_string());
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
            serde_json::to_string_pretty(&correlations)
                .expect("failed to serialize correlations")
        );
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&flow).expect("failed to serialize flow")
        );
    }
}

/// Run a single game with analytics tracking (snapshot/diff each step).
fn run_single_game_tracked(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replay: bool,
    overrides: &ConfigOverrides,
) -> GameAnalytics {
    let mut gs = match preset {
        Some(p) => GameState::with_preset(width, height, seed, p),
        None => GameState::with_seed(width, height, seed),
    };

    analytics::apply_overrides(&mut gs, overrides);

    let mut session = DevSession {
        recording: save_replay,
        ..DevSession::default()
    };

    let mut game_analytics = analytics::new_analytics(seed);
    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && gs.turn_count < max_turns {
        let before = analytics::snapshot_entities(&gs);

        let cmd = if gs.has_adjacent_monster() {
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::Move {
                dx: nx - gs.entities[0].x,
                dy: ny - gs.entities[0].y,
            }
        } else {
            path.clear();
            path_idx = 0;
            if let Some(p) = next_explore_path(&gs) {
                path = p;
            }
            if !path.is_empty() {
                let (nx, ny) = path[0];
                path_idx = 1;
                GameCommand::Move {
                    dx: nx - gs.entities[0].x,
                    dy: ny - gs.entities[0].y,
                }
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

fn run_single_game(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    save_replay: bool,
) -> ReplayResult {
    let mut gs = match preset {
        Some(p) => GameState::with_preset(width, height, seed, p),
        None => GameState::with_seed(width, height, seed),
    };

    let mut session = DevSession {
        recording: save_replay,
        ..DevSession::default()
    };

    // Current exploration path (recomputed when exhausted or interrupted).
    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && gs.turn_count < max_turns {
        let cmd = if gs.has_adjacent_monster() {
            // Attack weakest adjacent monster.
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            // Follow current exploration path.
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::Move {
                dx: nx - gs.entities[0].x,
                dy: ny - gs.entities[0].y,
            }
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
                GameCommand::Move {
                    dx: nx - gs.entities[0].x,
                    dy: ny - gs.entities[0].y,
                }
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
    ReplayResult {
        turns_played: gs.turn_count,
        game_over: gs.game_over,
        final_hp: gs.entities[0].hp,
        final_turn: gs.turn_count,
        kills,
    }
}

/// Run a single game and save it as a golden replay.
fn run_and_save_golden(
    width: Coord,
    height: Coord,
    seed: u64,
    preset: Option<MapPreset>,
    max_turns: Stat,
    output_path: &str,
) {
    let mut gs = match preset {
        Some(p) => GameState::with_preset(width, height, seed, p),
        None => GameState::with_seed(width, height, seed),
    };

    let mut session = DevSession {
        recording: true,
        ..DevSession::default()
    };

    let mut path: Vec<Pos> = Vec::new();
    let mut path_idx: usize = 0;

    while !gs.game_over && gs.turn_count < max_turns {
        let cmd = if gs.has_adjacent_monster() {
            path.clear();
            fight_command(&gs)
        } else if path_idx < path.len() {
            let (nx, ny) = path[path_idx];
            path_idx += 1;
            GameCommand::Move {
                dx: nx - gs.entities[0].x,
                dy: ny - gs.entities[0].y,
            }
        } else {
            path.clear();
            path_idx = 0;
            if let Some(p) = next_explore_path(&gs) {
                path = p;
            }
            if !path.is_empty() {
                let (nx, ny) = path[0];
                path_idx = 1;
                GameCommand::Move {
                    dx: nx - gs.entities[0].x,
                    dy: ny - gs.entities[0].y,
                }
            } else {
                GameCommand::Wait
            }
        };

        gs.step(cmd);
        after_step(&mut gs, &mut session, cmd);
    }

    let preset_name = preset.map(|p| format!("{:?}", p)).unwrap_or_else(|| "default".to_string());
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

            let updated_json =
                serde_json::to_string_pretty(&golden).expect("failed to serialize");
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
fn run_sweep(path: &str, analytics_enabled: bool, analysis_enabled: bool) {
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
                let ga = run_single_game_tracked(
                    config.width,
                    config.height,
                    game_seed,
                    config.preset,
                    config.max_turns,
                    false,
                    overrides,
                );
                point_analytics.push(ga);
            } else {
                // Still need analytics for sweep results.
                let ga = run_single_game_tracked(
                    config.width,
                    config.height,
                    game_seed,
                    config.preset,
                    config.max_turns,
                    false,
                    overrides,
                );
                point_analytics.push(ga);
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
        .map(|(_, e)| GameCommand::Move {
            dx: (e.x - px).signum(),
            dy: (e.y - py).signum(),
        })
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
        roguelike::pathfinding::nearest_by_cost(&gs.map, px, py, &frontier_set, &gs.explored)?;
    roguelike::pathfinding::find_path(&gs.map, px, py, tx, ty, &gs.explored)
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
        --help                 Show this help message"
    );
}
