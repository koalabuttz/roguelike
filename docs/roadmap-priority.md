# Roadmap Priority Analysis

Detailed breakdown of the [roadmap](../README.md#roadmap) with prioritization based on dependencies, effort, and impact.

## Dimensions

- **Blocks** — How many other roadmap items depend on this?
- **Impact** — How much does this change the player experience?
- **Effort** — S (hours), M (days), L (week+), XL (multi-week)

## Tier 1: Prerequisites

These items block the most downstream work or are essential for the game to function.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| ~~Input abstraction~~ | ~~&zwj;~15 items~~ | ~~Medium~~ | ~~S~~ | ~~Controller, replay, platforms, mouse-only, one-handed, auto-explore all depend on this. Single most blocking item.~~ Done. |
| Save/load | ~6 items | High | M | Unlocks meta-progression, bones, crash recovery. Expected baseline feature. |
| Stairs | ~3 items | High | M | Transforms single-room arena into a multi-level dungeon. |
| Items | ~4 items | High | M | Meaningful player decisions. Unlocks menus, inventory UI, hunger (food is an item). |
| Message history | 0 | Medium | S | Tiny effort, immediate QoL, essential for screen readers. |
| Code of Conduct | 0 | Low | S | One file. Should exist before accepting external contributions. |

## Tier 2: Core Systems

Fills out the core game loop and addresses high-value, low-effort improvements.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Menus | ~3 items | High | M | Needed the moment items exist. Design for keyboard + controller from the start. |
| Seeded RNG | ~4 items | Medium | S | Unlocks replay, daily challenges, seed sharing. Small refactor, outsized downstream value. |
| A* pathfinding | ~2 items | High | M | Current greedy chase gets stuck on corners in complex layouts. Unlocks MCP pathfind_to and auto-explore. |
| Hunger | 0 | Medium | S | Classic mechanic. Simple (decrement per turn, eat food). Depends on items. |
| Experience/leveling | 0 | High | M | Progression within a run. |
| Data-driven content | ~2 items | Medium | M | Move templates to RON/TOML. Unlocks hot reload and community content. |
| Look mode | 0 | Medium | S | "What is that T?" Needed as monster variety grows. |
| ~~Colorblind modes~~ | ~~0~~ | ~~High~~ | ~~S~~ | ~~Trivial if colors are in data files. 8% of men affected by current palette.~~ Done (protanopia, deuteranopia palettes in Settings). |
| MCP autorun tuning | 0 | High | S | Autorun stops every 1 step inside rooms due to open-neighbor count. Only stop at meaningful decision points (corridor forks, room exits). [Playtest: 31% of tool calls wasted.](llm-playtest-2026-02-13.md) |
| MCP pathfind_to tool | 0 | High | S | Walk to a visible/explored `(x, y)` via shortest path. Requires A*. Eliminates navigation fumbles. [Playtest: 19% of tool calls wasted.](llm-playtest-2026-02-13.md) |
| MCP explored map tool | 0 | Medium | S | Return all previously-seen tiles so LLM can plan exploration routes instead of wandering. |
| Wandering monsters | 0 | Medium | S | Spawn new monsters over time to pressure the player forward. Primary anti-camping guard for HP regen — prevents infinite wait-to-heal. Uses existing spawn system. |

## Tier 3: Extended Features

Builds on the core systems to add depth, platforms, and accessibility.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| ~~Platform abstraction~~ | ~~&zwj;~5 items~~ | ~~Low~~ | ~~M~~ | ~~Required before any platform port. Do alongside input abstraction.~~ Done. |
| ~~Type aliases~~ | ~~&zwj;~2 items~~ | ~~None~~ | ~~S~~ | ~~30-minute refactor. Enables platform-specific type sizing.~~ Done. |
| Controller support | ~1 item | High | M | Requires input abstraction. Enables couch play, Steam Deck. |
| Replay system | ~2 items | Medium | M | Requires seeded RNG. Unlocks spectating and seed sharing. |
| Auto-explore | 0 | High | M | Major QoL. Requires A*. Benefits all players, essential for motor accessibility. |
| Magic/abilities | 0 | High | L | Big design space. Requires targeting UI. Expands combat significantly. |
| Granular difficulty | 0 | Medium | S | Config toggles. Broadens who can enjoy the game. |
| Context-sensitive help | 0 | Medium | S | `?` key on any screen. Accessibility and onboarding. |
| Debug overlay | 0 | Medium | S | See AI decisions, FOV, pathfinding. Accelerates development. |
| ~~MCP spectator mode (file)~~ | ~~&zwj;~1 item~~ | ~~Medium~~ | ~~S~~ | ~~Write rendered frames to a file after each MCP action; viewer process displays them. Proves the concept, upgrades to TCP later. [Design doc.](spectator-mode-options.md)~~ Done (`ROGUELIKE_SPECTATE_PATH` env var, atomic file writes, integrated into all MCP actions). |
| Meta-progression | 0 | High | L | Persistent unlocks between runs. Requires save/load. |
| Web (WASM) | ~2 items | High | L | Browser-based play. Requires platform abstraction. |
| One-handed play | 0 | Medium | S | Keybind preset. Falls out of input abstraction + options. |
| High-contrast mode | 0 | Medium | S | Alternative color set. Do together with colorblind modes. |

## Tier 4: Networking & Polish

Features that build on a stable core and benefit from a wider feature set.

| Item | Blocks | Impact | Effort | Notes |
|------|--------|--------|--------|-------|
| Shared leaderboard | 0 | Medium | M | Requires a server/API. |
| Daily challenges | 0 | Medium | M | Requires seeded RNG + leaderboard. |
| ~~Seed sharing~~ | ~~0~~ | ~~Medium~~ | ~~S~~ | ~~Falls out of seeded RNG almost for free.~~ Done (base36 seed codes, title menu entry, death screen display, MCP seed_code param). |
| Steam Deck | 0 | Medium | M | Requires controller support. Steam Input API. |
| MCP spectator mode (TCP) | ~1 item | Medium | M | Upgrade file-based spectator to TCP server on localhost. Multiple viewers, remote access, WebSocket upgrade path. Enables live spectating. [Design doc.](spectator-mode-options.md) |
| Live spectating | 0 | High | L | Requires replay + networking. TCP spectator mode is a stepping stone. |
| Bones files | 0 | Medium | M | Requires save/load + networking. |
| SSH server | 0 | Medium | L | Requires platform abstraction. |
| Options/settings | 0 | Medium | M | Grows naturally as features accumulate. |
| Targeting | 0 | Medium | M | Needed for ranged magic. Distinct UI mode. |
| Hot reload | 0 | Medium | S | Requires data-driven content. Dev QoL. |
| ~~Balance telemetry~~ | ~~0~~ | ~~Medium~~ | ~~M~~ | ~~Useful once there's enough content to balance.~~ Done (per-game analytics, aggregate stats, CI balance workflow with baseline diffing). |
| Mouse-only play | 0 | Medium | M | Click-to-move + menus. Benefits touchscreen too. |
| Adjustable input timing | 0 | Low | S | Config option for players who need it. |
| Sound effects | 0 | Medium | M | Rodio + SoundEvent. |
| Tutorial/guided run | 0 | Medium | M | Useful once complexity warrants it. |
| Localization | 0 | Medium | L | Externalize strings. Easier after game text stabilizes. |
| Character identity | 0 | Low | S | Name + pronouns. Small but meaningful. |

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
| Game Boy Advance | 0 | Low | XL | Requires no_std rewrite of data structures. |
| PS Vita | 0 | Low | L | Native ARM via vita-sdk; hardware buttons, OLED display, memory card saves. |
| Commodore 64 | 0 | Low | XL | Requires no_std, 8-bit types, custom toolchain. |

## Critical Path

```
Input abstraction
  → Save/load
    → Items + Menus
      → Stairs + Hunger
        → Experience/leveling
          = Complete game loop

  → Seeded RNG
    → Replay system
      → Daily challenges / Seed sharing

  → Controller support
    → Steam Deck

  → Platform abstraction ✓
    → WASM (web)
      → Spectating / Leaderboards

  → MCP spectator (file) ✓
    → MCP spectator (TCP)
      → Live spectating

A* pathfinding
  → MCP pathfind_to tool
  → Auto-explore
```

Input abstraction and save/load unlock both the gameplay branch and the
networking branch. A* pathfinding unlocks MCP and exploration improvements.
Everything else hangs off those three.
