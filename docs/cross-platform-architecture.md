# Cross-Platform Architecture

How to structure the codebase for multiple platform targets (terminal, GBA, web, etc.) without maintaining separate branches.

## Current State

The codebase has type aliases in `src/types.rs` that centralize platform-sensitive sizing:

```rust
pub type Coord = i32;  // position/dimension in tile units
pub type Pos = (Coord, Coord);  // (x, y) tile position
pub type Stat = i32;   // character stat (HP, ATK, DEF, damage)
```

Five source files import `crossterm` (the terminal library). Everything else is pure game logic with no platform dependencies.

| File | Platform dependency | Role |
|------|-------------------|------|
| `entity.rs` | `crossterm::style::Color` | Entity data (Color only) |
| `data.rs` | `crossterm::style::Color` | Monster templates (Color only) |
| `input.rs` | `crossterm::event::*` | Keyboard → GameCommand translation |
| `render.rs` | `crossterm::*` | Terminal rendering |
| `main.rs` | `crossterm::*` | Terminal lifecycle |

## Why Not Branches

Maintaining a separate branch per platform (e.g. `gba`, `web`) creates constant merge conflicts:

- `types.rs` differs on every branch (different type sizes)
- Any game logic change must be merged to every port branch
- Platform-specific fixes can't be tested against other platforms in CI
- Drift accumulates — ports fall behind main

## Architecture: Workspace + Feature Flags

### Phase 1: Feature flags (minimal change)

Add feature flags to `Cargo.toml` for type sizing without restructuring:

```toml
[features]
default = ["terminal"]
terminal = ["dep:crossterm"]
gba = []
```

Then in `types.rs`:

```rust
#[cfg(feature = "gba")]
pub type Coord = i16;

#[cfg(not(feature = "gba"))]
pub type Coord = i32;
```

Same branch, same code. `cargo build --features gba` compiles with `i16` coordinates. CI tests all feature combinations.

### Phase 2: Workspace split (when adding a second frontend)

Split the single crate into a workspace with a shared core:

```
roguelike/
├── Cargo.toml              (workspace root)
├── crates/
│   ├── core/               (game logic, zero platform deps)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs    ← Coord, Stat, Pos + feature flags
│   │       ├── game.rs
│   │       ├── map.rs
│   │       ├── combat.rs
│   │       ├── ai.rs
│   │       ├── fov.rs
│   │       ├── pathfinding.rs
│   │       ├── spawn.rs
│   │       ├── entity.rs   ← Color abstracted (see below)
│   │       ├── data.rs     ← Color abstracted
│   │       └── message_log.rs
│   ├── terminal/           (crossterm frontend)
│   │   ├── Cargo.toml      depends on core
│   │   └── src/
│   │       ├── main.rs     ← current main.rs
│   │       ├── render.rs   ← current render.rs
│   │       └── input.rs    ← keyboard portion of current input.rs
│   ├── mcp/                (MCP server frontend)
│   │   ├── Cargo.toml      depends on core + rmcp + tokio
│   │   └── src/
│   │       ├── main.rs     ← current bin/mcp_server.rs
│   │       └── mcp.rs      ← current mcp.rs
│   └── gba/                (GBA frontend, future)
│       ├── Cargo.toml      depends on core with features = ["gba"]
│       └── src/
│           ├── main.rs
│           ├── render.rs   (GBA tile renderer)
│           └── input.rs    (GBA button mapping)
```

### What stays in core

Everything that doesn't touch a platform API:

- Game state, turns, commands (`game.rs`, `GameCommand` enum from `input.rs`)
- Map generation, rooms, tiles (`map.rs`)
- Combat, AI, FOV, pathfinding, spawning
- Entity data, monster templates, game config
- Message log
- Type aliases

### What moves to frontend crates

Anything that talks to hardware or external services:

- **Terminal crate**: crossterm rendering, keyboard input, terminal lifecycle
- **MCP crate**: rmcp server, tokio runtime, JSON serialization of game state
- **GBA crate**: GBA tile/sprite rendering, button input, no-std setup

## Abstracting Color

The one snag is `crossterm::style::Color` used in `entity.rs` and `data.rs`. These are core data files that shouldn't depend on crossterm. Two options:

### Option A: Core-defined color enum (recommended)

```rust
// core/src/types.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameColor {
    Yellow,
    Green,
    DarkGreen,
    DarkRed,
    // add as needed
}
```

Each frontend maps `GameColor` to its platform color type. Simple, no dependencies.

### Option B: RGB tuple

```rust
// core/src/types.rs
pub type Color = (u8, u8, u8);
```

More flexible but loses named colors. GBA has a limited palette, so named colors with per-platform mapping (Option A) is usually better.

## Type Sizing by Platform

Expected type sizes per target:

| Type | Terminal / Web | GBA | C64 (hypothetical) |
|------|---------------|-----|---------------------|
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

## Prerequisites

Before the workspace split, complete these roadmap items:

1. **Platform abstraction** — define `Renderer` and `InputSource` traits in core
2. **Abstract Color** — remove `crossterm::style::Color` from `entity.rs` and `data.rs`
3. **Move `GameCommand`** — the enum is already platform-agnostic; ensure `input.rs` only re-exports it and keeps keyboard translation separate

The type aliases (`Coord`, `Stat`, `Pos`) are already in place. The `crossterm` imports identify exactly where to cut.
