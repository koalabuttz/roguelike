# Developer Tooling Session Report — 2026-02-15

## Summary

Added four high-leverage developer features to accelerate development,
then iterated twice to align them with the project's modular architecture
and cross-platform portability goals (GBA/C64 roadmap).

**Commits:**

1. `c963198` — Add developer tooling: debug console, map presets, replay system, headless runner
2. `ad1a7e4` — Refactor dev_tools to respect modular architecture
3. `b7fed58` — Improve dev_tools portability for future GBA/C64 targets

**Files changed:** 8 files, +1109 lines
**Tests:** 274 passing, clippy clean, cargo fmt clean

---

## Features Added

### 1. Debug Console (`src/dev_tools.rs`)

Ten dev commands accessible via F1–F5 in debug builds:

| Key | Command | Description |
|-----|---------|-------------|
| F1  | `DumpStats` | Print turn, seed, HP, ATK, DEF, monster counts, exploration % |
| F2  | `ToggleFov` | Disable/enable FOV — see entire map |
| F3  | `ToggleGodMode` | Player takes no damage |
| F4  | `RevealMap` | Add all tiles to explored set |
| F5  | `KillAll` | Kill all living monsters |
| —   | `Teleport { x, y }` | Move player to any walkable tile |
| —   | `SetHp { hp }` | Set player HP (clamped to 1..max_hp) |
| —   | `SetAttack { attack }` | Set player attack stat |
| —   | `SetDefense { defense }` | Set player defense stat |
| —   | `Spawn { name, x, y }` | Spawn goblin/orc/troll at position |

Keybindings are behind `#[cfg(all(debug_assertions, feature = "dev-tools"))]`
so they never compile into release builds.

### 2. Map Generation Presets (`src/map.rs`)

Five deterministic map layouts for testing specific scenarios:

| Preset | Description |
|--------|-------------|
| `Arena` | Large open room with surrounding corridors |
| `Corridor` | Long narrow hallway with alcoves |
| `Labyrinth` | Randomized maze using recursive division |
| `SingleRoom` | One big room (combat testing) |
| `OpenField` | Completely open floor (pathfinding stress test) |

Constructor: `GameState::with_preset(width, height, seed, MapPreset::Arena)`

### 3. Replay System (`src/dev_tools.rs`)

Deterministic recording and playback of game sessions:

- `DevSession` records `GameCommand` sequences when `recording = true`
- `Replay` struct captures seed + dimensions + commands
- `Replay::execute()` recreates the exact game and replays all commands
- `ReplayResult` summarizes: turns played, final HP, kills, game over state
- Derives `Serialize`/`Deserialize` — callers choose format at boundary

### 4. Headless Runner (`src/bin/headless.rs`)

CLI binary for automated playtesting without rendering:

```sh
# Batch run 100 games, collect stats:
cargo run --bin headless -- --games 100

# Specific seed with a preset:
cargo run --bin headless -- --seed 42 --preset arena --games 1

# Replay a recorded game:
cargo run --bin headless -- --replay replay_42.json

# Save replays for every game:
cargo run --bin headless -- --games 10 --save-replays
```

Outputs `BatchRunStats` as structured JSON to stdout: games won/lost,
average turns, average kills, seeds used.

Strategy: auto-explore to find monsters, auto-fight when adjacent.

---

## Architecture Decisions

### DevSession Pattern

Debug state lives in `DevSession`, owned by the caller (main loop or
headless runner) — never on `GameState`. This keeps the core game struct
clean for all platforms.

```
┌─────────────┐     ┌────────────┐
│  main.rs    │────▶│ DevSession │  caller owns debug state
│  headless.rs│     └────────────┘
└──────┬──────┘            │
       │              exec_dev()
       ▼              after_step()
┌─────────────┐     replay_commands()
│  GameState  │◀──── free functions operate on public API
└─────────────┘
```

### Free Functions, Not Methods

`exec_dev()`, `after_step()`, and `replay_commands()` are free functions
in `dev_tools.rs`, not `impl GameState` methods. This follows the method
placement rule: GameState's interface is defined entirely in `game.rs`.

### No Debug Branches in Hot Paths

`step()` and `update_fov()` remain pure game logic. FOV override and god
mode are applied by callers via `after_step()` after each `step()` call.

---

## Platform Portability

Three changes ensure compatibility with the GBA/C64 roadmap:

1. **Feature gate:** `dev_tools` module is behind `#[cfg(feature = "dev-tools")]`
   (default on). Constrained platforms exclude it with `default-features = false`.

2. **No `serde_json` in core-adjacent code:** `Replay` derives
   `Serialize`/`Deserialize` but has no `to_json()`/`from_json()` methods.
   Callers (headless.rs, tests) call `serde_json` directly at the boundary.

3. **Type aliases throughout:** `ReplayResult` and `BatchRunStats` use `Stat`
   and `Coord` instead of bare `i32`. When these aliases change to `i16`/`i8`
   for GBA/C64 (Phase 1), these structs follow automatically.

---

## Test Coverage

20 new tests in `dev_tools.rs` covering:

- All 10 dev commands (teleport success/fail, stat clamping, spawn known/unknown/wall, reveal, FOV toggle, kill all, dump stats, god mode)
- God mode death prevention via `after_step()`
- Command recording
- Replay JSON roundtrip (serialization at boundary)
- Deterministic replay (same seed = same outcome)
- Replay with map presets
- Replay stops on game over
- `with_preset()` constructor validity

7 new tests in `map.rs` covering all five preset generators.
