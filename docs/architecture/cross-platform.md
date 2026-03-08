# Cross-Platform Architecture

How the codebase is structured for multiple platform targets (terminal, SSH, web, GBA, Vita, etc.) without maintaining separate branches.

## Why Not Branches

Maintaining a separate branch per platform (e.g. `gba`, `web`) creates constant merge conflicts:

- `types.rs` differs on every branch (different type sizes)
- Any game logic change must be merged to every port branch
- Platform-specific fixes can't be tested against other platforms in CI
- Drift accumulates — ports fall behind main

Instead, the project uses a single branch with feature flags and a crate-per-platform workspace layout. Same branch, same code — the frontend crate selects features via `Cargo.toml`. CI tests all feature combinations.

## Crate Layout

The codebase is split into a Cargo workspace with six member crates, plus two non-member crates (`c64` and `libudev-sys-dlopen`):

```
roguelike/
├── Cargo.toml              (workspace root)
├── crates/
│   ├── core/               roguelike-core: game logic, zero platform deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs      ← #![cfg_attr(not(feature = "std"), no_std)]
│   │       ├── rules/      ← Pure game rules (no_std, always compiled)
│   │       │   ├── balance.rs, damage.rs, items.rs, monster_table.rs
│   │       │   ├── message.rs, direction.rs, seed_code.rs
│   │       │   └── tiles.rs, color.rs
│   │       ├── tier_micro/  ← Complete micro-tier game engine (no_std)
│   │       │   ├── game.rs, map.rs, entity.rs, fov.rs, ai.rs
│   │       │   ├── combat.rs, spawn.rs, prng.rs, msglog.rs, types.rs
│   │       ├── tier_compact/ ← GBA tier stubs (types.rs, prng.rs)
│   │       ├── game_step.rs ← GameStep trait, MicroGameStateAdapter, create_game()
│   │       ├── command.rs  ← GameCommand enum (Move(Direction), Pickup, etc.)
│   │       ├── types.rs    ← Coord = i32, Stat = i32, Pos (standard tier)
│   │       ├── game.rs, map.rs, combat.rs, ai.rs, fov.rs
│   │       ├── item.rs, pathfinding.rs, spawn.rs, entity.rs, data.rs
│   │       ├── platform.rs ← Renderer, InputSource traits
│   │       ├── spectate.rs ← FrameSink trait, NullFrameSink, render_frame()
│   │       ├── exploration_graph.rs ← Graph-based exploration tracking
│   │       ├── menu.rs, saves.rs, settings.rs
│   │       ├── dev_tools.rs, analytics.rs, scenario.rs
│   │       └── message_log.rs, message_history.rs
│   ├── saves/              roguelike-saves: SaveBackend trait (connected platforms)
│   │   ├── Cargo.toml      depends on core only
│   │   └── src/
│   │       └── lib.rs       SaveBackend trait definition
│   ├── tui/                roguelike-tui: shared terminal rendering + game loop
│   │   ├── Cargo.toml      depends on core + saves + crossterm
│   │   └── src/
│   │       ├── game_loop.rs (unified game loop, uses dyn GameStep)
│   │       ├── render.rs    (CrosstermRenderer, uses dyn RenderSource)
│   │       ├── render_source.rs (RenderSource trait: GameState + MicroGameStateAdapter impls)
│   │       ├── input.rs     (key-to-command translation)
│   │       ├── input_provider.rs (InputProvider trait, InputResult, GameInput)
│   │       └── saves.rs     (re-exports SaveBackend from roguelike-saves)
│   ├── terminal/           roguelike-terminal: local terminal frontend
│   │   ├── Cargo.toml      depends on core + tui + gilrs (optional)
│   │   └── src/
│   │       ├── main.rs
│   │       ├── render.rs   (CrosstermRenderer setup)
│   │       ├── input.rs    (crossterm event polling)
│   │       ├── terminal_input.rs (InputProvider impl for local terminal)
│   │       ├── local_saves.rs    (SaveBackend impl for local filesystem)
│   │       ├── dev_hooks.rs (DevHooks impl for debug overlay keys, dev-tools feature)
│   │       └── gamepad.rs  (gilrs gamepad input, optional `gamepad` feature)
│   ├── ssh/                roguelike-ssh: SSH server frontend
│   │   ├── Cargo.toml      depends on core + tui + russh + argon2
│   │   └── src/
│   │       ├── main.rs      (SSH + server startup)
│   │       ├── server.rs    (russh Handler, lobby↔session loop)
│   │       ├── lobby.rs     (dgamelaunch-style login/register TUI)
│   │       ├── accounts.rs  (argon2 password hashing, JSON account files)
│   │       ├── session.rs   (server menu + game session, LogOut returns to lobby)
│   │       ├── ssh_input.rs (InputProvider impl for SSH channels)
│   │       ├── saves.rs     (SaveBackend impl for per-user server directories)
│   │       ├── ansi_input.rs (raw byte → KeyEvent parser)
│   │       └── channel_writer.rs (async SSH channel → sync Write adapter)
│   ├── mcp/                roguelike-mcp: MCP server for LLM play
│   │   ├── Cargo.toml      depends on core + rmcp + tokio
│   │   └── src/
│   │       ├── lib.rs       (re-exports mcp_server and spectate modules)
│   │       ├── main.rs
│   │       ├── mcp_server.rs
│   │       └── spectate.rs  (file-based spectator, ROGUELIKE_SPECTATE_PATH)
│   ├── c64/               roguelike-c64: C64 frontend (no_std, production)
│   │   ├── Cargo.toml      depends on core (default-features = false); builds via rust-mos Docker, not a workspace member
│   │   └── src/             C64 frontend: c64.rs, input.rs, render.rs, main.rs (~4,300 lines). SID music, two-phase inventory, screen shake, I/O banking. All game logic from core::tier_micro + core::rules.
│   ├── libudev-sys-dlopen/ Drop-in libudev-sys replacement via dlopen (not a workspace member)
│   ├── atproto/            (future: AT Protocol identity + PDS save storage)
│   ├── web/                (future: WASM browser frontend)
│   ├── gba/                (future: GBA frontend)
│   └── vita/               (future: PS Vita frontend)
```

### Dependency Graph

```
roguelike-core
    ↓                              ↓
roguelike-saves (SaveBackend trait)  roguelike-c64  (no_std, depends on core directly)
    ↓
roguelike-tui   (crossterm rendering + game loop)
    ↓
├── roguelike-terminal  (desktop: keyboard + gamepad + local saves)
├── roguelike-ssh       (SSH server: per-user sessions + accounts)
└── roguelike-mcp       (MCP server: AI tool interface, core only)
```

> **Build pipeline note.** `roguelike-c64` is intentionally **excluded from the
> workspace `Cargo.toml` members list**. It requires the rust-mos Docker
> toolchain (a different compiler fork) and builds via its own `Makefile`.
> Standard `cargo build` at the workspace root builds only the PC crates.
> The `roguelike-core` dependency is specified via a relative path
> (`../../crates/core`) in the C64 crate's `Cargo.toml`, which works because
> the Docker container mounts the full project directory as `/work`.
> Cargo's feature unification is a non-issue — the C64 builds with a
> completely separate toolchain, not as a workspace member.

### Crate Roles

The **saves** crate defines the `SaveBackend` trait for platforms with enough storage for JSON-serialized game state and multiple save slots. It depends only on `core`. Connected platforms (`terminal`, `ssh`) implement it; constrained platforms (`gba`, `c64`) have their own save mechanisms suited to their hardware and don't depend on this crate.

The **tui** crate sits between `core`/`saves` and the terminal-based frontends (`terminal`, `ssh`). It provides the shared game loop, crossterm-based rendering, and the `InputProvider` trait. Both `terminal` and `ssh` implement these traits for their respective I/O mechanisms.

The **c64** crate is a Commodore 64 frontend using [rust-mos](https://github.com/mrk-its/rust-mos) — a fork of the Rust compiler backed by the llvm-mos LLVM backend that compiles `no_std` Rust to MOS 6502 machine code. The crate (~4,300 lines) is a production frontend that depends on `roguelike-core::tier_micro` and `roguelike-core::rules` for all game logic. C64-specific features include: SID music via CIA timer IRQs, two-phase inventory with action bar and equip bonus display, screen shake on combat via VIC-II raster IRQ, I/O banking overlay (frees 3.6 KB by placing pure-computation functions under $D000–$DFFF), BASIC/KERNAL ROM unmapping for additional RAM, corpse glyph rendering, seed code display on end screens, sprite-based loading spinner, and joystick + keyboard input with edge detection. See [C64 port proposal](../platforms/c64-port-proposal.md).

The **libudev-sys-dlopen** crate is a `[patch.crates-io]` replacement for `libudev-sys` that loads `libudev.so.1` via dlopen at runtime instead of linking at build time. This means Linux builds no longer require `libudev-dev` to compile — gamepad support loads when available, keyboard input works regardless.

### Dependency Isolation

Only `tui`, `terminal`, and `ssh` import `crossterm`. Only the `mcp` crate imports `rmcp`/`tokio`. The `core` and `saves` crates have zero platform dependencies.

Type aliases are per-tier: `types.rs` has `i32` for standard tier, `tier_micro/types.rs` has `u8`, `tier_compact/types.rs` has `i16`. The [capability tier system](capability-tier-reference.md) defines the full per-tier type hierarchy.

## What Goes Where

### Core

Everything that doesn't touch a platform API stays in `roguelike-core`:

- Game state, turns, commands (`game.rs`, `GameCommand` enum from `command.rs`)
- Map generation, rooms, tiles (`map.rs`)
- Combat, AI, FOV, pathfinding, spawning
- Entity data, monster templates, game config
- Message log, menus, settings, saves
- Type aliases and `GameColor`
- Platform traits (`Renderer`, `InputSource` in `platform.rs`)
- Spectating infrastructure (`FrameSink` trait, `NullFrameSink`, `render_frame()` in `spectate.rs`)

### Frontend crates

Anything that talks to hardware or external services:

- **Saves crate**: `SaveBackend` trait for platforms with JSON-serializable game state and multiple save slots. Depends only on core. Not used by constrained platforms (GBA, C64) that need hardware-specific save mechanisms.
- **TUI crate**: crossterm rendering, shared game loop, `InputProvider` trait, key-to-command translation
- **Terminal crate**: local crossterm event polling, local filesystem saves, gamepad input (gilrs, optional), terminal lifecycle
- **SSH crate**: russh server, lobby/accounts system, ANSI input parsing, per-user saves, SSH channel I/O
- **MCP crate**: rmcp server, tokio runtime, JSON serialization of game state
- **C64 crate** (production): `no_std` frontend with SID music, inventory, screen shake, I/O banking. See [C64 port proposal](../platforms/c64-port-proposal.md).
- **Atproto crate** (future): AT Protocol OAuth, PDS save backend. See [design doc](../design/atproto.md).
- **Web crate** (future): WASM entry point, canvas rendering. See [design doc](../design/atproto.md#wasm-frontend).
- **GBA crate** (future): GBA tile/sprite rendering, hardware saves. See [design doc](../platforms/gba-port.md).
- **Vita crate** (future): vita-sdk rendering, memory card saves. See [design doc](../platforms/vita-port.md).
- **C64 bridge** (future): external companion service proxying C64 TCP packets to AT Protocol. See [design doc](../platforms/c64-atproto-bridge.md).
- **Steam integration** (future, `steam` feature on terminal crate): Steamworks API for cloud saves and Steam Deck hooks. See [atproto design doc](../design/atproto.md#steam-cloud-coexistence).

## Color Abstraction

Colors use a core-defined `GameColor` enum:

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

## Feature Flags

Feature flags control which standard-library-dependent code is included:

- **`std`** (default) — gates all standard-tier code (shadowcasting FOV, A* pathfinding, Vec-based collections, ChaCha20 via rand). Also enables serde.
- **`serde`** (default) — serde serialization/deserialization for save/load
- **`data-files`** (default, requires `std`) — enables `game.toml` loading via TOML parser
- **`dev-tools`** (default, requires `std`) — debug console, overlays, analytics, headless runner, replay system

```toml
[features]
default = ["dev-tools", "data-files", "serde", "std"]
std = ["dep:rand", "serde"]
serde = ["dep:serde", "dep:serde_json"]
data-files = ["dep:toml", "std"]
dev-tools = ["std"]
```

The C64 uses `default-features = false` and only accesses `tier_micro` and `rules` — both are always compiled and `no_std` compatible. The `rules/`, `tier_micro/`, and `tier_compact/` modules are always available regardless of feature flags.

## Development Workflow

All development happens on one branch:

1. **Game logic** changes go in `core/` — automatically available to all frontends that depend on it.
2. **New platform?** Add a new crate under `crates/`, implement rendering and input. All frontends depend on `roguelike-core` directly.
3. **CI** builds all workspace frontends in a matrix — catches cross-platform breakage immediately. The C64 crate builds separately via Docker (rust-mos toolchain).

## Planned Work

- **Compact tier full implementation** — `tier_compact/` currently has stubs only (types + PRNG). Full game state, mapgen, entity storage deferred until GBA port begins. See [capability tier reference](capability-tier-reference.md).
- **SpawnDirective structs** — spawn logic is currently per-tier; planned to produce tier-agnostic directives that each tier applies to its own state.
- **AT Protocol integration** — Bluesky OAuth login, PDS-based portable saves. See [design doc](../design/atproto.md).
- **WASM frontend** — browser-based play via `web` crate. See [design doc](../design/atproto.md#wasm-frontend).

## Implemented (formerly planned)

- **Capability tier hierarchy** — `no_std` support, per-tier types, shared `rules/` module, `tier_micro/` complete game engine, `GameStep` cross-tier trait, `RenderSource` unified rendering trait. See [capability tier reference](capability-tier-reference.md).
- **Cross-platform seed system** — tier inference from seed numeric value (`seed <= 0xFFFF` → micro), base36 encode/decode, MCP seed_code param. See [capability tier reference](capability-tier-reference.md#19-seed-system-and-cross-platform-seeds).
- **C64 production frontend** — rewritten from standalone POC to production frontend (~4,300 lines) over `core::tier_micro` + `core::rules`.

## Architecture History

Key milestones in the cross-platform architecture:

- Platform abstraction — `Renderer` and `InputSource` traits in `core/src/platform.rs`
- Abstract color — `GameColor` enum in `core/src/types.rs`; crossterm removed from `entity.rs` and `data.rs`
- `GameCommand` moved to core — `command.rs` in core; terminal's `input.rs` only does key translation
- Workspace split — six crates: core, saves, tui, terminal, ssh, mcp
- Shared game loop — `tui/src/game_loop.rs` for both terminal and SSH frontends
- SSH server — `ssh` crate with russh, lobby system, per-user accounts and saves
- `SaveBackend` extracted to `crates/saves` — depends only on `roguelike-core`. See [atproto design doc](../design/atproto.md#prerequisite-extract-savebackend-to-cratessaves).
- `FrameSink` and `render_frame()` extracted to core — `crates/core/src/spectate.rs`. See [atproto spectating design doc](../design/atproto-spectating.md#phase-0-extract-render_frame-and-define-framesink).
- C64 POC validated — rust-mos toolchain, 13 KB `.PRG`, playable on VICE and c64.emu. See [C64 port proposal](../platforms/c64-port-proposal.md).
- Tier micro ported to core — `tier_micro/` module with complete no_std game engine (MicroGameState, iterative shadowcasting FOV, LFSR-16, fixed arrays)
- Rules module extracted — `rules/` with pure game rules (damage, balance, items, monster_table, GameEvent, Direction, seed_code, tiles, color)
- Standard-tier code gated behind `std` feature — `#![cfg_attr(not(feature = "std"), no_std)]`
- GameStep cross-tier trait — `game_step.rs` with MicroGameStateAdapter, create_game() factory
- RenderSource unified rendering trait — `tui/render_source.rs` eliminates duplicate render code paths
- C64 production frontend — depends on `core::tier_micro` + `core::rules`, ~4,300 lines with SID music, inventory, screen shake, I/O banking
