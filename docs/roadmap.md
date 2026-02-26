# Roadmap Priority Analysis

Detailed breakdown of the [roadmap](../README.md#roadmap) with prioritization based on dependencies, effort, and impact.

## Dimensions

- **Blocks** — How many other roadmap items depend on this?
- **Impact** — How much does this change the player experience?
- **Effort** — S (hours), M (days), L (week+), XL (multi-week)

## Tier 1: Prerequisites

These items block the most downstream work or are essential for the game to function.

All Tier 1 prerequisites are **complete**. See [Completed](#completed) section.

## Tier 2: Core Systems

Fills out the core game loop and addresses high-value, low-effort improvements.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Hunger | 0 | Medium | S | Classic mechanic. Simple (decrement per turn, eat food). Depends on items. |
| Item-based progression | 0 | High | M | Progression via enchantment scrolls, depth-gated gear, permanent consumables. [Implementation plan.](design/gameplay-implementation-plan.md#phase-4-item-based-progression) |

## Tier 3: Extended Features

Builds on the core systems to add depth, platforms, and accessibility.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Magic/abilities | 0 | High | L | Big design space. Requires targeting UI. Expands combat significantly. |
| Granular difficulty | 0 | Medium | S | Config toggles. Broadens who can enjoy the game. |
| Meta-progression | 0 | High | L | Persistent unlocks between runs. Requires save/load. |
| AT Protocol integration | ~2 items | High | L | Bluesky login via OAuth on SSH (HTTP callback bridge) and terminal (loopback redirect). Saves stored in user's PDS for cross-platform portability. [Design doc.](design/atproto.md) |
| Web (WASM) | ~2 items | High | L | Browser-based play. Requires platform abstraction + `roguelike-saves`. CanvasRenderer, Web Worker for blocking game loop. [Design doc.](design/atproto.md#wasm-frontend) |
| One-handed play (remaining) | 0 | Medium | S | Partially done (left-hand QWEASDZXC and WEASDZXCR layouts). Mouse-only and macros still TODO. |

## Tier 4: Networking & Polish

Features that build on a stable core and benefit from a wider feature set.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Shared leaderboard | 0 | Medium | M | Requires a server/API. |
| Daily challenges | 0 | Medium | M | Requires seeded RNG + leaderboard. |
| Steam Deck + Steam Cloud | 0 | Medium | M | Controller support done (gilrs). Remaining: Steam Input API, Steam Auto-Cloud config for save directory sync. Steam Cloud syncs the local save/cache directory, coexisting with PDS saves — see [atproto design doc](design/atproto.md#steam-cloud-coexistence). |
| MCP spectator mode (TCP) | 0 | Low | M | Upgrade file-based spectator to TCP server on localhost for low-latency local multi-viewer. Largely superseded by atproto spectating for remote viewing. [Design doc.](design/spectator-mode.md) |
| Atproto spectating | 0 | High | L | Federated live spectating via PDS frame publishing + Jetstream. Zero infrastructure — player's PDS handles storage and delivery. Depends on atproto OAuth. [Design doc.](design/atproto-spectating.md) |
| Bones files | 0 | Medium | M | Requires save/load + networking. |
| PDS save backend | 0 | High | M | Store saves in user's AT Protocol PDS repo using custom lexicons. Enables cross-server save portability. Part of [atproto integration](design/atproto.md#phase-2-pds-save-backend). |
| Options/settings | 0 | Medium | M | Grows naturally as features accumulate. |
| Targeting | 0 | Medium | M | Needed for ranged magic. Distinct UI mode. |
| Mouse-only play | 0 | Medium | M | Click-to-move + menus. Benefits touchscreen too. |
| Adjustable input timing | 0 | Low | S | Config option for players who need it. |
| Sound effects | 0 | Medium | M | Rodio + SoundEvent. |
| Tutorial/guided run | 0 | Medium | M | Useful once complexity warrants it. |
| Localization | 0 | Medium | L | Externalize strings. Easier after game text stabilizes. |

## Tier 5: Long-Term

Significant investment. May depend on earlier tiers being complete.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Configurable glyphs | 0 | Low | M | Pairs with data-driven content. |
| Reduced clutter mode | 0 | Low | S | Display option. |
| Screen reader support | 0 | Medium | L | Structured output redesign. Important but complex to do well. |
| Macros | 0 | Low | M | Power-user feature. |
| Predictable UI | 0 | Low | S | Design principle — follow always, not a standalone task. |
| Visual audio alternatives | 0 | Low | S | Design rule for when audio is added. |
| Animation effects | 0 | Low | M | Terminal animations are limited. |
| Music | 0 | Low | M | Atmospheric but optional for a terminal game. |
| Tileset support | 0 | Medium | L | Alternative renderer. |
| Scripting | 0 | Medium | XL | Lua/Rhai embedding. Only if community content demands it. |
| Map editor | 0 | Low | L | For hand-crafted content if needed. |
| Game Boy Advance | 0 | Low | XL | Runs at tier compact (i16 coords, no_std). Compiles `core::rules` + `core::tier_micro` + `core::tier_compact` (compact is stubs until GBA work begins). Frontend crate needed. |
| PS Vita | 0 | Low | L | Native ARM via vita-sdk; hardware buttons, OLED display, memory card saves. |
| Commodore 64 | 0 | Low | XL | Runs at tier micro (u8 coords, u8 stats, fixed arrays, LFSR-16). C64 crate is a thin frontend using `core::tier_micro` + `core::rules`. Builds via rust-mos Docker. |
| C64 AT Protocol bridge | 0 | Low | L | Self-hostable Docker bridge connecting C64 (via Ultimate64 Ethernet) to PDS saves and spectating. External companion service, not a workspace crate. [Design doc.](platforms/c64-atproto-bridge.md) |

## Critical Path

```
Gameplay branch (current focus):
  Save/load ✓
    → Menus ✓
    → Wandering monsters ✓
      → Items ✓
        → Stairs ✓
          → Item-based progression               ← NEXT
            = Complete game loop

Platform/identity branch (deferred until gameplay branch completes):
  Platform abstraction ✓
    → SSH server ✓
      → Extract SaveBackend to crates/saves ✓
        → AT Protocol OAuth (SSH + terminal login with Bluesky)
          → PDS save backend (portable saves across all frontends)
            → WASM frontend (browser play + same saves)

Spectating branch:
  MCP spectator (file) ✓
    → Extract FrameSink/render_frame to core ✓
      → Thread FrameSink through game loop ✓
        → Atproto spectating (federated live spectating via Jetstream)
          → Web spectate viewer (lightweight JS, no full WASM needed)

Other completed branches:
  Input abstraction ✓ → Controller support ✓ → Steam Deck + Steam Cloud
  Seeded RNG ✓ → Replay system ✓ → Daily challenges / Seed sharing ✓ (seed sharing done)
  A* pathfinding ✓ → MCP pathfind_to ✓, Auto-explore ✓, MCP exploration graph ✓
```

Items and stairs are complete. The **immediate priority is Item-based progression**
(Phase 4), the final piece of the core game loop. See the
[gameplay implementation plan](design/gameplay-implementation-plan.md) for
detailed designs and playtest gates for each phase.

The platform/identity branch (atproto OAuth → PDS saves → WASM) and the
spectating branch (atproto spectating via Jetstream) are deferred until the
gameplay branch is complete — there's no value in platform expansion until the
game has items, stairs, and progression. See the
[atproto design doc](design/atproto.md) (4 phases) and the
[atproto spectating design doc](design/atproto-spectating.md) (5 phases).

## Completed

Items that have been implemented, organized by original tier.

### Tier 1

| Item | Notes |
|------|-------|
| Input abstraction | Controller, replay, platforms, mouse-only, one-handed, auto-explore all depend on this. Single most blocking item. |
| Save/load | Serde serialization, classic/casual modes, 5 save slots in casual. |
| Message history | Ctrl+P, scrollable, gamepad LB/RB page up/down. |
| Code of Conduct | One file. |
| Items | Items on the ground, inventory, equipment (weapons/armor), consumables (potions, scrolls). Data-driven via `[[items]]` in `game.toml`. MCP support (pickup, use_item, equip actions). |
| Stairs | Multi-level dungeons, `>` stairs, depth scaling, win condition at target depth. Descend preserves player stats/inventory. |

### Tier 2

| Item | Notes |
|------|-------|
| Menus | Title screen, pause menu, settings, seed entry. |
| Seeded RNG | Separate RNG streams per system. |
| A* pathfinding | Unlocks MCP pathfind_to and auto-explore. |
| Data-driven content | `game.toml` with hot reload via F10. |
| Look mode | Cursor-based tile examination, `x` key / Y button. |
| Colorblind modes | Protanopia, deuteranopia palettes in Settings. |
| MCP autorun tuning | Smarter stop conditions. |
| MCP pathfind_to tool | Walk to a visible/explored `(x, y)` via shortest path. |
| MCP explored map tool | `get_explored_map` with frontier markers. |
| MCP exploration graph | `exploration_graph.rs`, integrated into all MCP observations. |
| Wandering monsters | Time-pressure spawning with grace period, idle acceleration, spawn chance, max cap. `Wander` AI behavior (switches to Chase on LOS). Distance-based sound cues. Data-driven via `[wandering]` in `game.toml`. |

### Tier 3

| Item | Notes |
|------|-------|
| Platform abstraction | Required before any platform port. |
| Type aliases | 30-minute refactor. Enables platform-specific type sizing. |
| Extract SaveBackend to `crates/saves` | `SaveBackend` trait moved from `tui` to `crates/saves` (depends only on core). `tui/saves.rs` re-exports. Connected platforms depend on `roguelike-saves`; constrained platforms don't. |
| Extract FrameSink/render_frame to core | `FrameSink` trait, `NullFrameSink`, and `render_frame()` now in `crates/core/src/spectate.rs`. MCP crate imports `render_frame` from core. |
| Thread FrameSink through game loop | `run_game_loop` accepts `&dyn FrameSink` parameter. `SpectatorWriter` renamed to `FileFrameSink` (implements `FrameSink`). Terminal and SSH pass `&NullFrameSink`. Three `write_frame` call sites in the game loop (autorun, auto-explore, normal commands). |
| Controller support | gilrs, 8-dir d-pad/stick, LB autorun, context-sensitive button mapping. |
| Replay system | Deterministic recording/playback, golden replays. |
| Auto-explore | `o` key, gamepad X button, MCP `auto_explore` tool. |
| Context-sensitive help | Dynamic help from Settings and GameData, `?` key. |
| Debug console | In-game console (`dev-tools` feature): teleport, god mode, FOV toggle, spawn monsters, stat editing, replay export. |
| Debug overlay | F6–F12 toggles: FOV, targets, pathfinding, frontiers, reveal monsters, monster FOV. |
| MCP spectator mode (file) | `ROGUELIKE_SPECTATE_PATH` env var, atomic file writes, integrated into all MCP actions. [Design doc.](design/spectator-mode.md) |
| High-contrast mode | ColorPalette variant with brighter remappings. |
| Headless runner | Automated playtesting binary: run N games, configurable seeds/presets, replay support, analytics, parameter sweeps, golden replay management. [Docs.](tooling/headless-runner.md) |
| Scenario framework | Fluent builder API for composing specific game states and asserting balance outcomes. |
| Golden replay regression | Stored deterministic replays with expected results; detects unintended gameplay changes. |
| Parameter sweeps | Sweep across player stats (HP, ATK, DEF) to find balance boundaries; JSON config, structured output. |
| LLM playtesting | Strategic LLM-driven playtesting via `/playtest` skill and `tools/llm_playtest.py`; dual backends, parallel execution, token optimization. [Docs.](tooling/llm-playtesting.md) |
| CI balance check | GitHub Actions workflow runs headless presets on every gameplay change, diffs against cached baseline, posts verdict to workflow summary and PR comments. |
| Capability tier system | `rules/` module (no_std pure game rules), `tier_micro/` (complete C64 game engine), `tier_compact/` stubs (GBA, deferred), `GameStep` trait (cross-tier interface), `RenderSource` trait (unified rendering), standard-tier code gated behind `std` feature. |
| C64 thin frontend | C64 crate rewritten as thin frontend over `roguelike-core::tier_micro` + `roguelike-core::rules` (14.9 KB binary). |
| Viewport scrolling | Player-centered viewport for maps larger than terminal, universal across tiers. |

### Tier 4

| Item | Notes |
|------|-------|
| Seed sharing | Base36 seed codes, title menu entry, death screen display, MCP seed_code param. |
| SSH server | russh, lobby with login/register, per-user saves, argon2 password hashing. Server menu (Play / Watch / Log Out) with lobby↔session loop. Platform-aware menus ("Lobby" instead of "Quit" on SSH). |
| Hot reload | F10 in dev-tools build reloads `game.toml`. |
| Balance telemetry | Per-game analytics, aggregate stats, CI balance workflow with baseline diffing. |
| Character identity | Pronouns enum, player_name in Settings, shown in save slots and death screen. |
