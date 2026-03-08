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

> **Note:** The default build includes the `gamepad` feature (gilrs). On Linux, libudev is loaded at runtime via dlopen — no system packages are needed to build or run. If libudev is not installed at runtime, gamepad support is silently skipped and keyboard input works normally.
>
> **Raw USB feature:** The `raw-usb` feature (USB HID gamepad fallback for environments without `/dev/input/`) requires `libusb-1.0-0-dev` on Linux (`apt install libusb-1.0-0-dev`). Build/test with: `cargo test --workspace --features raw-usb`.

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

### Debug Keys (dev-tools build only)

When building with the `dev-tools` feature (enabled by default), the following debug keys are available:

| Key | Action |
|-----|--------|
| `F1` | Dump stats |
| `F2` | Toggle FOV |
| `F3` | Toggle god mode |
| `F4` | Reveal map |
| `F5` | Kill all monsters |
| `F6` | Toggle FOV boundary overlay |
| `F7` | Toggle monster targets overlay |
| `F8` | Toggle pathfinding overlay |
| `F9` | Toggle frontiers overlay |
| `F10` | Reload `game.toml` (hot reload) |
| `F11` | Toggle reveal monsters overlay |
| `F12` | Toggle monster FOV overlay |

In pathfinding overlay mode (`F8`) and monster FOV overlay mode (`F12`), arrow keys move a cursor to select individual paths/monsters. Press `Esc` to return to union mode. Monster FOV uses a three-state toggle: off → union (all monster sight boundaries) → cursor (single monster) → off.

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
- **Tier-aware coding** — The capability tier system (micro/compact/standard) is implemented. Pure game rules live in `rules/` (no_std, always compiled), tier micro is in `tier_micro/` (complete no_std game engine for C64). When adding gameplay features to `core`:
  - Prefer **enums over strings** for game concepts (item types, effects, equipment slots) — these map to `u8` discriminants on constrained platforms
  - Write **pure functions for rules** in `rules/` — if a calculation (damage, item effects) doesn't need `&self`, put it in `rules/` as a free/const function. See `rules::damage`, `rules::items`, `rules::monster_table` for examples. The `Inventory` struct and `InvSlot` type also live in `rules/items.rs`, shared across all tiers.
  - Use **named constants for limits** (inventory size, max items per room) rather than hardcoded literals. Per-tier constants live in `rules::balance`.
  - Keep **balance data in `game.toml`** — constants in `rules::balance` are compiled-in defaults; `game.toml` can override them

## Testing Systems

The project has five levels of testing. Use the appropriate level for each type of change.

### Quick Reference

| Category | Command | What it checks |
|----------|---------|----------------|
| Unit tests | `cargo test -p roguelike-core --lib` | Module-level logic across core modules |
| Golden replays | `cargo test -p roguelike-core --test golden_replays` | Deterministic replay regression — detects unintended gameplay changes |
| Scenario tests | `cargo test -p roguelike-core --test scenarios` | Balance properties — e.g., "player survives 2 goblins", "troll kills weak player" |
| Invariant tests | `cargo test -p roguelike-core --test invariants` | Property-based: random command sequences verify HP bounds, explored monotonicity, dead-stay-dead, save/load roundtrip |
| MCP integration | `cargo test -p roguelike-mcp --test mcp_integration` | All MCP tools: response schemas, error paths, session lifecycle |
| MCP property tests | `cargo test -p roguelike-mcp --test mcp_proptest` | Random MCP tool sequences verify game invariants hold through the JSON interface |
| Benchmarks | `cargo bench -p roguelike-core` | Criterion benchmarks for FOV, pathfinding, game step, and exploration graph |

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

**`mcp_integration.rs`** — Deterministic tests calling MCP tool methods directly on `RoguelikeMcpServer`. Covers all tools: response schemas, error paths (e.g., calling tools before `new_game`), session lifecycle (reset, save persistence across games), edge cases (compact mode, invalid actions, auto\_fight metadata), and inventory actions (pickup, use/equip/drop by slot letter).

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

### CI Pipeline

The `.github/workflows/ci.yml` workflow runs on every push and PR that touches `crates/`, `Cargo.toml`, `Cargo.lock`, or workflow files. It runs four jobs:

1. **Lint** — `cargo fmt --check` + `cargo clippy` (including `raw-usb` feature)
2. **Test** — `cargo test --workspace` on 4 platforms: Linux x86_64, Linux ARM64, macOS ARM64, Windows x86_64 (plus `raw-usb` feature tests on Linux)
3. **Benchmark** — `cargo bench -p roguelike-core` with Criterion reports uploaded as artifacts (30-day retention)
4. **Audit** — `cargo audit` for known vulnerabilities (advisory, non-blocking)

### Release Pipeline

The `.github/workflows/release.yml` workflow triggers on version tags (`v*`). It builds release binaries for all 4 platforms, generates SHA256 checksums, and creates a GitHub Release with all artifacts attached. Each release includes `roguelike`, `mcp_server`, `headless`, and `roguelike-ssh` binaries.

### Visualization Tools

The `tools/` directory contains Python scripts for analytics visualization. These require a virtual environment:

```sh
python3 -m venv tools/.venv
source tools/.venv/bin/activate
pip install -r tools/requirements.txt
```

Alternatively, use the built-in `--report` flag on the headless runner to generate HTML reports without Python.

See the [headless runner docs](docs/tooling/headless-runner.md#visualization) for full usage examples.

### LLM Playtesting

The `tools/llm_playtest.py` script requires an `ANTHROPIC_API_KEY` environment variable. This drives games through the Anthropic API, letting an LLM play strategically instead of using the dumb auto-explore AI. See the [LLM playtesting docs](docs/tooling/llm-playtesting.md) for full usage.

The `/playtest` skill works in Claude Code sessions with the MCP server connected — no API key needed since Claude Code uses its own session.

## Adding a Monster

The simplest way to contribute content.

**Without recompiling** — add a `[[monsters]]` entry to `game.toml` in the working directory:

```toml
[[monsters]]
name = "Dragon"
glyph = "D"
color = "Red"
hp = 40
attack = 10
defense = 5
ai = "Chase"
spawn_weight = 5
sight_radius = 8
```

The terminal, headless runner, and MCP server load `game.toml` on startup. In dev-tools builds, press `F10` to hot-reload without restarting.

**Compiled-in** — edit `crates/core/data/game.toml` (the embedded defaults) to add monsters permanently.

If a monster needs new AI, add a variant to `AiBehavior` in `crates/core/src/entity.rs` and implement it in `crates/core/src/ai.rs`.

When adding a new monster, also add a scenario test to `crates/core/tests/scenarios.rs` to verify the player can survive (or not) as intended. See [Scenario Tests](#scenario-tests-balance-assertions) above.

## Questions?

Open an issue to discuss before starting large changes.
