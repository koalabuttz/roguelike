# Contributing

Thanks for your interest in contributing to this project!

## License

This project is licensed under **GPL-3.0-or-later**. The author reserves the right to release platform-specific ports (e.g., console, mobile) under a commercial license.

By submitting a pull request, you agree that your contributions may be included in commercially licensed builds. The open source version will always remain fully functional and identical in gameplay.

## Getting Started

1. Fork the repository and clone your fork
2. Create a feature branch: `git checkout -b my-feature`
3. Make your changes

## Before Submitting

All of the following must pass:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

CI runs these automatically on every pull request.

If golden replay tests fail after an intentional gameplay change, regenerate them:

```sh
cargo run --bin headless -- --regenerate-goldens tests/golden_replays/
```

## Development Workflow

### Running Checks Locally

Before pushing, run the validation commands above. You can run them individually during development or all at once before committing.

### Pre-commit Hook (Recommended but Optional)

To automatically run all checks before each commit:

```sh
# One-time setup
cp .github/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

**Why it's optional**: The hook helps catch issues early and saves CI minutes, but it's not required — GitHub Actions will run the same checks on every PR. Some developers prefer to commit freely and rely on CI feedback.

**Best practice**: If you're actively developing, enabling the hook prevents you from pushing broken code and provides faster feedback than waiting for CI.

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

The project has three levels of testing. Use the appropriate level for each type of change.

### Unit Tests (module-level)

Standard `#[test]` functions inside each module's `#[cfg(test)] mod tests` block. These test individual functions and methods in isolation. Run with `cargo test --lib`.

### Scenario Tests (balance assertions)

Use the scenario framework (`src/scenario.rs`) when testing how gameplay changes affect balance. Scenarios use a fluent builder API to set up specific game states and assert outcomes:

```rust
use roguelike::scenario::Scenario;
use roguelike::map::MapPreset;

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

Add scenario tests to `tests/scenarios.rs` when:
- Changing monster stats, player stats, or combat formulas
- Adding new monster types
- Modifying regeneration, spawning, or AI behavior

### Golden Replay Tests (regression detection)

Golden replays are stored recordings of full game playthroughs with their expected final state. They detect when a code change unintentionally alters gameplay. Located in `tests/golden_replays/`.

**When golden tests fail**: If the change was intentional (e.g., you rebalanced damage), regenerate:

```sh
cargo run --bin headless -- --regenerate-goldens tests/golden_replays/
```

**Adding a new golden**: Use the headless runner to generate one:

```sh
cargo run --bin headless -- --save-golden tests/golden_replays/seed_99_arena.json --seed 99 --preset arena
```

Then add a corresponding test in `tests/golden_replays.rs`:

```rust
#[test]
fn golden_seed_99_arena() {
    load_and_verify("tests/golden_replays/seed_99_arena.json");
}
```

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

## Adding a Monster

The simplest way to contribute content — see the [README](README.md#adding-a-new-monster) for a step-by-step guide.

## Questions?

Open an issue to discuss before starting large changes.
