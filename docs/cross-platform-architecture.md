# Cross-Platform Architecture

How the codebase is structured for multiple platform targets (terminal, GBA, Vita, web, etc.) without maintaining separate branches.

## Current State

The codebase is split into a Cargo workspace with three crates:

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
│   ├── terminal/           roguelike-terminal: crossterm frontend
│   │   ├── Cargo.toml      depends on core + gilrs (optional)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── render.rs   (CrosstermRenderer)
│   │       ├── input.rs    (crossterm key translation)
│   │       └── gamepad.rs  (gilrs gamepad input, optional `gamepad` feature)
│   ├── mcp/                roguelike-mcp: MCP server
│   │   ├── Cargo.toml      depends on core + rmcp + tokio
│   │   └── src/
│   │       ├── main.rs
│   │       └── mcp_server.rs
│   └── gba/                (future: GBA frontend)
│   └── vita/               (future: PS Vita frontend)
```

Type aliases in `crates/core/src/types.rs` centralize platform-sensitive sizing:

```rust
pub type Coord = i32;  // position/dimension in tile units
pub type Pos = (Coord, Coord);  // (x, y) tile position
pub type Stat = i32;   // character stat (HP, ATK, DEF, damage)
```

Only the `terminal` crate imports `crossterm`. Only the `mcp` crate imports `rmcp`/`tokio`. The `core` crate has zero platform dependencies.

### Completed prerequisites

- [x] **Platform abstraction** — `Renderer` and `InputSource` traits in `core/src/platform.rs`
- [x] **Abstract Color** — `GameColor` enum in `core/src/types.rs`; crossterm removed from `entity.rs` and `data.rs`
- [x] **Move `GameCommand`** — `command.rs` in core; terminal's `input.rs` only does key translation
- [x] **Workspace split** — three crates: core, terminal, mcp

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

- **Terminal crate**: crossterm rendering, keyboard input, gamepad input (gilrs, optional), terminal lifecycle
- **MCP crate**: rmcp server, tokio runtime, JSON serialization of game state
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
