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
//! ```

use std::collections::HashSet;

use roguelike::dev_tools::{BatchRunStats, DevSession, Replay, ReplayResult, after_step};
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

    // Mode 2: Batch run.
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

    // Output structured JSON to stdout.
    println!(
        "{}",
        serde_json::to_string_pretty(&stats).expect("failed to serialize stats")
    );
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
    -n, --games N          Number of games to run (default: 10)
    -w, --width N          Map width (default: 80)
    -H, --height N         Map height (default: 40)
    -s, --seed N           Starting seed (increments per game)
    -p, --preset NAME      Map preset: arena, corridor, labyrinth, single_room, open_field
    -t, --max-turns N      Max turns per game (default: 500)
    -r, --replay FILE      Replay a recorded game from JSON file
        --save-replays     Save replay JSON for each game
        --help             Show this help message"
    );
}
