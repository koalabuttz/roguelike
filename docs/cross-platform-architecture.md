# Cross-Platform Architecture

How the codebase is structured for multiple platform targets (terminal, SSH, web, GBA, Vita, etc.) without maintaining separate branches.

## Current State

The codebase is split into a Cargo workspace with five crates:

```
roguelike/
├── Cargo.toml              (workspace root)
├── crates/
│   ├── core/               roguelike-core: game logic, zero platform deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── command.rs  ← GameCommand enum (platform-independent)
│   │       ├── types.rs    ← Coord, Stat, Pos + GameColor
│   │       ├── game.rs, map.rs, combat.rs, ai.rs, fov.rs
│   │       ├── pathfinding.rs, spawn.rs, entity.rs, data.rs
│   │       ├── platform.rs ← Renderer, InputSource traits
│   │       ├── menu.rs, saves.rs, settings.rs
│   │       ├── dev_tools.rs, analytics.rs, scenario.rs
│   │       └── message_log.rs
│   ├── tui/                roguelike-tui: shared terminal rendering + game loop
│   │   ├── Cargo.toml      depends on core + crossterm
│   │   └── src/
│   │       ├── game_loop.rs (unified game loop for terminal + SSH)
│   │       ├── render.rs    (CrosstermRenderer, color palette mapping)
│   │       ├── input.rs     (key-to-command translation)
│   │       ├── input_provider.rs (InputProvider trait, InputResult, GameInput)
│   │       └── saves.rs     (SaveBackend trait)
│   ├── terminal/           roguelike-terminal: local terminal frontend
│   │   ├── Cargo.toml      depends on core + tui + gilrs (optional)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── render.rs   (CrosstermRenderer setup)
│   │       ├── input.rs    (crossterm event polling)
│   │       ├── terminal_input.rs (InputProvider impl for local terminal)
│   │       ├── local_saves.rs    (SaveBackend impl for local filesystem)
│   │       └── gamepad.rs  (gilrs gamepad input, optional `gamepad` feature)
│   ├── ssh/                roguelike-ssh: SSH server frontend
│   │   ├── Cargo.toml      depends on core + tui + russh + argon2
│   │   └── src/
│   │       ├── main.rs      (SSH + server startup)
│   │       ├── server.rs    (russh Handler, per-connection state)
│   │       ├── lobby.rs     (dgamelaunch-style login/register TUI)
│   │       ├── accounts.rs  (argon2 password hashing, JSON account files)
│   │       ├── session.rs   (game session for logged-in user)
│   │       ├── ssh_input.rs (InputProvider impl for SSH channels)
│   │       ├── saves.rs     (SaveBackend impl for per-user directories)
│   │       ├── ansi_input.rs (raw byte → KeyEvent parser)
│   │       └── channel_writer.rs (async SSH channel → sync Write adapter)
│   ├── mcp/                roguelike-mcp: MCP server for LLM play
│   │   ├── Cargo.toml      depends on core + rmcp + tokio
│   │   └── src/
│   │       ├── lib.rs       (re-exports mcp_server and spectate modules)
│   │       ├── main.rs
│   │       ├── mcp_server.rs
│   │       └── spectate.rs  (file-based spectator, ROGUELIKE_SPECTATE_PATH)
│   ├── atproto/            (future: AT Protocol identity + PDS save storage)
│   ├── web/                (future: WASM browser frontend)
│   └── gba/                (future: GBA frontend)
│   └── vita/               (future: PS Vita frontend)
```

The `tui` crate sits between `core` and the terminal-based frontends (`terminal`, `ssh`). It provides the shared game loop, crossterm-based rendering, and the `InputProvider` / `SaveBackend` traits. Both `terminal` and `ssh` implement these traits for their respective I/O mechanisms.

Type aliases in `crates/core/src/types.rs` centralize platform-sensitive sizing:

```rust
pub type Coord = i32;  // position/dimension in tile units
pub type Pos = (Coord, Coord);  // (x, y) tile position
pub type Stat = i32;   // character stat (HP, ATK, DEF, damage)
```

Only `tui`, `terminal`, and `ssh` import `crossterm`. Only the `mcp` crate imports `rmcp`/`tokio`. The `core` crate has zero platform dependencies.

### Completed prerequisites

- [x] **Platform abstraction** — `Renderer` and `InputSource` traits in `core/src/platform.rs`
- [x] **Abstract Color** — `GameColor` enum in `core/src/types.rs`; crossterm removed from `entity.rs` and `data.rs`
- [x] **Move `GameCommand`** — `command.rs` in core; terminal's `input.rs` only does key translation
- [x] **Workspace split** — five crates: core, tui, terminal, ssh, mcp
- [x] **Shared game loop** — `tui/src/game_loop.rs` is the single game loop for both terminal and SSH frontends
- [x] **SSH server** — `ssh` crate with russh, lobby system, per-user accounts and saves

### Pending prerequisites (for web/atproto)

- [ ] **Extract `SaveBackend` to core** — Currently in `tui/src/saves.rs` which depends on crossterm. The trait itself has no crossterm dependency and should move to `core/src/saves.rs` so that `crates/atproto` and `crates/web` can implement it without pulling in crossterm. See [atproto design doc](design/atproto.md#prerequisite-extract-savebackend-from-roguelike-tui).
- [ ] **AT Protocol integration** — `atproto` crate for Bluesky OAuth login and PDS-based portable saves. See [design doc](design/atproto.md).
- [ ] **WASM frontend** — `web` crate with CanvasRenderer, Web Worker game loop, JS-bridged saves. See [design doc](design/atproto.md#wasm-frontend).

## Why Not Branches

Maintaining a separate branch per platform (e.g. `gba`, `web`) creates constant merge conflicts:

- `types.rs` differs on every branch (different type sizes)
- Any game logic change must be merged to every port branch
- Platform-specific fixes can't be tested against other platforms in CI
- Drift accumulates — ports fall behind main

## Feature Flags for Type Sizing

For platforms that need different type sizes (GBA, C64), add feature flags to the core crate:

```toml
[features]
gba = []
```

Then in `types.rs`:

```rust
#[cfg(feature = "gba")]
pub type Coord = i16;

#[cfg(not(feature = "gba"))]
pub type Coord = i32;
```

Same branch, same code. The frontend crate selects the feature via `Cargo.toml`. CI tests all feature combinations.

### What stays in core

Everything that doesn't touch a platform API:

- Game state, turns, commands (`game.rs`, `GameCommand` enum from `command.rs`)
- Map generation, rooms, tiles (`map.rs`)
- Combat, AI, FOV, pathfinding, spawning
- Entity data, monster templates, game config
- Message log, menus, settings, saves
- Type aliases and `GameColor`

### What moves to frontend crates

Anything that talks to hardware or external services:

- **TUI crate**: crossterm rendering, shared game loop, `InputProvider` / `SaveBackend` traits, key-to-command translation
- **Terminal crate**: local crossterm event polling, local filesystem saves, gamepad input (gilrs, optional), terminal lifecycle
- **SSH crate**: russh server, lobby/accounts system, ANSI input parsing, per-user saves, SSH channel I/O
- **MCP crate**: rmcp server, tokio runtime, JSON serialization of game state
- **Atproto crate** (future): AT Protocol OAuth, handle resolution, PDS save backend, XRPC client. See [design doc](design/atproto.md).
- **Web crate** (future): WASM entry point, canvas rendering, Web Worker input, JS interop for OAuth and PDS saves
- **GBA crate** (future): GBA tile/sprite rendering, button input, no-std setup
- **Vita crate** (future): vita-sdk rendering, hardware buttons, memory card saves

## Color Abstraction

Colors use a core-defined `GameColor` enum (Option A from the original design):

```rust
// core/src/types.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameColor {
    Yellow,
    Green,
    DarkGreen,
    DarkRed,
    Rgb(u8, u8, u8),
    // ...
}
```

Each frontend maps `GameColor` to its platform color type. The terminal crate's `render.rs` has `to_crossterm_color()` for this.

## Type Sizing by Platform

Expected type sizes per target:

| Type | Terminal / Web / Vita | GBA | C64 (hypothetical) |
|------|----------------------|-----|---------------------|
| `Coord` | `i32` | `i16` | `i8` |
| `Stat` | `i32` | `i8` | `i8` |
| `Pos` | `(i32, i32)` | `(i16, i16)` | `(i8, i8)` |

Remaining `i32` values (turn counts, kill counts, pathfinding costs, etc.) would be sized per-field during the actual port based on their value ranges. These don't share a uniform sizing requirement, so they don't benefit from a blanket alias.

## Workflow

All development happens on one branch:

1. **Game logic** changes go in `core/` — automatically available to all frontends
2. **New platform?** Add a new crate under `crates/`, implement rendering and input
3. **Type sizing?** Feature flag on `core`'s `types.rs` — the frontend crate selects it via `Cargo.toml`
4. **CI** builds all frontends in a matrix — catches cross-platform breakage immediately
