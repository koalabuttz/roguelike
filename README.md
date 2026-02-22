# Roguelike

A terminal-based roguelike dungeon crawler written in Rust.

Explore randomly generated dungeons, fight monsters, and try to survive. Renders directly in the terminal using ASCII characters and ANSI colors — no external game engine required.

## Installation

**Pre-built binaries** are available on the [Releases](https://github.com/koalabuttz/roguelike/releases) page for Linux (x86_64, ARM64), macOS (ARM64), and Windows (x86_64). Download, make executable, and run:

```sh
chmod +x roguelike-linux-x86_64   # Linux/macOS only
./roguelike-linux-x86_64
```

Each release also includes the `mcp_server`, `headless`, and `roguelike-ssh` binaries.

### Building from Source

Requires [Rust](https://www.rust-lang.org/tools/install) **1.85.0 or later** (for edition 2024 support).

```sh
cargo run
```

The game adapts to your terminal size automatically.

## Testing

```sh
cargo test --workspace               # All ~600 tests across all crates
cargo test -p roguelike-core --lib    # Unit tests across core modules
cargo test -p roguelike-core --test golden_replays # 5 golden replay regression tests
cargo test -p roguelike-core --test scenarios      # 8 balance integration tests
cargo test -p roguelike-core --test invariants     # Property-based core game invariant tests
cargo test -p roguelike-mcp                        # MCP integration + property tests
cargo bench -p roguelike-core              # Criterion benchmarks (FOV, pathfinding, step)
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

### Test Categories

| Category | Command | What it checks |
|----------|---------|----------------|
| Unit tests | `cargo test -p roguelike-core --lib` | Module-level logic across core modules |
| Golden replays | `cargo test -p roguelike-core --test golden_replays` | Deterministic replay regression — detects unintended gameplay changes |
| Scenario tests | `cargo test -p roguelike-core --test scenarios` | Balance properties — e.g., "player survives 2 goblins", "troll kills weak player" |
| Invariant tests | `cargo test -p roguelike-core --test invariants` | Property-based: random command sequences verify HP bounds, explored monotonicity, dead-stay-dead, save/load roundtrip |
| MCP integration | `cargo test -p roguelike-mcp --test mcp_integration` | All 10 MCP tools: response schemas, error paths, session lifecycle |
| MCP property tests | `cargo test -p roguelike-mcp --test mcp_proptest` | Random MCP tool sequences verify game invariants hold through the JSON interface |
| Benchmarks | `cargo bench -p roguelike-core` | Criterion benchmarks for FOV, pathfinding, game step, and exploration graph |

### Golden Replays

Golden replays are stored game recordings with their expected outcomes (`crates/core/tests/golden_replays/*.json`). After any code change, re-running them detects if game behavior has diverged. If a change is intentional (e.g., rebalancing monster stats), regenerate the goldens:

```sh
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/
```

To add a new golden replay:

```sh
cargo run --bin headless -- --save-golden crates/core/tests/golden_replays/seed_99_arena.json --seed 99 --preset arena
```

### Scenario Tests

Scenario tests use a fluent builder API to compose specific game states and assert outcomes. They live in `crates/core/tests/scenarios.rs` and are the recommended way to test balance changes:

```rust
Scenario::new(20, 20, 42)
    .preset(MapPreset::SingleRoom)
    .kill_all()
    .spawn("troll", 4, 5)
    .set_player_hp(10)
    .run_turns(50)
    .assert_dead();
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.

## Controls

| Action | Keys |
|--------|------|
| Move cardinal | Arrow keys, `hjkl` (vi), numpad `2468` |
| Move diagonal | `yubn` (vi), numpad `7913` |
| Autorun | Shift+arrow, `HJKLYUBN` (vi uppercase), Shift+numpad |
| Auto-explore | `o` |
| Look mode | `x` (move cursor to examine tiles, Esc to close) |
| Wait a turn | `.` or numpad `5` |
| Message history | `Ctrl+P` |
| Help | `?` |
| Quit | `q`, `Esc`, or `Ctrl+C` |

Vi keys and numpad are opt-in via Settings. Moving into a monster attacks it. Autorun keeps moving until hitting a wall, spotting a monster, or reaching a junction.

### Gamepad

Controller support is enabled by default (`gamepad` feature). Any XInput/DInput/HID gamepad recognized by [gilrs](https://crates.io/crates/gilrs) works — Xbox, PlayStation, Switch Pro, Steam Deck, etc. Keyboard and gamepad work simultaneously.

| Context | D-pad / Stick | A (South) | B (East) | X (West) | Y (North) | LB | RB | Start |
|---------|--------------|-----------|----------|----------|-----------|----|----|-------|
| **Gameplay** | Move (8-dir) | Wait | Pause menu | Auto-explore | Look mode | Autorun modifier | — | Pause menu |
| **Menu** | Up/Down | Select | Back | — | — | — | — | Select |
| **Look mode** | Move cursor | — | Close | — | — | — | — | Close |
| **Msg history** | Scroll up/down | Close | Close | — | — | Page up | Page down | Close |

The analog stick is edge-triggered: one command per deflection, return to center before the next. Hold LB + D-pad/stick for autorun. D-pad diagonals (e.g., Up+Right) produce diagonal movement.

To build without gamepad support: `cargo build --no-default-features --features dev-tools`

#### Raw USB Fallback (ChromeOS / Crostini)

In environments where the kernel lacks gamepad drivers (e.g. Crostini on ChromeOS), gilrs won't detect the controller because `/dev/input/` doesn't exist. If a USB HID gamepad is visible at `/dev/bus/usb/`, the `raw-usb` feature talks to it directly via USB HID reports:

```sh
cargo build --features raw-usb
```

Currently targets the 8BitDo SN30 Pro in DirectInput mode (`2dc8:6001`). Use the `usb_hid_test` example (`cargo run --example usb_hid_test --features raw-usb`) to verify HID report layouts on other controllers.

Requires `libusb-1.0-0-dev` on Linux (`apt install libusb-1.0-0-dev`). The `raw-usb` feature implies `gamepad` and acts as an automatic fallback — gilrs is tried first, and raw USB is only used when gilrs finds zero gamepads.

### Debug Keys (dev-tools build only)

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

## MCP Server (AI Play)

An LLM agent (like Claude) can play the game through the [Model Context Protocol](https://modelcontextprotocol.io/) server.

```sh
cargo run --bin mcp_server
```

The server communicates over stdio and exposes these tools:

| Tool | Description |
|------|-------------|
| `new_game` | Start a game (optional `width`/`height`/`seed`/`compact`/`seed_code` params) |
| `observe` | Get visible state: map, entities, HP, messages |
| `act` | Take an action: move, wait, autorun, or auto\_fight |
| `look_at` | Examine a tile: terrain, entity info, visibility (no turn consumed) |
| `pathfind_to` | A\* pathfind to a target tile; stops for monsters or damage |
| `auto_explore` | Find nearest frontier and walk to it in one call |
| `get_explored_map` | Full explored map with frontier markers (`~`) |
| `save_game` | Save current state to an in-memory slot |
| `load_game` | Restore a previously saved game state |
| `get_rules` | Read game mechanics and strategy tips |

### Spectator Mode

Set `ROGUELIKE_SPECTATE_PATH` to watch the LLM play in a separate terminal:

```sh
# Terminal 1: start the MCP server with spectating enabled
ROGUELIKE_SPECTATE_PATH=/tmp/roguelike-spectate.txt cargo run --bin mcp_server

# Terminal 2: watch the game (using the helper script)
./tools/spectate.sh

# Or watch a specific seed:
./tools/spectate.sh 12345
```

Frames are written atomically after every action. The spectator file shows the full explored map, HP/turn/kills status, and recent log messages.

To use with Claude Desktop, add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "roguelike": {
      "command": "cargo",
      "args": ["run", "--bin", "mcp_server", "--manifest-path", "/path/to/roguelike/crates/mcp/Cargo.toml"]
    }
  }
}
```

### Testing the MCP Server Locally

Use the MCP inspector to test server functionality without Claude Desktop:

```sh
npx @modelcontextprotocol/inspector cargo run --bin mcp_server
```

This opens a web UI where you can manually invoke tools and verify responses. Useful for debugging tool implementations or testing changes before integrating with Claude Desktop.

## SSH Server (Multiplayer)

Play over SSH — no installation needed on the client side. A dgamelaunch-style lobby handles user registration and login with argon2 password hashing. Each user gets persistent saves and settings.

```sh
# Start the SSH server:
cargo run --bin roguelike-ssh -- --port 2222

# Connect from any machine:
ssh -p 2222 localhost
```

### Features

- **Account system** — Register/login from the lobby; passwords hashed with argon2
- **Server menu** — Post-login menu (Play / Watch a Game / Log Out) with lobby↔session loop
- **Per-user saves** — Autosave, 5 manual save slots, and settings per account
- **Full game experience** — Title screen, settings, save/load, look mode, message history; "Lobby" replaces "Quit" in SSH menus to return to the server menu
- **Terminal resize** — Adapts to client terminal size; enforces 60x20 minimum
- **Connection limits** — Configurable max connections (default: 64)

### CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 2222 | Listen port |
| `--data-dir` | `~/.local/share/roguelike-ssh/` | Data directory for accounts, saves, and host key |
| `--max-connections` | 64 | Maximum simultaneous connections |
| `--idle-timeout` | 30 | Idle timeout in minutes |

Environment variables `ROGUELIKE_SSH_PORT` and `ROGUELIKE_SSH_DATA_DIR` also work.

## Headless Runner (Automated Playtesting)

The headless runner plays games automatically using auto-explore + auto-fight AI. It requires the `dev-tools` feature (enabled by default).

```sh
# Run 100 games, output aggregate JSON stats:
cargo run --bin headless -- --games 100

# Run with analytics (per-monster damage/kill tracking):
cargo run --bin headless -- --games 50 --analytics

# Run with analytics + difficulty analysis:
cargo run --bin headless -- --games 50 --analytics --analysis

# Run a parameter sweep (vary player stats, measure win rate):
cargo run --bin headless -- --sweep sweep.json

# Save a golden replay for regression testing:
cargo run --bin headless -- --save-golden crates/core/tests/golden_replays/my_test.json --seed 42

# Regenerate all golden replays after an intentional gameplay change:
cargo run --bin headless -- --regenerate-goldens crates/core/tests/golden_replays/

# Replay a recorded game:
cargo run --bin headless -- --replay replay.json
```

### CLI Flags

| Flag | Description |
|------|-------------|
| `-n`, `--games N` | Number of games to run (default: 10) |
| `-w`, `--width N` | Map width (default: 80) |
| `-H`, `--height N` | Map height (default: 40) |
| `-s`, `--seed N` | Starting seed (increments per game) |
| `-p`, `--preset NAME` | Map preset: `arena`, `corridor`, `labyrinth`, `single_room`, `open_field` |
| `-t`, `--max-turns N` | Max turns per game (default: 500) |
| `-r`, `--replay FILE` | Replay a recorded game from JSON |
| `--save-replays` | Save replay JSON for each game |
| `--analytics` | Collect per-game combat analytics (snapshot/diff each step) |
| `--analysis` | With `--analytics`, compute difficulty metrics and monster correlations |
| `--report FILE` | Generate self-contained HTML report with charts (requires `--analytics` or `--sweep`) |
| `--sweep FILE` | Run parameter sweep from JSON config |
| `--save-golden FILE` | Save run as golden replay JSON for regression testing |
| `--regenerate-goldens DIR` | Re-execute all goldens in a directory, update expected outcomes |

### Parameter Sweep Config

Sweeps test how game balance changes across different player configurations:

```json
{
  "axes": [
    { "param": "player_hp", "values": [10, 20, 30] },
    { "param": "player_attack", "values": [3, 5, 7] }
  ],
  "games_per_point": 10,
  "width": 80,
  "height": 40,
  "max_turns": 500,
  "preset": null
}
```

Supported sweep parameters: `player_hp`, `player_attack`, `player_defense`, `regen_interval`, `max_monsters_per_room`.

### Visualization

Two tools for visualizing analytics output:

**HTML Report** (built-in, zero dependencies):

```sh
# Basic report with charts and insights:
cargo run --bin headless -- --games 100 --analytics --report report.html

# Full report with analysis (monster danger, damage flow):
cargo run --bin headless -- --games 100 --analytics --analysis --report report.html

# Sweep report:
cargo run --bin headless -- --sweep sweep.json --report sweep_report.html
```

Opens in any browser. Uses Chart.js (loaded from CDN) with a dark theme.

**Balance diff** (`tools/balance_diff.py`, stdlib only):

```sh
# Compare two combined stats JSON files and output a markdown diff:
python3 tools/balance_diff.py baseline.json current.json
```

Compares win rate, avg turns/kills/HP/explored across presets, flags per-monster damage changes >= 5%, and emits a verdict: STABLE, MINOR SHIFT, or BALANCE SHIFT. Used automatically by the CI balance workflow.

**Python charts** (`tools/visualize.py`, requires matplotlib):

```sh
# Setup (one-time):
python3 -m venv tools/.venv
source tools/.venv/bin/activate
pip install -r tools/requirements.txt

# Batch analytics -> PNGs:
cargo run --bin headless -- --games 100 --analytics | python3 tools/visualize.py batch

# Sweep results -> PNGs:
cargo run --bin headless -- --sweep sweep.json | python3 tools/visualize.py sweep

# Analysis data -> PNGs:
cargo run --bin headless -- --games 100 --analytics --analysis 2>analysis.json
python3 tools/visualize.py analysis analysis.json
```

Output PNGs are saved to `tools/output/` (or `--output-dir DIR`). Both tools also print text insights to stdout.

### CI Balance Check

The `.github/workflows/balance.yml` workflow runs automatically when gameplay-relevant files change (combat, entity, spawn, map, AI, data, analytics, headless). It:

1. Builds the headless runner in release mode
2. Runs 3 presets with deterministic seeds: default (500 games), arena (50), corridor (50)
3. Compares against a cached baseline from the previous run
4. Posts a balance diff to the workflow run's **Summary tab** (visible on every push)
5. On PRs, also posts/updates a comment with the diff
6. Uploads HTML reports and stats as artifacts (14-day retention)

The diff classifies changes as **STABLE**, **MINOR SHIFT** (2pp+ win rate or 5%+ avg turns), or **BALANCE SHIFT** (5pp+ win rate or 10%+ turns/kills). Since all runs use `--seed 1`, results are deterministic — any delta means actual gameplay behavior changed.

### CI Pipeline

The `.github/workflows/ci.yml` workflow runs on every push and PR that touches `crates/`, `Cargo.toml`, `Cargo.lock`, or workflow files. It runs four jobs:

1. **Lint** — `cargo fmt --check` + `cargo clippy` (including `raw-usb` feature)
2. **Test** — `cargo test --workspace` on 4 platforms: Linux x86_64, Linux ARM64, macOS ARM64, Windows x86_64 (plus `raw-usb` feature tests on Linux)
3. **Benchmark** — `cargo bench -p roguelike-core` with Criterion reports uploaded as artifacts (30-day retention)
4. **Audit** — `cargo audit` for known vulnerabilities (advisory, non-blocking)

### Release Pipeline

The `.github/workflows/release.yml` workflow triggers on version tags (`v*`). It builds release binaries for all 4 platforms, generates SHA256 checksums, and creates a GitHub Release with all artifacts attached. Each release includes `roguelike`, `mcp_server`, `headless`, and `roguelike-ssh` binaries.

### LLM Playtesting

Strategic LLM-driven playtesting where an LLM plays the game making tactical decisions (fight, flee, explore) rather than the headless runner's simple auto-explore + auto-fight AI.

The system prompt teaches the LLM combat math (ATK - DEF = damage per round, compute rounds-to-kill vs rounds-to-die) and recovery strategies (corridor running for safe regen, corner kiting to kill tough monsters). The LLM chooses between aggressive, cautious, and exploratory strategies based on each encounter.

**`/playtest` skill** (Claude Code, interactive):

```sh
# Play 5 games (default) using MCP tools in the current session:
/playtest

# Play 10 games with a specific starting seed:
/playtest 10 --seed 42
```

The skill uses the connected MCP server directly. Results are saved to `tools/output/llm_playtest_results.json`.

**`tools/llm_playtest.py`** (standalone, dual-backend, unattended):

```sh
# Setup:
pip install -r tools/requirements.txt
cargo build --release --bin mcp_server

# Claude Code backend (uses `claude` CLI, parallel execution):
python3 tools/llm_playtest.py --backend claude-code -n 10 --parallel 5

# API backend (uses Anthropic API directly):
ANTHROPIC_API_KEY=... python3 tools/llm_playtest.py --backend api -n 50

# Reproducible runs with specific seeds:
python3 tools/llm_playtest.py --backend claude-code -n 5 --seed 63519 --parallel 5

# Custom budget and output path:
python3 tools/llm_playtest.py --backend claude-code -n 10 --max-budget 2.00 -o results.json
```

Two backends:
- **`claude-code`**: Spawns `claude -p` subprocesses with MCP config. Supports parallel execution. Default budget: $2.00/game.
- **`api`**: Direct Anthropic API tool_use loop with a local MCP server subprocess. Strips map data from old tool results to reduce context growth. Requires `ANTHROPIC_API_KEY`.

Per-game analytics include token usage (input, output, cache creation, cache read), cost, tool call counts, and strategy notes. Both backends output `EnhancedBatchStats`-compatible JSON:

```sh
cat tools/output/llm_playtest_results.json | \
  python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['batch_stats']))" | \
  python3 tools/visualize.py batch
```

#### Token Optimization

The MCP server supports a `compact` mode (`new_game` with `compact=true`) that omits the ASCII map from all observation responses, significantly reducing token usage for LLM agents that only need stats and entity info. Observation field names are also shortened (e.g., `player_hp` → `hp`, `visible_entities` → `entities`) to reduce per-turn overhead. The API backend additionally strips map data from old conversation turns to limit context window growth.

## Gameplay

- `@` is you. Monsters appear as colored letters (`g`oblin, `o`rc, `T`roll).
- Monsters have their own sight range and chase you when *they* see you (not when you see them). Trolls are dim-sighted and easy to sneak past; goblins are alert scouts.
- `%` marks a corpse. Dead monsters stay on the map.
- The HP bar and message log are at the bottom of the screen.
- **Title screen** lets you start a new game, enter a seed code, load a save, or adjust settings.
- **Seed codes** are shown on the death screen and in MCP observations. Enter one from the title menu to replay the exact same dungeon. Format: `<base36_seed>[-<W>x<H>][preset_char]` — e.g., `r7z3kq`, `r7z3kq-120x60a`.
- **Classic mode** (default): NetHack-style save discipline — saving quits, death deletes the save.
- **Casual mode**: 5 manual save slots, save without quitting, keep saves on death.

## Monsters

| Monster | Glyph | HP | ATK | DEF | Sight | Spawn Weight |
|---------|-------|----|-----|-----|-------|--------------|
| Goblin  | `g`   | 6  | 3   | 0   | 6     | 60%          |
| Orc     | `o`   | 12 | 4   | 1   | 7     | 30%          |
| Troll   | `T`   | 20 | 6   | 3   | 5     | 10%          |

## Adding a New Monster

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

## Project Structure

```
crates/
  core/                     roguelike-core: game logic, zero platform deps
    src/
      lib.rs                Library root — re-exports all modules
      command.rs            GameCommand enum (platform-independent)
      game.rs               GameState struct, step(), observe(), look_at(), core game logic
      look.rs               Look mode: cursor, commands, tile description formatting
      help.rs               Context-sensitive help screen (? key), dynamic from Settings/GameData
      exploration_graph.rs  Room/corridor graph for MCP strategic planning
      map.rs, combat.rs, ai.rs, fov.rs, pathfinding.rs, spawn.rs
      entity.rs, data.rs, types.rs, message_log.rs, message_history.rs
      platform.rs           Renderer and InputSource traits
      spectate.rs           FrameSink trait, NullFrameSink, render_frame()
      seed_code.rs          Shareable seed code encode/decode (base36)
      menu.rs, saves.rs, settings.rs
      dev_tools.rs, analytics.rs, scenario.rs  (dev-tools feature)
      bin/headless.rs       Automated headless runner
    tests/
      golden_replays.rs     Golden replay regression tests
      scenarios.rs          Balance integration tests
      invariants.rs         Property-based game invariant tests (proptest)
      golden_replays/       Stored golden replay JSON files
    benches/
      core_benchmarks.rs    Criterion benchmarks (FOV, pathfinding, step, exploration graph)
  saves/                    roguelike-saves: SaveBackend trait (connected platforms)
    src/
      lib.rs                SaveBackend trait definition (depends only on core)
  tui/                      roguelike-tui: shared terminal game loop + rendering
    src/
      lib.rs                Re-exports all modules
      game_loop.rs          Unified game loop for terminal + SSH (title, playing, paused states)
      render.rs             CrosstermRenderer<W: Write>, render functions, color palettes
      input.rs              Key-to-command translation (game, menu, look mode)
      input_provider.rs     InputProvider trait, InputResult, GameInput
      saves.rs              Re-exports SaveBackend from roguelike-saves
  terminal/                 roguelike-terminal: crossterm frontend
    src/
      main.rs               Terminal game entry point
      render.rs             CrosstermRenderer setup and lifecycle
      input.rs              Crossterm event polling
      terminal_input.rs     InputProvider impl for local terminal (crossterm events)
      local_saves.rs        SaveBackend impl for local filesystem
      dev_hooks.rs          DevHooks impl for debug overlay keys (dev-tools feature)
      gamepad.rs            Gamepad input via gilrs (optional `gamepad` feature)
  ssh/                      roguelike-ssh: SSH server frontend
    src/
      main.rs               SSH server entry point (CLI args, host key, bind)
      server.rs             russh Handler impl, lobby↔session loop
      session.rs            Server menu (Play/Watch/Log Out) + game session
      lobby.rs              dgamelaunch-style lobby (login, register, quit)
      accounts.rs           Account storage with argon2 password hashing
      saves.rs              SaveBackend impl for per-user server directories
      ssh_input.rs          InputProvider impl for SSH channels
      ansi_input.rs         ANSI escape sequence -> KeyEvent parser
      channel_writer.rs     Write impl over SSH channel
  mcp/                      roguelike-mcp: MCP server
    src/
      lib.rs                Library root — re-exports mcp_server and spectate modules
      main.rs               MCP server entry point
      mcp_server.rs         MCP tools for LLM-driven play
      spectate.rs           FileFrameSink (implements FrameSink, ROGUELIKE_SPECTATE_PATH env var)
    tests/
      mcp_integration.rs    Deterministic MCP tool integration tests
      mcp_proptest.rs       Property-based MCP session tests (proptest)
  libudev-sys-dlopen/       Drop-in libudev-sys replacement via dlopen (Linux gamepad)
  c64/                      roguelike-c64: C64 port proof-of-concept (standalone, not in workspace)
    src/
      main.rs, c64.rs       Entry point + C64 hardware abstraction (VIC-II, SID, CIA)
      map.rs, render.rs     40x25 scrolling map, PETSCII rendering
      entity.rs, combat.rs  8-bit entity system, simplified combat
      ai.rs, fov.rs         Monster AI + field of view (no_std)
      input.rs, prng.rs     Keyboard polling, hand-rolled xorshift PRNG
      msglog.rs             Fixed-size message log
tools/
  visualize.py            Python/matplotlib analytics visualizer (batch, sweep, analysis modes)
  balance_diff.py         Balance diff tool — compares stats JSON, outputs markdown verdict (stdlib only)
  llm_playtest.py         Dual-backend LLM playtesting (claude-code CLI or Anthropic API) with parallel execution
  playtest_analytics.py   Shared analytics module for LLM playtesting tools
  spectate.sh             Helper script to watch LLM play in real time (wraps watch + cat)
  requirements.txt        Python dependencies (matplotlib, anthropic)
.github/workflows/
  ci.yml                  CI pipeline — lint, test matrix (4 platforms), benchmarks, cargo audit
  release.yml             Release pipeline — tag-triggered multi-platform builds, GitHub Release with checksums
  balance.yml             CI balance check — runs headless presets, diffs against baseline, posts to PR/summary
```

## Configuration

Game-wide tuning knobs are defined in `crates/core/data/game.toml` (compiled into the binary as defaults). To override any value, place a `game.toml` in the working directory — the terminal, headless runner, and MCP server all load it on startup. In dev-tools builds, press `F10` to hot-reload changes without restarting.

Configurable fields under `[config]`:

| Setting | Default | Description |
|---------|---------|-------------|
| `fov_radius` | 8 | Player's field of view radius |
| `max_rooms` | 30 | Maximum rooms per dungeon |
| `room_size_min` | 4 | Minimum room dimension |
| `room_size_max` | 10 | Maximum room dimension |
| `max_monsters_per_room` | 2 | Monster cap per room |
| `ui_bottom_rows` | 5 | Rows reserved for status bar and log |
| `max_autorun_steps` | 100 | Maximum steps per autorun command |
| `regen_interval` | 3 | Turns between HP regeneration ticks |

## Dependencies

- [crossterm](https://crates.io/crates/crossterm) 0.28 — cross-platform terminal manipulation
- [gilrs](https://crates.io/crates/gilrs) 0.11 — cross-platform gamepad input (optional `gamepad` feature)
- [rusb](https://crates.io/crates/rusb) 0.9 — raw USB HID gamepad access (optional `raw-usb` feature)
- [rand](https://crates.io/crates/rand) 0.8 — random number generation
- [serde](https://crates.io/crates/serde) 1 / [serde_json](https://crates.io/crates/serde_json) 1 — serialization for save/load and game observations
- [toml](https://crates.io/crates/toml) 0.8 — data-driven game configuration (`game.toml`)
- [rmcp](https://crates.io/crates/rmcp) 0.15 — MCP server (official Rust SDK)
- [russh](https://crates.io/crates/russh) 0.49 / [russh-keys](https://crates.io/crates/russh-keys) 0.49 — SSH server for multiplayer
- [argon2](https://crates.io/crates/argon2) 0.5 — Password hashing for SSH account system
- [tokio](https://crates.io/crates/tokio) 1 — async runtime for MCP and SSH servers
- [tracing](https://crates.io/crates/tracing) 0.1 / [tracing-subscriber](https://crates.io/crates/tracing-subscriber) 0.3 — structured logging for MCP and SSH servers
- [criterion](https://crates.io/crates/criterion) 0.5 — benchmarks (dev dependency)
- [proptest](https://crates.io/crates/proptest) 1 — property-based testing (dev dependency)

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for a detailed breakdown with dependencies, effort estimates, and a critical path diagram. See [docs/README.md](docs/README.md) for a full index of design documents, architecture notes, and session reports.

### Foundation (enables networking, platforms & advanced features)
- [x] **Input abstraction** — `GameCommand` enum, decouple game logic from terminal input
- [x] **Platform abstraction** — `Renderer` and `InputSource` traits; game logic never imports platform-specific crates
- [x] **Type aliases** — `Coord`, `Stat` type aliases for coordinates and stats, enabling platform-specific sizing
- [x] **Seeded RNG** — Separate RNG streams per system (map, combat, spawn, loot) for deterministic replay
- [x] **Save/load** — Serialize/deserialize game state via serde

### Core Gameplay
- [ ] **Items** — Potions, scrolls, equipment, inventory system
- [ ] **Hunger** — Food clock mechanic to encourage exploration
- [ ] **Stairs** — Multi-level dungeons with procedural depth
- [ ] **Experience/leveling** — Player progression system
- [ ] **Magic/abilities** — Spells, special attacks, or class-specific powers
- [x] **A\* pathfinding** — Smarter monster AI that navigates around obstacles
- [ ] **Meta-progression** — Persistent unlocks between runs (classes, upgrades, achievements)

### UI/UX
- [x] **Controller support** — Gamepad input via gilrs on all desktop platforms; d-pad/stick for 8-directional movement, LB autorun modifier, context-sensitive button mapping for menus/look/history
- [ ] **Steam Deck + Steam Cloud** — Verified controller layout, Steam Input API, Steam Auto-Cloud save sync (coexists with AT Protocol PDS saves)
- [ ] **Menus** — Title screen and pause menu (done); inventory screen, character sheet, help screen still needed
- [x] **Look mode** — Cursor to examine tiles and entities
- [ ] **Targeting** — Ranged attacks, spell targeting
- [ ] **Options/settings** — Classic/casual mode, display preferences, and colorblind palettes (done); keybind customization and difficulty modes still needed

### Accessibility

- [ ] **Visual** — ~~Colorblind palettes~~ (done: protanopia, deuteranopia), ~~high-contrast mode~~ (done), configurable glyphs, reduced clutter option
- [ ] **Screen reader support** — Structured output for NVDA/JAWS/VoiceOver; braille display compatible
- [ ] **Motor** — ~~One-handed layouts~~ (done: left-hand QWEASDZXC/WEASDZXCR), mouse-only play, adjustable input timing, ~~auto-explore~~ (done), macros
- [ ] **Cognitive** — Granular difficulty toggles, ~~scrollable message history~~ (done), ~~context-sensitive help~~ (done: `?` key)
- [x] **Character identity** — Player-chosen name and pronouns in Settings, shown in save slots and death screen
- [x] **Code of Conduct** — See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)

Design principles (not checkboxes — follow these always):
- Same input always does the same thing; current mode always visible
- Every sound cue must have a text/visual equivalent; no game-critical info through sound alone

### Networking
- [x] **Replay system** — Record and playback games deterministically
- [ ] **Shared leaderboard** — REST API for score submission
- [ ] **Daily challenges** — Everyone plays the same seed, compare scores
- [x] **Seed sharing** — Share a seed code from the death screen or observations; enter it from the title menu to replay the same dungeon
- [ ] **Live spectating** — Watch other players in real-time via WebSocket
- [ ] **Bones files** — Dead players leave traces in others' dungeons

### Platform Support
- [x] **Windows / macOS / Linux** — Native terminal via crossterm (current)
- [x] **CI matrix** — Tests on Linux (x86_64 + ARM64), macOS (ARM64), Windows (x86_64) in GitHub Actions
- [ ] **Web (WASM)** — Browser-based play via wasm-pack + xterm.js; enables browser spectating and leaderboards
- [ ] **Game Boy Advance** — Native ARM via `thumbv4t-none-eabi` target + `gba` crate; no_std, fixed-size containers
- [ ] **PS Vita** — Native ARM via vita-sdk + vitasdk-sys; hardware buttons, OLED display, memory card saves
- [x] **SSH server** — Server-side play via russh, NetHack-server style (players connect via SSH); dgamelaunch-style lobby with account registration/login, per-user persistent saves
- [ ] **Commodore 64** — Native 6502 via rust-mos; no_std, fixed-size containers, 8-bit types, C64 screen/keyboard I/O (POC validated in `crates/c64/`; [proposal](docs/c64-port-proposal.md), [technical reference](docs/c64-technical-reference.md))

### Modding
- [x] **Data-driven content** — Game balance data in TOML; drop a `game.toml` in the working directory to override player stats, config knobs, and monster definitions without recompiling
- [x] **Hot reload** — Press F10 (`dev-tools` build) to reload `game.toml` at runtime; diffs changes and applies updated config, FOV radius, and monster stats to the live game
- [ ] **Scripting** — Embedded Lua or Rhai for custom AI, quests, and event handlers (future)

### Developer Tools
- [x] **Debug console** — In-game console (`dev-tools` feature): teleport, god mode, FOV toggle, spawn monsters, stat editing, replay export
- [x] **Headless runner** — Automated playtesting binary with JSON stats output: run N games, configurable seeds/presets, replay support, analytics, parameter sweeps, golden replay management
- [x] **Balance telemetry** — Per-game combat analytics via snapshot/diff, aggregate statistics, per-monster-type damage/kill tracking, difficulty metrics
- [x] **Scenario framework** — Fluent builder API for composing specific game states and asserting balance outcomes
- [x] **Golden replay regression** — Stored deterministic replays with expected results; detects unintended gameplay changes
- [x] **Parameter sweeps** — Sweep across player stats (HP, ATK, DEF) to find balance boundaries; JSON config, structured output
- [x] **LLM playtesting** — Strategic LLM-driven playtesting via `/playtest` skill and `tools/llm_playtest.py`; dual backends (Claude Code CLI + Anthropic API), parallel execution, contextual strategy prompt with combat math, token usage tracking, compact mode for cost optimization
- [x] **Debug overlay** — Visualize FOV boundaries, monster AI targets, A\* pathfinding routes, exploration frontiers, hidden monsters, and per-monster FOV boundaries as colored overlays; toggle layers with F6–F9/F11–F12, cursor mode for interactive pathfinding and single-monster FOV inspection
- [x] **CI balance check** — GitHub Actions workflow runs headless presets on every gameplay change, diffs against cached baseline, posts verdict (STABLE/MINOR SHIFT/BALANCE SHIFT) to workflow summary and PR comments
- [x] **MCP spectator mode** — File-based spectator for watching LLM play; set `ROGUELIKE_SPECTATE_PATH` env var to write ASCII frames atomically after every MCP action. [Design doc.](docs/design/spectator-mode.md)
- [ ] **Map editor** — Visual tool for designing and testing dungeon layouts

### Polish
- [ ] **Animation effects** — Attack swooshes, spell particles (terminal-rendered)
- [ ] **Sound effects** — Audio via rodio (MP3/WAV/OGG), background threads, SoundEvent enum
- [ ] **Music** — Atmospheric dungeon tracks, boss themes, adaptive based on depth/danger
- [ ] **Tileset support** — Alternative to ASCII (graphical tiles)
- [ ] **Localization (i18n)** — Externalized strings, multi-language support
- [ ] **Tutorial/guided run** — Introduce mechanics gradually for new players

## David's Statement on AI Use

This project is largely vibecoded with architectural and design decisions made by me (David).

Once the AI boom began, I initially found it quite interesting. Quickly I became very skeptical as I saw how it was affecting schools, artists, the market, etc. I consider it very likely that there is an AI bubble that will burst at some point. I also think LLMs will still be around after it does.

Internally, I have a bias to reject it. However, it is a value of mine to not auto-reject new technology, remain generally open-minded, and be willing to change my mind. This project is me exploring the capabilities of AI firsthand in order to better understand its current state and its potential future role in society.

I have the utmost respect for the talented developers who handcode their applications and games - this project is not on the same playing field as those developers' projects and I do not claim to have the skills they do.

This is first and foremost an experiment and secondly a way for me to have fun. I do not support the ensloppification of the internet nor the use of sub-standard AI art in lieu of hiring real artists.

To be clear:
- AI was used for code generation, architecture planning, and documentation (except this section)
- All design decisions are mine

## License

GPL-3.0-or-later
