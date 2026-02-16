//! Golden replay regression tests.
//!
//! Each test loads a golden replay JSON file and verifies that re-executing
//! the replay produces the same result. If any test fails after a code change,
//! it means the change altered game behavior — intentional changes should
//! regenerate the goldens via `--regenerate-goldens`.

use roguelike::dev_tools::GoldenReplay;

fn load_and_verify(path: &str) {
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    let golden: GoldenReplay = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path, e));
    if let Err(msg) = golden.verify() {
        panic!("{}", msg);
    }
}

#[test]
fn golden_seed_42_default() {
    load_and_verify("tests/golden_replays/seed_42_default.json");
}

#[test]
fn golden_seed_42_arena() {
    load_and_verify("tests/golden_replays/seed_42_arena.json");
}

#[test]
fn golden_seed_100_corridor() {
    load_and_verify("tests/golden_replays/seed_100_corridor.json");
}

#[test]
fn golden_seed_7_labyrinth() {
    load_and_verify("tests/golden_replays/seed_7_labyrinth.json");
}

#[test]
fn all_golden_replays_pass() {
    let dir = "tests/golden_replays";
    let entries = std::fs::read_dir(dir).expect("failed to read golden_replays directory");

    let mut count = 0;
    let mut failures = Vec::new();

    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let json = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
            let golden: GoldenReplay = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e));
            if let Err(msg) = golden.verify() {
                failures.push(format!("{}: {}", path.display(), msg));
            }
            count += 1;
        }
    }

    assert!(count > 0, "No golden replay files found in {}", dir);
    if !failures.is_empty() {
        panic!(
            "{} of {} golden replays failed:\n{}",
            failures.len(),
            count,
            failures.join("\n")
        );
    }
}
