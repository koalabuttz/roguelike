use std::collections::HashSet;

use criterion::{Criterion, criterion_group, criterion_main};

use roguelike_core::command::GameCommand;
use roguelike_core::exploration_graph;
use roguelike_core::fov;
use roguelike_core::game::GameState;
use roguelike_core::pathfinding;

const BENCH_SEED: u64 = 42;

/// Create a fully-explored game state for pathfinding/graph benchmarks.
fn fully_explored_state(width: i32, height: i32) -> GameState {
    let mut gs = GameState::with_seed(width, height, BENCH_SEED);
    // Mark all walkable tiles as explored.
    for y in 0..gs.map.height {
        for x in 0..gs.map.width {
            if gs.map.is_walkable(x, y) {
                gs.explored.insert((x, y));
            }
        }
    }
    gs.update_fov();
    gs
}

fn bench_step_move(c: &mut Criterion) {
    c.bench_function("step (move)", |b| {
        b.iter_batched(
            || {
                let mut gs = GameState::with_seed(80, 40, BENCH_SEED);
                gs.update_fov();
                gs
            },
            |mut gs| {
                gs.step(GameCommand::Move { dx: 1, dy: 0 });
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_step_wait(c: &mut Criterion) {
    c.bench_function("step (wait, with monsters)", |b| {
        b.iter_batched(
            || {
                let mut gs = GameState::with_seed(80, 40, BENCH_SEED);
                gs.update_fov();
                gs
            },
            |mut gs| {
                gs.step(GameCommand::Wait);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_compute_fov_r8(c: &mut Criterion) {
    let gs = GameState::with_seed(80, 40, BENCH_SEED);
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    c.bench_function("compute_fov (radius 8)", |b| {
        b.iter(|| fov::compute_fov(&gs.map, px, py, 8));
    });
}

fn bench_compute_fov_r16(c: &mut Criterion) {
    let gs = GameState::with_seed(80, 40, BENCH_SEED);
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    c.bench_function("compute_fov (radius 16)", |b| {
        b.iter(|| fov::compute_fov(&gs.map, px, py, 16));
    });
}

fn bench_find_path(c: &mut Criterion) {
    let gs = fully_explored_state(80, 40);
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    // Pick the farthest room center as the target.
    let (tx, ty) = gs
        .map
        .rooms
        .iter()
        .map(|r| r.center())
        .max_by_key(|&(rx, ry)| (rx - px).abs() + (ry - py).abs())
        .unwrap_or((px + 10, py + 10));
    c.bench_function("find_path (A* cross-map)", |b| {
        b.iter(|| pathfinding::find_path(&gs.map, px, py, tx, ty, &gs.explored));
    });
}

fn bench_nearest_by_cost(c: &mut Criterion) {
    let gs = fully_explored_state(80, 40);
    let px = gs.entities[0].x;
    let py = gs.entities[0].y;
    let targets: HashSet<(i32, i32)> = gs.frontier_tiles().into_iter().collect();
    // If no frontiers (fully explored), use room centers.
    let targets = if targets.is_empty() {
        gs.map.rooms.iter().map(|r| r.center()).collect()
    } else {
        targets
    };
    c.bench_function("nearest_by_cost (Dijkstra)", |b| {
        b.iter(|| pathfinding::nearest_by_cost(&gs.map, px, py, &targets, &gs.explored));
    });
}

fn bench_map_generation_80x40(c: &mut Criterion) {
    c.bench_function("map generation (80x40)", |b| {
        b.iter(|| GameState::with_seed(80, 40, BENCH_SEED));
    });
}

fn bench_map_generation_120x60(c: &mut Criterion) {
    c.bench_function("map generation (120x60)", |b| {
        b.iter(|| GameState::with_seed(120, 60, BENCH_SEED));
    });
}

fn bench_build_exploration_graph(c: &mut Criterion) {
    let gs = fully_explored_state(80, 40);
    c.bench_function("build_exploration_graph", |b| {
        b.iter(|| exploration_graph::build_exploration_graph(&gs));
    });
}

criterion_group!(
    benches,
    bench_step_move,
    bench_step_wait,
    bench_compute_fov_r8,
    bench_compute_fov_r16,
    bench_find_path,
    bench_nearest_by_cost,
    bench_map_generation_80x40,
    bench_map_generation_120x60,
    bench_build_exploration_graph,
);
criterion_main!(benches);
