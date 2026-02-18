# Contributing

Thanks for your interest in contributing to this project!

## License

This project is licensed under **GPL-3.0-or-later**. The author reserves the right to release platform-specific ports (e.g., console, mobile) under a commercial license.

By submitting a pull request, you agree that your contributions may be included in commercially licensed builds. The open source version will always remain fully functional and identical in gameplay.

## Getting Started

1. Fork the repository and clone your fork
2. Enable the pre-commit hook: `git config core.hooksPath .github/hooks`
3. Create a feature branch: `git checkout -b my-feature`
4. Make your changes

## Before Submitting

All of the following must pass:

```sh
cargo fmt --all --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

CI runs these automatically on every pull request.

If golden replay tests fail after an intentional gameplay change, regenerate them:

```sh
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/
```

## Development Workflow

### Running Checks Locally

Before pushing, run the validation commands above. You can run them individually during development or all at once before committing.

### Pre-commit Hook (Recommended)

The pre-commit hook runs fmt, clippy, and tests before each commit. Enable it with:

```sh
git config core.hooksPath .github/hooks
```

This points git at the repo's hooks directory so the hook stays in sync with the codebase. CI runs the same checks on every PR, so the hook is technically optional — but it gives much faster feedback than waiting for CI.

## Edition 2024 Notes

This project uses Rust edition 2024. Key differences:

- **`gen` is a reserved keyword** — Use `r#gen()` when calling `rand::Rng::gen()` directly
- `gen_range()` and `gen_bool()` work normally (not exact match on `gen`)
- Requires Rust 1.85.0 or later

## Guidelines

- **One logical change per PR** — Don't mix features with refactors or bug fixes
- **Add tests** — New features and behavior changes should include unit tests
- **Keep it modular** — New systems should be self-contained modules with clear interfaces
- **Follow existing patterns** — Look at how current modules are structured before adding new ones

## Code Organization

### Method Placement Rule

Place functions based on **what they operate on**, not who calls them:

- **Map topology queries** → `map.rs` (e.g., `is_walkable`, `get_neighbors`)
- **Entity spatial queries** → `game.rs` as public functions (e.g., `entity_at`, `find_monsters`)
- **Game logic systems** → Own module (e.g., `combat.rs`, `ai.rs`, `spawn.rs`)
- **Orchestration only** → `GameState` methods (multi-step game actions like `step()`, `autorun()`)
- **Helpers go where the data lives**

### Project Conventions

- **Player entity** — Always `entities[0]` in the entity list
- **Tests** — Use `#[cfg(test)] mod tests` blocks at the bottom of each source file
- **Test coverage** — Add or update tests when adding features or changing behavior

## Testing Systems

The project has five levels of testing. Use the appropriate level for each type of change.

### Unit Tests (module-level)

Standard `#[test]` functions inside each module's `#[cfg(test)] mod tests` block. These test individual functions and methods in isolation. Run with `cargo test -p roguelike-core --lib`.

### Scenario Tests (balance assertions)

Use the scenario framework (`crates/core/src/scenario.rs`) when testing how gameplay changes affect balance. Scenarios use a fluent builder API to set up specific game states and assert outcomes:

```rust
use roguelike_core::scenario::Scenario;
use roguelike_core::map::MapPreset;

#[test]
fn new_monster_is_survivable() {
    Scenario::new(20, 20, 42)
        .preset(MapPreset::SingleRoom)
        .kill_all()
        .spawn("goblin", 4, 5)
        .run_turns(50)
        .assert_alive()
        .assert_kills(1);
}
```

Available builder methods: `.preset()`, `.kill_all()`, `.spawn(name, x, y)`, `.set_player_hp()`, `.set_player_attack()`, `.set_player_defense()`, `.god_mode()`, `.teleport(x, y)`, `.reveal_map()`, `.mutate(closure)`.

Run methods: `.run_turns(n)` (auto-fight AI), `.run_auto_fight(n)` (fight adjacent only).

Assertions: `.assert_alive()`, `.assert_dead()`, `.assert_hp(n)`, `.assert_hp_between(min, max)`, `.assert_kills(n)`, `.assert_monsters_alive(n)`, `.assert_turns(n)`, `.assert_turns_less_than(n)`. All are chainable.

Add scenario tests to `crates/core/tests/scenarios.rs` when:
- Changing monster stats, player stats, or combat formulas
- Adding new monster types
- Modifying regeneration, spawning, or AI behavior

### Golden Replay Tests (regression detection)

Golden replays are stored recordings of full game playthroughs with their expected final state. They detect when a code change unintentionally alters gameplay. Located in `crates/core/tests/golden_replays/`.

**When golden tests fail**: If the change was intentional (e.g., you rebalanced damage), regenerate:

```sh
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/
```

**Adding a new golden**: Use the headless runner to generate one:

```sh
cargo run --bin headless -- --save-golden crates/core/tests/golden_replays/seed_99_arena.json --seed 99 --preset arena
```

Then add a corresponding test in `crates/core/tests/golden_replays.rs`:

```rust
#[test]
fn golden_seed_99_arena() {
    load_and_verify(&format!("{}/seed_99_arena.json", GOLDEN_DIR));
}
```

### Invariant Property Tests (core game invariants)

Property-based tests using `proptest` that generate random `GameCommand` sequences and verify fundamental invariants hold after every `step()`. Located in `crates/core/tests/invariants.rs`.

Invariants checked:
- `player.hp <= player.max_hp`
- `game_over == true` iff `player.hp <= 0`
- Explored set never shrinks
- Dead entities stay dead
- Player is always on a walkable tile (while alive)
- `observe()` and `save_to_json()`/`load_from_json()` never panic

Run with `cargo test -p roguelike-core --test invariants`. These are most useful when changing core game logic (combat, movement, entity lifecycle, save/load).

### MCP Integration & Property Tests (MCP session layer)

Two test files in `crates/mcp/tests/`:

**`mcp_integration.rs`** — Deterministic tests calling MCP tool methods directly on `RoguelikeMcpServer`. Covers all 10 tools: response schemas, error paths (e.g., calling tools before `new_game`), session lifecycle (reset, save persistence across games), and edge cases (compact mode, invalid actions, auto\_fight metadata).

**`mcp_proptest.rs`** — Property-based tests generating random sequences of MCP tool calls. Verifies that the MCP session layer (mutex logic, JSON serialization, error handling) preserves game invariants through the JSON interface. Includes tests for random tool sequences, save/load roundtrips, and panic-freedom (intentionally skipping `new_game` to test error resilience).

Run with `cargo test -p roguelike-mcp`. Add or update these tests when changing MCP tool implementations, response formats, or session management.

### Headless Analytics (exploratory testing)

For exploratory balance investigation, use the headless runner with `--analytics`:

```sh
# Run 100 games and see aggregate combat stats:
cargo run --bin headless -- --games 100 --analytics

# Run with full analysis (difficulty metrics, monster correlations):
cargo run --bin headless -- --games 100 --analytics --analysis

# Sweep across player HP values to find the survivability threshold:
cargo run --bin headless -- --sweep sweep.json
```

This is useful during development but doesn't replace automated tests.

### CI Balance Check

When you push changes to gameplay-relevant files (combat, entity, spawn, map, AI, data, analytics, headless), the `balance.yml` workflow runs automatically. It:

- Runs 500 default + 50 arena + 50 corridor games with deterministic seeds
- Compares results against the previous baseline cached in CI
- Posts a balance diff to the workflow run's **Summary tab**
- On PRs, posts/updates a comment with the verdict: **STABLE**, **MINOR SHIFT**, or **BALANCE SHIFT**

You can also run the diff tool locally to preview what CI will report:

```sh
# Run headless twice (before and after your change), then compare:
cargo run --release --bin headless -- --games 500 --seed 1 --analytics > before.json
# ... make your gameplay change ...
cargo run --release --bin headless -- --games 500 --seed 1 --analytics > after.json
python3 -c "import json,sys; json.dump({'default':json.load(open('before.json'))},sys.stdout)" > baseline.json
python3 -c "import json,sys; json.dump({'default':json.load(open('after.json'))},sys.stdout)" > current.json
python3 tools/balance_diff.py baseline.json current.json
```

### Visualization Tools

The `tools/` directory contains Python scripts for analytics visualization. These require a virtual environment:

```sh
python3 -m venv tools/.venv
source tools/.venv/bin/activate
pip install -r tools/requirements.txt
```

Alternatively, use the built-in `--report` flag on the headless runner to generate HTML reports without Python.

See the [README](README.md#visualization) for full usage examples.

### LLM Playtesting

The `tools/llm_playtest.py` script requires an `ANTHROPIC_API_KEY` environment variable. This drives games through the Anthropic API, letting an LLM play strategically instead of using the dumb auto-explore AI. See the [README](README.md#llm-playtesting) for usage.

The `/playtest` skill works in Claude Code sessions with the MCP server connected — no API key needed since Claude Code uses its own session.

## Adding a Monster

The simplest way to contribute content — see the [README](README.md#adding-a-new-monster) for a step-by-step guide.

## Questions?

Open an issue to discuss before starting large changes.
