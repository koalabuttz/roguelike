# Roguelike

A terminal-based roguelike dungeon crawler written in Rust.

Explore randomly generated dungeons, fight monsters, and try to survive. Renders directly in the terminal using ASCII characters and ANSI colors — no external game engine required.

## Building and Running

Requires [Rust](https://www.rust-lang.org/tools/install) **1.85.0 or later** (for edition 2024 support).

```sh
cargo run
```

The game adapts to your terminal size automatically.

## Testing

```sh
cargo test                       # All tests: 304 unit + 13 integration
cargo test --lib                 # 304 unit tests across 17 modules
cargo test --test golden_replays # 5 golden replay regression tests
cargo test --test scenarios      # 8 balance integration tests
cargo clippy -- -D warnings
cargo fmt --check
```

### Test Categories

| Category | Command | What it checks |
|----------|---------|----------------|
| Unit tests | `cargo test --lib` | Module-level logic across all 17 modules |
| Golden replays | `cargo test --test golden_replays` | Deterministic replay regression — detects unintended gameplay changes |
| Scenario tests | `cargo test --test scenarios` | Balance properties — e.g., "player survives 2 goblins", "troll kills weak player" |

### Golden Replays

Golden replays are stored game recordings with their expected outcomes (`tests/golden_replays/*.json`). After any code change, re-running them detects if game behavior has diverged. If a change is intentional (e.g., rebalancing monster stats), regenerate the goldens:

```sh
cargo run --bin headless -- --regenerate-goldens tests/golden_replays/
```

To add a new golden replay:

```sh
cargo run --bin headless -- --save-golden tests/golden_replays/seed_99_arena.json --seed 99 --preset arena
```

### Scenario Tests

Scenario tests use a fluent builder API to compose specific game states and assert outcomes. They live in `tests/scenarios.rs` and are the recommended way to test balance changes:

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
| Wait a turn | `.` or numpad `5` |
| Quit | `q`, `Esc`, or `Ctrl+C` |

Vi keys and numpad are opt-in via Settings. Moving into a monster attacks it. Autorun keeps moving until hitting a wall, spotting a monster, or reaching a junction.

## MCP Server (AI Play)

An LLM agent (like Claude) can play the game through the [Model Context Protocol](https://modelcontextprotocol.io/) server.

```sh
cargo run --bin mcp_server
```

The server communicates over stdio and exposes these tools:

| Tool | Description |
|------|-------------|
| `new_game` | Start a game (optional `width`/`height`/`seed`/`compact` params) |
| `observe` | Get visible state: map, entities, HP, messages |
| `act` | Take an action: move, wait, autorun, or auto\_fight |
| `pathfind_to` | A\* pathfind to a target tile; stops for monsters or damage |
| `auto_explore` | Find nearest frontier and walk to it in one call |
| `get_explored_map` | Full explored map with frontier markers (`~`) |
| `save_game` | Save current state to an in-memory slot |
| `load_game` | Restore a previously saved game state |
| `get_rules` | Read game mechanics and strategy tips |

To use with Claude Desktop, add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "roguelike": {
      "command": "cargo",
      "args": ["run", "--bin", "mcp_server", "--manifest-path", "/path/to/roguelike/Cargo.toml"]
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
cargo run --bin headless -- --save-golden tests/golden_replays/my_test.json --seed 42

# Regenerate all golden replays after an intentional gameplay change:
cargo run --bin headless -- --regenerate-goldens tests/golden_replays/

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
- Monsters wake and chase you when they enter your field of view.
- `%` marks a corpse. Dead monsters stay on the map.
- The HP bar and message log are at the bottom of the screen.
- **Title screen** lets you start a new game, load a save, or adjust settings.
- **Classic mode** (default): NetHack-style save discipline — saving quits, death deletes the save.
- **Casual mode**: 5 manual save slots, save without quitting, keep saves on death.

## Monsters

| Monster | Glyph | HP | ATK | DEF | Spawn Weight |
|---------|-------|----|-----|-----|--------------|
| Goblin  | `g`   | 6  | 3   | 0   | 60%          |
| Orc     | `o`   | 12 | 4   | 1   | 30%          |
| Troll   | `T`   | 20 | 6   | 3   | 10%          |

## Adding a New Monster

All content lives in `src/data.rs`. To add a monster:

1. Define a template constant:

```rust
pub const DRAGON: MonsterTemplate = MonsterTemplate {
    name: "Dragon",
    glyph: 'D',
    color: GameColor::Red,
    hp: 40,
    attack: 10,
    defense: 5,
    ai: AiBehavior::Chase,
};
```

2. Add it to the spawn table:

```rust
SpawnEntry { template: &DRAGON, weight: 5 },
```

If it needs new AI, add a variant to `AiBehavior` in `src/entity.rs` and implement it in `src/ai.rs`.

## Project Structure

```
src/
  lib.rs           Library root — re-exports all modules
  main.rs          Terminal game entry point
  input.rs         GameCommand enum, key-to-command translation
  game.rs          GameState struct, step(), observe(), core game logic
  data.rs          Monster templates, spawn table, config constants
  entity.rs        Entity struct, EntityKind, AiBehavior enum
  ai.rs            Monster AI system (dispatches on AiBehavior)
  combat.rs        Melee attack resolution
  spawn.rs         Monster spawning using weighted tables
  map.rs           Map struct and dungeon generation
  fov.rs           Field of view (recursive shadowcasting)
  pathfinding.rs   A* pathfinding for monsters and MCP navigation
  platform.rs      Renderer and InputSource traits (platform abstraction)
  render.rs        Terminal rendering (implements Renderer)
  menu.rs          Title screen, pause menu, settings UI
  saves.rs         Save slot metadata for menu display
  settings.rs      Platform-aware settings (casual mode, display options)
  dev_tools.rs     Debug console, map presets, replay export, golden replays (dev-tools feature)
  analytics.rs     Combat analytics, snapshot/diff tracking, parameter sweeps (dev-tools feature)
  scenario.rs      Fluent scenario builder for balance testing (dev-tools feature)
  message_log.rs   Message log
  types.rs         Type aliases (Coord, Stat, Pos) and GameColor enum
  mcp.rs           MCP server — tools for LLM-driven play
  bin/
    mcp_server.rs  MCP server binary entry point
    headless.rs    Automated headless runner with analytics, sweeps, goldens (dev-tools feature)
tests/
  golden_replays.rs       Integration tests for golden replay verification
  scenarios.rs            Balance integration tests using the scenario framework
  golden_replays/         Stored golden replay JSON files (committed to repo)
tools/
  visualize.py            Python/matplotlib analytics visualizer (batch, sweep, analysis modes)
  llm_playtest.py         Dual-backend LLM playtesting (claude-code CLI or Anthropic API) with parallel execution
  playtest_analytics.py   Shared analytics module for LLM playtesting tools
  requirements.txt        Python dependencies (matplotlib, anthropic)
```

## Configuration

Game-wide tuning knobs are in `src/data.rs` under `GameConfig`:

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
- [rand](https://crates.io/crates/rand) 0.8 — random number generation
- [serde](https://crates.io/crates/serde) 1 / [serde_json](https://crates.io/crates/serde_json) 1 — serialization for save/load and game observations
- [rmcp](https://crates.io/crates/rmcp) 0.15 — MCP server (official Rust SDK)
- [tokio](https://crates.io/crates/tokio) 1 — async runtime for MCP server
- [tracing](https://crates.io/crates/tracing) 0.1 / [tracing-subscriber](https://crates.io/crates/tracing-subscriber) 0.3 — structured logging for MCP server

## Roadmap

See [docs/roadmap-priority.md](docs/roadmap-priority.md) for a detailed breakdown with dependencies, effort estimates, and a critical path diagram.

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
- [ ] **Controller support** — Gamepad input on all desktop platforms; d-pad/stick for movement, context-sensitive menus for actions
- [ ] **Steam Deck** — Verified controller layout, Steam Input API integration
- [ ] **Menus** — Title screen and pause menu (done); inventory screen, character sheet, help screen still needed
- [ ] **Look mode** — Cursor to examine tiles and entities
- [ ] **Targeting** — Ranged attacks, spell targeting
- [ ] **Options/settings** — Classic/casual mode and display preferences (done); keybind customization and difficulty modes still needed

### Accessibility

- [ ] **Visual** — Colorblind palettes, high-contrast mode, configurable glyphs, reduced clutter option
- [ ] **Screen reader support** — Structured output for NVDA/JAWS/VoiceOver; braille display compatible
- [ ] **Motor** — One-handed layouts, mouse-only play, adjustable input timing, auto-explore (done), macros
- [ ] **Cognitive** — Granular difficulty toggles, scrollable message history, context-sensitive help (`?`)
- [ ] **Character identity** — Player-chosen name and pronouns used in game text
- [x] **Code of Conduct** — See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)

Design principles (not checkboxes — follow these always):
- Same input always does the same thing; current mode always visible
- Every sound cue must have a text/visual equivalent; no game-critical info through sound alone

### Networking
- [x] **Replay system** — Record and playback games deterministically
- [ ] **Shared leaderboard** — REST API for score submission
- [ ] **Daily challenges** — Everyone plays the same seed, compare scores
- [ ] **Seed sharing** — Share a seed as a code/URL, others play the same dungeon
- [ ] **Live spectating** — Watch other players in real-time via WebSocket
- [ ] **Bones files** — Dead players leave traces in others' dungeons

### Platform Support
- [x] **Windows / macOS / Linux** — Native terminal via crossterm (current)
- [x] **CI matrix** — Tests on Linux (x86_64 + ARM64), macOS (ARM64), Windows (x86_64) in GitHub Actions
- [ ] **Web (WASM)** — Browser-based play via wasm-pack + xterm.js; enables browser spectating and leaderboards
- [ ] **Game Boy Advance** — Native ARM via `thumbv4t-none-eabi` target + `gba` crate; no_std, fixed-size containers
- [ ] **SSH server** — Server-side play via russh, NetHack-server style (players connect via SSH)
- [ ] **Commodore 64** — Native 6502 via rust-mos; no_std, fixed-size containers, 8-bit types, C64 screen/keyboard I/O

### Modding
- [ ] **Data-driven content** — Move templates to external files (RON/TOML); add monsters/items without recompiling
- [ ] **Hot reload** — Reload data files during development without restarting
- [ ] **Scripting** — Embedded Lua or Rhai for custom AI, quests, and event handlers (future)

### Developer Tools
- [x] **Debug console** — In-game console (`dev-tools` feature): teleport, god mode, FOV toggle, spawn monsters, stat editing, replay export
- [x] **Headless runner** — Automated playtesting binary with JSON stats output: run N games, configurable seeds/presets, replay support, analytics, parameter sweeps, golden replay management
- [x] **Balance telemetry** — Per-game combat analytics via snapshot/diff, aggregate statistics, per-monster-type damage/kill tracking, difficulty metrics
- [x] **Scenario framework** — Fluent builder API for composing specific game states and asserting balance outcomes
- [x] **Golden replay regression** — Stored deterministic replays with expected results; detects unintended gameplay changes
- [x] **Parameter sweeps** — Sweep across player stats (HP, ATK, DEF) to find balance boundaries; JSON config, structured output
- [x] **LLM playtesting** — Strategic LLM-driven playtesting via `/playtest` skill and `tools/llm_playtest.py`; dual backends (Claude Code CLI + Anthropic API), parallel execution, contextual strategy prompt with combat math, token usage tracking, compact mode for cost optimization
- [ ] **Debug overlay** — Visualize AI state, FOV boundaries, pathfinding routes
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
