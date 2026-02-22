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

## Gameplay

- `@` is you. Monsters appear as colored letters (`g`oblin, `o`rc, `T`roll).
- Monsters have their own sight range and chase you when *they* see you (not when you see them). Trolls are dim-sighted and easy to sneak past; goblins are alert scouts.
- `%` marks a corpse. Dead monsters stay on the map.
- The HP bar and message log are at the bottom of the screen.
- **Title screen** lets you start a new game, enter a seed code, load a save, or adjust settings.
- **Seed codes** are shown on the death screen and in MCP observations. Enter one from the title menu to replay the exact same dungeon. Format: `<base36_seed>[-<W>x<H>][preset_char]` — e.g., `r7z3kq`, `r7z3kq-120x60a`.
- **Classic mode** (default): NetHack-style save discipline — saving quits, death deletes the save.
- **Casual mode**: 5 manual save slots, save without quitting, keep saves on death.

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

## Monsters

| Monster | Glyph | HP | ATK | DEF | Sight | Spawn Weight |
|---------|-------|----|-----|-----|-------|--------------|
| Goblin  | `g`   | 6  | 3   | 0   | 6     | 60%          |
| Orc     | `o`   | 12 | 4   | 1   | 7     | 30%          |
| Troll   | `T`   | 20 | 6   | 3   | 5     | 10%          |

Monsters are data-driven — add a `[[monsters]]` entry to `game.toml` to define new monsters without recompiling. See [CONTRIBUTING.md](CONTRIBUTING.md#adding-a-monster) for details.

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
```

Frames are written atomically after every action. See the [spectator mode design doc](docs/design/spectator-mode.md) for implementation details.

## Development

```sh
cargo test --workspace               # All ~600 tests across all crates
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for test categories, scenario/golden replay workflows, CI pipelines, debug keys, and coding guidelines.

### Developer Tools

- **[Headless runner](docs/tooling/headless-runner.md)** — Automated playtesting: run N games with configurable seeds/presets, parameter sweeps, analytics, HTML reports, and golden replay management
- **[LLM playtesting](docs/tooling/llm-playtesting.md)** — Strategic LLM-driven playtesting via `/playtest` skill and `tools/llm_playtest.py`; dual backends, parallel execution, token optimization
- **Debug overlays** — Visualize FOV boundaries, monster AI targets, A\* pathfinding, exploration frontiers, and per-monster FOV (F6–F12 in dev-tools builds)
- **CI balance check** — GitHub Actions workflow diffs gameplay changes against baseline, posts verdict to PR comments

## Project Structure

```
crates/
  core/       roguelike-core: game logic, zero platform deps
  saves/      roguelike-saves: SaveBackend trait (connected platforms)
  tui/        roguelike-tui: shared terminal game loop + rendering
  terminal/   roguelike-terminal: crossterm frontend (local play)
  ssh/        roguelike-ssh: SSH server frontend (multiplayer)
  mcp/        roguelike-mcp: MCP server (AI play)
  c64/        roguelike-c64: C64 port proof-of-concept (standalone)
  libudev-sys-dlopen/  Drop-in libudev-sys replacement via dlopen
tools/        Python analytics, visualization, and playtesting scripts
```

See [docs/architecture/cross-platform.md](docs/architecture/cross-platform.md) for detailed crate responsibilities and the platform abstraction design.

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for the full breakdown with dependencies, effort estimates, and critical path. See [docs/README.md](docs/README.md) for all design documents and session reports.

**Next up:**
- Items, inventory, and equipment
- Multi-level dungeons (stairs)
- Experience and leveling
- Web (WASM) frontend
- Daily challenges and shared leaderboard

**Completed highlights:** Platform abstraction, save/load, A\* pathfinding, gamepad support, SSH multiplayer, MCP server for AI play, data-driven content with hot reload, full CI/CD with balance regression testing, seed sharing.

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
