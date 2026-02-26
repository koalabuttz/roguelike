### Roguelike Project Rules

#### Workspace Structure
Cargo workspace with 6 member crates + 2 standalone:
- `crates/core/` (roguelike-core) — game logic, zero platform deps. Contains `rules/` (no_std pure game rules), `tier_micro/` (complete C64 game engine), `tier_compact/` (GBA stubs), `game_step.rs` (cross-tier trait)
- `crates/saves/` (roguelike-saves) — SaveBackend trait
- `crates/tui/` (roguelike-tui) — shared terminal game loop + rendering. Contains `render_source.rs` (RenderSource cross-tier trait)
- `crates/terminal/` (roguelike-terminal) — crossterm frontend
- `crates/mcp/` (roguelike-mcp) — MCP server for LLM play
- `crates/ssh/` (roguelike-ssh) — SSH server frontend
- `crates/c64/` (roguelike-c64) — C64 thin frontend over core (not a workspace member, builds via rust-mos)
- `crates/libudev-sys-dlopen/` — dlopen-based libudev replacement (not a workspace member)

#### Build & Test (ALL must pass before committing)
```
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

#### Key Conventions
- One logical change per commit
- Player is always `entities[0]`
- Put helpers where the data lives (map queries → `map.rs`, entity queries → `game.rs`)
- Only orchestration on `GameState`
- Tier-friendly coding: enums over strings, pure functions for rules, const for limits
- Keep balance data in `crates/core/data/game.toml` (compiled into binary, not runtime-loaded)

#### Feature Flags (conditional compilation)
- `std` — gates standard-tier code (shadowcasting FOV, A* pathfinding, Vec-based collections). On by default.
- `serde` — serialization/deserialization for save/load. On by default.
- `dev-tools` — debug console, overlays, analytics, headless runner, replay system (on by default for terminal/tui)
- `data-files` — game.toml loading via TOML parser. On by default.
- `gamepad` — gilrs gamepad input (on by default for terminal). Requires `libudev-dev` on Linux.
- `raw-usb` — raw USB Xbox controller fallback via `rusb` (implies `gamepad`). Requires `libusb-1.0-0-dev`.
- Build without optional features: `cargo build --no-default-features --features dev-tools`

#### Determinism Requirement
Game logic MUST be deterministic given the same seed and input sequence. Golden replay tests verify this.
- Do NOT use `HashMap` iteration order in game logic (use `Vec` or `BTreeMap`)
- Do NOT use system time, thread IDs, or other non-deterministic sources in game logic
- All randomness must come from the seeded RNG (`rand::rngs::StdRng`)
- If golden replays break after an intentional change, regenerate them:
  `cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/`

#### Balance CI
Changes to gameplay files (`combat.rs`, `entity.rs`, `spawn.rs`, `map.rs`, `game.rs`, `ai.rs`, `data.rs`, `analytics.rs`) trigger `.github/workflows/balance.yml` — runs 500+ deterministic games and compares against a cached baseline. Balance regressions are posted as PR comments.

#### Core API Surface
`GameState` in `game.rs` is the central orchestrator. Key methods:
- `step(command)` — process one game command (move, wait, pickup, use, equip, descend, etc.)
- `autorun(direction)` — keep moving until wall/monster/junction
- `auto_fight()` — resolve adjacent combat to completion
- `observe()` — return current visible state (used by MCP server)
- `look_at(pos)` — examine a tile in look mode
New gameplay features should integrate through these methods, not bypass them.

`GameStep` trait in `game_step.rs` provides the cross-tier interface:
- Implemented by `GameState` (standard) and `MicroGameStateAdapter` (micro)
- `create_game()` factory routes micro seeds to the appropriate tier
- Used by MCP server, TUI game loop, and FrameSink

#### Python Tools (use `python3`, not `python`)
- `tools/llm_playtest.py` — LLM-driven playtesting (claude-code or API backend)
- `tools/visualize.py` — matplotlib analytics visualizer
- `tools/balance_diff.py` — balance comparison (stdlib only)
- `tools/playtest_analytics.py` — shared analytics module

#### Linux Build Dependency
The `gamepad` feature (on by default) requires `pkg-config` and `libudev-dev`.
Build without gamepad: `cargo build --no-default-features --features dev-tools`
