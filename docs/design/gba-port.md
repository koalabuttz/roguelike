# Game Boy Advance Port

> **Status:** Proposed. Design and implementation plan for the GBA frontend crate (`crates/gba/`).

How to bring the roguelike to the Game Boy Advance while keeping all game logic in `roguelike-core`, leveraging GBA hardware for features that benefit the game across platforms, and establishing patterns reusable by other constrained ports (Vita, C64).

## Hardware Summary

| Resource | Spec | Relevance |
|----------|------|-----------|
| CPU | ARM7TDMI @ 16.78 MHz (32-bit ARM + 16-bit Thumb) | Single-cycle bitwise ops. Thumb code is smaller but 2 cycles for some ops. |
| IWRAM | 32 KB (fast, 32-bit bus) | Hot data: entity array, sound grid, stack, small lookup tables. |
| EWRAM | 256 KB (slow, 16-bit bus) | Map tiles, explored set, floor deltas, tile/sprite VRAM staging. |
| VRAM | 96 KB | Tile data, tilemaps, sprite tiles. |
| OAM | 128 sprites, 32 affine matrices | Entities rendered as hardware sprites — free compositing. |
| Palette RAM | 512 bytes (256 BG colors + 256 sprite colors) | 16 palettes × 16 colors each for BG; same for sprites. Writable during VBlank. |
| ROM | Up to 32 MB | `const` tables (interaction matrix, tile graphics, monster defs). Zero RAM cost. |
| Display | 240×160 pixels, 30×20 tiles at 8×8 | Viewport fits ~28×18 gameplay tiles plus a 2-tile status bar. |
| Sound | 4 PSG channels (2 pulse, 1 wave, 1 noise) + 2 DMA | Procedural audio from register writes. See [acoustic propagation doc](acoustic-propagation.md#gba-psg). |
| Save | SRAM (32 KB), Flash (64/128 KB), or EEPROM (512 B / 8 KB) | SRAM is simplest. 32 KB is sufficient for compact save format. |
| Input | D-pad, A, B, L, R, Start, Select | 10 buttons. No analog. |

## Relationship to Existing Docs

This proposal covers the **GBA frontend crate** — rendering, input, save, audio, and the `no_std` adaptations needed in core. It does not redesign game logic. Several existing docs define systems that the GBA port consumes:

| Doc | What GBA uses from it |
|-----|----------------------|
| [cross-platform.md](../architecture/cross-platform.md) | Crate structure, `Renderer`/`InputSource` traits, feature-flag type sizing, `GameColor` mapping |
| [simulation.md](../architecture/simulation.md) | `SimBudget` caps for entity count, CA tiles/turn, event queue depth |
| [acoustic-propagation.md](acoustic-propagation.md) | PSG channel assignments, sound event → register write mapping |
| [gameplay-implementation-plan.md](gameplay-implementation-plan.md) | Items, stairs, XP, mood — GBA renders these, doesn't change their logic |

Where those docs specify GBA-relevant details (e.g., PSG audio, SimBudget values), this doc references rather than duplicates them. Where GBA needs something those docs don't cover (tile streaming, palette-based lighting, OAM management, save format), this doc defines it.

## Architecture

### Crate Structure

```
crates/gba/
├── Cargo.toml          depends on roguelike-core (no_std, gba feature)
├── src/
│   ├── main.rs         Entry point: #![no_std], #![no_main], gba crate setup
│   ├── render.rs       GbaRenderer: Renderer trait impl, tile streaming, palette mgmt
│   ├── sprites.rs      OAM management: entity → sprite mapping, animation state
│   ├── input.rs        GbaInput: InputSource trait impl, button → GameCommand
│   ├── lighting.rs     FOV distance → palette index mapping, flicker state
│   ├── audio.rs        PSG driver: SoundEvent → register writes
│   ├── saves.rs        SRAM read/write for compact save format
│   └── assets/
│       ├── tiles.rs    const tile graphics (8×8 pixel data, compiled into ROM)
│       └── sprites.rs  const sprite graphics (8×8 or 16×16 pixel data)
```

The GBA crate depends only on `roguelike-core` (with the `gba` feature flag) and the [`agb`](https://github.com/agbrs/agb) or [`gba`](https://crates.io/crates/gba) hardware abstraction crate. It does **not** depend on `roguelike-saves` — save handling is hardware-specific (SRAM) and doesn't need JSON serialization or multiple save slots.

### What Changes in Core

The [cross-platform architecture doc](../architecture/cross-platform.md) already defines the feature-flag mechanism for type sizing. The GBA port activates it and adds one new requirement: `no_std` compatibility for core when the `gba` feature is enabled.

#### Feature flag in `crates/core/Cargo.toml`

```toml
[features]
default = ["dev-tools", "data-files"]
dev-tools = []
data-files = ["toml"]
gba = []        # Enables i16 Coord/Stat, no_std-compatible paths
```

#### Type sizing in `types.rs`

Already specified in cross-platform.md:

```rust
#[cfg(feature = "gba")]
pub type Coord = i16;
#[cfg(feature = "gba")]
pub type Stat = i8;

#[cfg(not(feature = "gba"))]
pub type Coord = i32;
#[cfg(not(feature = "gba"))]
pub type Stat = i32;
```

`Stat` becomes `i8` on GBA (HP values 0–127 are sufficient; ATK/DEF values fit easily). `Coord` becomes `i16` (maps up to 32767×32767 tiles — far beyond GBA screen size).

#### `no_std` in core

The `gba` feature implies `no_std`. Core's current `std` dependencies:

| Usage | Current | `no_std` replacement |
|-------|---------|---------------------|
| `HashSet<Pos>` for FOV/explored | `std::collections::HashSet` | Fixed-size bitset indexed by `map.idx()`. A 30×20 map = 600 bits = 75 bytes. |
| `Vec<Entity>` for entity list | `std::vec::Vec` | `ArrayVec<Entity, N>` from `heapless` or a fixed-size array with a count. `N` comes from `SimBudget::max_entities` (128 for GBA). |
| `Vec<Tile>` for map tiles | `std::vec::Vec` | Fixed-size array `[Tile; MAX_TILES]`. 30×20 = 600 bytes. |
| `Vec<Rect>` for rooms | `std::vec::Vec` | `ArrayVec<Rect, MAX_ROOMS>`. 30 rooms × ~10 bytes = 300 bytes. |
| `Vec<String>` for messages | `std::vec::Vec` + `String` | `ArrayVec<ArrayString<64>, 8>` from `heapless` / `arrayvec`. 8 messages × 64 chars. |
| `String` for entity names | `std::string::String` | `ArrayString<16>` or `&'static str` (monster names are compile-time known). |
| `HashMap` for floor items | `std::collections::HashMap` | Not needed on GBA until items are implemented. When needed: parallel array indexed by `map.idx()`, or `ArrayVec` of `(Pos, Item)`. |

The approach: **gate `std` collections behind `#[cfg(not(feature = "gba"))]`**, with `no_std` alternatives behind `#[cfg(feature = "gba")]`. This is a significant refactor but is mechanical — the logic doesn't change, only the container types. The `heapless` crate provides `Vec`, `String`, and other `std`-like types backed by fixed-size arrays, reducing the diff.

A type alias layer can reduce churn:

```rust
#[cfg(feature = "gba")]
pub type EntityVec = heapless::Vec<Entity, 128>;
#[cfg(not(feature = "gba"))]
pub type EntityVec = std::vec::Vec<Entity>;
```

This keeps call sites unchanged: `entities.push()`, `entities.iter()`, `entities[i]` all work on both types.

**Risk:** This is the most invasive change the GBA port requires. It affects every module in core that uses `Vec`, `String`, or `HashSet`. The mitigation is that all existing tests continue to pass under the default feature set — the `gba` feature only activates when the GBA crate builds. CI tests both feature sets.

**"Don't close doors" note:** The `no_std` refactor benefits the C64 port (which also needs `no_std` with even tighter caps) and any future embedded target. It does not affect terminal/SSH/MCP/web builds, which continue using `std` collections with no capacity limits.

## Rendering

### Background Layer: The Dungeon

Use Mode 0 (tiled) with a single 256×256 pixel background layer (BG0). This gives a 32×32 tile screenblock — large enough to hold the visible map area (28×18 gameplay tiles) plus off-screen buffer tiles for scrolling.

#### Tile Graphics

Each tile type maps to an 8×8 pixel graphic stored in VRAM as 4bpp (16 colors per palette):

| Tile | Graphic | Palette |
|------|---------|---------|
| `Wall` | Solid block, subtle texture | Grey tones |
| `Floor` | Dotted or clean flat | Dark tones |
| `StairsDown` | `>` glyph or arrow-down graphic | Cyan/highlight |
| `StairsUp` | `<` glyph or arrow-up graphic | Cyan/highlight |
| `Water` (future) | Animated 2-frame wave | Blue palette |
| `Lava` (future) | Animated 2-frame glow | Red/orange palette |
| Corpse (`%`) | Small mark on floor tile | Dark red |
| Item glyphs | `!`, `/`, `[`, `?` on floor | Per-item-type color |
| Unexplored | Fully black | Palette 0 |
| Explored, not visible | Dim version of the base tile | Palette with reduced brightness |

Tile graphics are `const` data compiled into ROM. At 32 bytes per 8×8 4bpp tile, 64 unique tiles = 2 KB. This is negligible against the 32 MB ROM budget.

#### Tile Streaming

The map (up to 30×20 tiles, or larger on desktop-generated seeds clamped to GBA size) may exceed the 32×32 screenblock. The renderer maintains a **viewport** tracking which map region is loaded into the screenblock.

When the player moves, the renderer:

1. Updates BG0's scroll registers (`REG_BG0HOFS`, `REG_BG0VOFS`) for smooth pixel offset.
2. Writes one row or column of tilemap entries at the edge the player is moving toward.
3. The GBA's tilemap wraps at screenblock boundaries, so the overwritten edge wraps around to become the new leading edge.

This is a standard GBA scrolling technique. It costs one tilemap row (32 half-words = 64 bytes) or column (32 half-words = 64 bytes) written per player step, performed during VBlank.

#### Smooth Scrolling

Player movement animates over 4–6 frames rather than snapping:

1. Player presses a direction. `GameState::step()` executes immediately (game logic is instant).
2. The renderer interpolates the scroll offset from the old position to the new position over `N` frames.
3. During interpolation, input is buffered but not processed until the animation completes (preserving turn-based determinism).
4. The player sprite also interpolates position, matching the scroll.

Frame count `N` is a tuning constant (start with 4 = ~67ms at 60fps). Autorun and pathfinding use `N = 2` for faster traversal feel. This is purely a renderer concern — `GameState` sees instantaneous movement.

### Sprite Layer: Entities

Every living entity is a hardware sprite via the GBA's Object Attribute Memory (128 sprite slots). Dead entities (`alive == false`) are rendered as corpse tiles on the background layer, not as sprites.

#### Sprite Management

```
GbaRenderer {
    /// Maps entity index → OAM slot. Entities beyond max_sprites are not rendered.
    entity_to_oam: [Option<u8>; 128],
    /// Animation frame counter per OAM slot.
    anim_frame: [u8; 128],
}
```

Each frame during VBlank:

1. Iterate `GameState.entities` where `alive && visible`.
2. For each visible entity, assign an OAM slot (stable — same entity keeps the same slot until it dies or leaves FOV).
3. Set OAM attributes: position (world-to-screen transform), tile index (from glyph → sprite graphic table), palette (from `GameColor` mapping), flip/size.
4. Entities exiting FOV: set their OAM entry to off-screen or disable the sprite.

The player always occupies OAM slot 0 (highest priority). Monsters fill slots 1–N. With `SimBudget::max_entities = 128` and 128 OAM slots available, there is no contention in practice — the FOV limits how many entities are simultaneously visible to well under 128.

#### Animation

Two-frame idle animation for living entities:

- **Frame A:** Base sprite graphic.
- **Frame B:** 1-pixel vertical offset (bob) applied via OAM Y attribute.
- Toggle every 30 frames (0.5s at 60fps).

Attack animation when `melee_attack()` produces a hit:

- The attacker sprite shifts 2 pixels toward the defender over 2 frames, then returns.
- A small "impact" sprite (spark or slash) appears at the defender's position for 3 frames using a spare OAM slot.

Death animation:

- The killed entity's sprite uses the GBA's semi-transparency (OBJ blending with BG) to fade over 8 frames.
- After fade, the OAM slot is freed, and a corpse tile is written to the background.

All animation state lives in `GbaRenderer`, not in `GameState`. The core never knows about frames or pixels.

#### `GameColor` → Sprite Palette Mapping

The GBA has 16 sprite palettes of 16 colors each. Assign one palette per monster color:

| `GameColor` | Palette index | Primary color | Usage |
|-------------|---------------|---------------|-------|
| `Yellow` | 0 | #FFD700 | Player (`@`) |
| `Green` | 1 | #00CC00 | Goblin (`g`) |
| `DarkGreen` | 2 | #006600 | Orc (`o`) |
| `DarkRed` | 3 | #880000 | Troll (`T`) |
| `Red` | 4 | #CC0000 | Damage flash / effects |
| `Cyan` | 5 | #00CCCC | Stairs, items |
| `White` | 6 | #FFFFFF | UI, cursor |

New monster types added via `game.toml` map through `GameColor` to an existing palette. If a color has no palette assignment, fall back to palette 6 (white). This means data-driven monsters work without ROM changes — the constraint is the palette count, not the code.

### Dynamic FOV Lighting

The GBA's per-tile palette assignment turns the existing FOV distance data into a visual light gradient at zero per-tile CPU cost.

#### How It Works

The background tilemap stores a palette index per tile entry (4 bits). Instead of a single "floor" palette, define 4 floor palettes with decreasing brightness:

| BG palette | Brightness | FOV zone |
|------------|------------|----------|
| Palette 0 | 100% | Player's tile and radius 1–2 |
| Palette 1 | 75% | Radius 3–5 |
| Palette 2 | 50% | Radius 6–7 |
| Palette 3 | 25% | Radius 8 (FOV edge) |
| Palette 4 | 10% | Explored but not currently visible |
| Palette 5 | 0% (black) | Unexplored |

When the FOV updates (once per turn), the renderer walks the visible tile set, computes the Chebyshev distance from the player to each tile, and writes the corresponding palette index into the tilemap entry. Tiles that drop out of FOV get palette 4 (dim explored). This is one write per visible tile — on a 28×18 viewport, that's at most 504 half-word writes, well within a VBlank.

#### Torchlight Flicker

On a VBlank interrupt (60 Hz), the renderer randomly shifts 2–4 tiles at the FOV boundary between their assigned palette and one step dimmer. This is:

1. Pick 2–4 random tile indices from the FOV boundary set.
2. Toggle their palette index between N and N+1.
3. Write the tilemap entries.

Cost: ~20 register writes per frame. The visual effect is a flickering light boundary that makes the dungeon feel alive.

The flicker state is an `u32` LFSR (linear feedback shift register) seeded from the game's RNG. This keeps flicker deterministic for replay purposes, though since it's purely visual, determinism is optional.

**"Don't close doors" note:** The FOV distance data that drives palette selection comes from `compute_fov()` in core, which already returns `HashSet<Pos>`. The GBA renderer computes Chebyshev distance from the player to each visible tile — this is a per-tile subtraction, not a core change. Other platforms can implement similar lighting if desired (terminal could use ANSI 256-color greyscale; C64 uses color RAM as described in the [acoustic propagation doc](acoustic-propagation.md#visual-complement-dynamic-fov-lighting-c64)). The core doesn't know about palettes.

## Input

GBA buttons map to `GameCommand` and `MenuCommand`:

### Gameplay

| Input | Command | Notes |
|-------|---------|-------|
| D-pad | `Move { dx, dy }` | 4 cardinal directions |
| D-pad + L | Diagonal `Move` | L acts as a modifier for NE/NW/SE/SW. D-pad Up+Right with L held = `Move { dx: 1, dy: -1 }`. |
| A | `Wait` | |
| B | Pause menu | |
| R | Auto-explore | |
| Select | Look mode | |
| Start | Pause menu | |
| L + A | Autorun in last-moved direction | |
| L + R | Auto-fight (nearest) | |

### Diagonals via L Modifier

The GBA's D-pad natively reports only 4 directions (with hardware diagonals from pressing two buttons simultaneously, but this is unreliable on some hardware). The L-button modifier provides clean 8-directional input:

- **No L held:** D-pad = 4 cardinal directions.
- **L held:** D-pad Up = NW, D-pad Right = NE, D-pad Down = SE, D-pad Left = SW.

Alternative: L held + two D-pad buttons = diagonal (L + Up + Right = NE). This mirrors the desktop gamepad mapping where LB is the autorun modifier. Either scheme works; the choice is a playtesting question.

### Menus

| Input | Command |
|-------|---------|
| D-pad Up/Down | `MenuCommand::Up` / `Down` |
| A | `MenuCommand::Select` |
| B | `MenuCommand::Back` |

### Look Mode

| Input | Command |
|-------|---------|
| D-pad | Move cursor |
| A | Examine tile |
| B | Exit look mode |

## Save System

### Format

GBA SRAM provides 32 KB of battery-backed storage. JSON serialization (`serde_json`) is not available in `no_std` and would be wasteful of space. Instead, use a compact binary format.

Option A: **`postcard`** — a `no_std`-compatible serde format that produces compact binary. Core's `GameState` already derives `Serialize`/`Deserialize`, so this requires minimal code. A typical game state (30×20 map, ~50 entities, 8 messages) serializes to ~2–4 KB with `postcard`, fitting comfortably in 32 KB SRAM with room for multiple save slots.

Option B: **Manual binary layout** — hand-written `to_bytes()` / `from_bytes()` methods. Smaller output, more code to maintain, fragile across version changes.

Recommendation: **Option A (`postcard`)**. It leverages the existing serde derives, is `no_std`-compatible, and the GBA's 32 KB SRAM budget is generous enough that `postcard`'s overhead is acceptable.

#### Save Discipline

Follow the existing save modes from `settings.rs`:

- **Classic mode:** Save-and-quit on save, delete on death. One slot.
- **Casual mode:** Save without quitting. Up to 2 slots (SRAM budget constraint — 32 KB / ~4 KB per save = ~8 slots theoretical, but leave room for settings and header).

#### SRAM Layout

```
Offset  Size    Content
0x0000  4       Magic number ("RGSV")
0x0004  2       Format version
0x0006  1       Number of occupied save slots
0x0007  1       Active slot index
0x0008  2       Settings length
0x000A  var     Settings (serialized)
0x????  2       Slot 0 length
0x????  var     Slot 0 data (serialized GameState)
0x????  2       Slot 1 length
0x????  var     Slot 1 data
```

The format version enables forward compatibility — if the save format changes, the GBA can detect old saves and either migrate or reject them.

### Multi-Level Dungeon Saves

When stairs are implemented ([gameplay plan Phase 2](gameplay-implementation-plan.md#phase-2-stairs--multi-level-dungeons)), the one-way-descent model (recommended v1) means only the current floor needs saving. Previous floors are discarded. This keeps save size constant regardless of depth.

If bidirectional stairs are added later, the [dormant world pattern](../../simulation-on-retro-hardware.md#4-the-dormant-world-pattern) applies: store each visited floor as `(seed: u64, depth: u8, delta: FloorDelta)` where `FloorDelta` records player-caused changes (killed monsters, picked-up items) as a compact diff. At ~8–32 bytes per floor, 50 floors would cost ~0.4–1.6 KB — well within SRAM budget.

## Audio

The [acoustic propagation doc](acoustic-propagation.md#gba-psg) defines the GBA PSG channel assignments and sound event mapping. This section covers the GBA-specific driver implementation.

### PSG Driver

```rust
// crates/gba/src/audio.rs

/// Tracks current register state to avoid redundant writes.
pub struct PsgDriver {
    /// Last written frequency for each channel (0–3).
    freq: [u16; 4],
    /// Last written volume for each channel.
    vol: [u8; 4],
    /// Ambient drone state (channel 3 wave table).
    ambient_wave: u8,
    /// Flicker counter for ambient modulation.
    ambient_tick: u16,
}
```

Each game turn, the driver:

1. Receives the turn's `Vec<SoundEvent>` from `GameState` (or an `ArrayVec` on GBA).
2. Maps each event to channel assignments per the acoustic propagation doc.
3. Writes only changed registers (compare against cached state) to minimize bus traffic.
4. Updates the ambient drone: modulate channel 3's wave table and volume based on `GameState.tension` (from the escalation system) and room size.

The driver runs during VBlank after rendering. PSG register writes are fast (~2 cycles each), and a typical turn produces 0–3 sound events = 10–30 register writes.

### Volume Attenuation by Distance

Sound events carry an `intensity` field (0–255). The driver maps this to the GBA's 4-bit volume (0–15):

```rust
fn intensity_to_volume(intensity: u8) -> u8 {
    (intensity >> 4).min(15) // Linear map: 0–255 → 0–15
}
```

A combat sound at intensity 12, heard from 8 tiles away (attenuated to ~4), becomes volume 0 — inaudible. The player hears nearby events loudly and distant events faintly. This emerges naturally from the sound grid in core.

## Memory Budget

Estimated IWRAM (32 KB) usage for hot data:

| Data | Size | Notes |
|------|------|-------|
| Entity array | 128 × 20 bytes = 2,560 B | `Entity` with `i16` coords, `i8` stats, properties, mood/memory |
| Map tiles | 600 B (30×20) | `Tile` is 1 byte |
| FOV bitset | 75 B (600 bits) | Replaces `HashSet<Pos>` |
| Explored bitset | 75 B | Same |
| Structural walls | 75 B | Same |
| Sound grid | 600 B | `u8` per tile |
| Event queue | ~512 B | 32 events × 16 bytes |
| OAM shadow | 1,024 B | 128 entries × 8 bytes, written to OAM during VBlank |
| Sprite anim state | 256 B | Per-OAM-slot counters |
| PSG driver state | 32 B | |
| Stack + misc | ~4 KB | |
| **Total** | **~9.8 KB** | **30% of IWRAM** |

EWRAM (256 KB) usage:

| Data | Size | Notes |
|------|------|-------|
| Tile graphics (staging) | 2 KB | 64 tiles × 32 bytes. Also in ROM; EWRAM copy for palette swaps. |
| Sprite graphics (staging) | 4 KB | 128 sprite tiles. |
| Room array | 300 B | 30 rooms × 10 bytes |
| Message buffer | 512 B | 8 × `ArrayString<64>` |
| Floor deltas (if multi-level) | ~1.6 KB | 50 floors × 32 bytes |
| Save staging buffer | ~4 KB | For serialization before SRAM write |
| Scroll/lighting working buffers | ~2 KB | Tilemap row/column staging |
| **Total** | **~14.4 KB** | **5.6% of EWRAM** |

The budget is comfortable. Even doubling all estimates leaves significant headroom.

## Implementation Phases

Each phase is independently testable. Phases 1–3 produce a playable GBA build. Phases 4–6 add polish and integrate with systems from other design docs.

### Phase 1: Scaffold and Static Rendering (Effort: M)

**Goal:** GBA binary boots, renders a static dungeon map, player is visible.

- Set up `crates/gba/` with `agb` or `gba` crate, `#![no_std]`, `#![no_main]`.
- Activate `gba` feature on core. Begin `no_std` refactor for core containers (this is the bulk of the effort — see [What Changes in Core](#what-changes-in-core)).
- Implement `GbaRenderer` with static tile rendering (no scrolling yet). Render the map as background tiles in Mode 0.
- Render the player as OAM sprite 0.
- Display a static HP bar using background tiles on a second layer (BG1) or the bottom 2 tile rows of BG0.
- **Test:** Build with `cargo build --target thumbv4t-none-eabi -p roguelike-gba`. Boot in mGBA emulator. Verify map and player render.

**Depends on:** Nothing. This is the starting point.

### Phase 2: Input and Game Loop (Effort: S-M)

**Goal:** Playable turn-based game — move, attack, die.

- Implement `GbaInput` (`InputSource` trait). D-pad → cardinal movement, L modifier → diagonals.
- Wire up the game loop: read input → `GameState::step()` → re-render changed tiles.
- Render monsters as sprites. Assign OAM slots to visible entities.
- Display messages in the bottom tile rows (fixed-width font, 30 chars per line, 2 lines).
- Handle game over state (death screen, restart).
- **Test:** Full gameplay loop in mGBA. Kill goblins, die to trolls.

**Depends on:** Phase 1.

### Phase 3: Smooth Scrolling + Tile Streaming (Effort: M)

**Goal:** Camera follows the player smoothly instead of snapping.

- Implement viewport tracking and tilemap edge-writing (tile streaming).
- Implement scroll register interpolation over `N` frames per move.
- Buffer input during scroll animation.
- Handle autorun and pathfinding (multi-step sequences) with `N = 2` frame animations.
- **Test:** Walk around a 30×20 map. Scrolling feels smooth, no visual tearing, edge tiles load correctly.

**Depends on:** Phase 2.

### Phase 4: Palette-Based FOV Lighting (Effort: S)

**Goal:** Visible tiles fade from bright near the player to dim at the FOV edge.

- Define 6 BG palettes (4 brightness levels + explored-dim + unexplored-black).
- After each FOV update, write palette indices into tilemap entries based on Chebyshev distance.
- Implement torchlight flicker on VBlank interrupt.
- **Test:** Move around. Light gradient is visible. FOV boundary flickers subtly.

**Depends on:** Phase 3 (needs tile streaming to update palette indices on scrolled tiles).

### Phase 5: Sprite Animation + Audio (Effort: M)

**Goal:** Entities animate. Combat has audio feedback.

- Implement 2-frame idle bob, attack lunge, and death fade animations.
- Implement PSG driver. Map the 3–4 most important sound events first: player footstep, combat hit, monster death, damage taken.
- Ambient drone on channel 3 (optional — adds atmosphere but isn't mechanically necessary until escalation is implemented).
- **Test:** Animations are smooth, audio plays at correct moments, volume attenuation works by distance.

**Depends on:** Phase 2 (sprites must exist). Independent of Phases 3–4.

### Phase 6: Save System (Effort: S-M)

**Goal:** Game state persists across power cycles.

- Add `postcard` as a dependency (or implement manual binary layout).
- Implement SRAM read/write routines.
- Wire up save/load to the existing pause menu commands.
- Implement classic and casual save discipline.
- **Test:** Save game, power cycle mGBA (or reset), load game. State is restored. Classic mode deletes save on death.

**Depends on:** Phase 2 (needs a playable game to save).

## Open Questions

1. **`agb` vs `gba` crate.** `agb` is higher-level (provides allocator, sprite management, tiled backgrounds) but opinionated. The `gba` crate is thinner and more manual. `agb` may fight the project's own rendering approach; `gba` requires more boilerplate but fewer surprises. Needs evaluation against the specific tile streaming and OAM management requirements above.

2. **Map size clamping.** Desktop-generated maps can be 80×40 or larger. The GBA viewport is 28×18 with a 32×32 screenblock. Options:
   - Generate GBA-sized maps (30×20) when the `gba` feature is active, via `GameConfig` overrides.
   - Support larger maps with tile streaming (Phase 3 handles this). The screenblock wraps, so maps up to ~60×60 work. Beyond that, a second screenblock is needed (BG0 supports two screenblocks at the cost of VRAM).
   - The simplest v1: cap map size to 30×20 on GBA via `game.toml` defaults baked into ROM.

3. **Seed code compatibility.** GBA-generated dungeons (30×20 map, `i16` coords, `i8` stats) will not produce the same dungeon as desktop for the same seed — the map dimensions and RNG consumption differ. Seed codes should encode the platform or dimensions (the format already supports `seed-WxH`, e.g., `r7z3kq-30x20`) so that seed sharing is explicit about what generated the dungeon. Sharing seeds across platforms with different map sizes is not a goal.

4. **`no_std` refactor scope.** The `Vec` → `heapless::Vec` / `ArrayVec` refactor could be done incrementally (start with the modules GBA actually exercises — `map`, `entity`, `fov`, `combat`, `ai`, `spawn`, `game` — and leave `analytics`, `scenario`, `exploration_graph` as `std`-only behind feature gates) or all at once. Incremental is lower risk but means some core modules aren't available on GBA. Since those modules are dev-tools or MCP-specific, gating them behind `#[cfg(not(feature = "gba"))]` is fine.

5. **Tile art style.** Pixel art tiles versus rendered font glyphs. Using actual ASCII glyphs in an 8×8 font preserves the terminal aesthetic and is simpler to implement (one tile per character, palette swap for color). Custom pixel art tiles are more visually interesting but require an artist or generated assets. A reasonable path: start with font-glyph tiles, add pixel art later as a separate tileset (the tile index mapping is the same either way).

## Testing Strategy

### Emulator Testing

All development and CI testing uses **mGBA** (accurate GBA emulator with CLI mode). `mgba-rom-test` can run headless with a timeout, verifying that the ROM boots and doesn't crash.

### Cross-Feature CI

The CI matrix ([cross-platform.md](../architecture/cross-platform.md)) already tests all workspace crates. Add:

```yaml
- name: Build GBA
  run: cargo build --target thumbv4t-none-eabi -p roguelike-gba

- name: Test core with GBA features
  run: cargo test -p roguelike-core --features gba --no-default-features
```

This catches regressions where a core change breaks `no_std` compatibility or overflows a fixed-size container.

### Gameplay Parity

The deterministic replay system can verify that a given seed + command sequence produces the same outcomes on GBA-featured core as on default core (accounting for map size differences). This is a property test: "for any command sequence, GBA and desktop produce the same combat results, XP awards, etc. when given the same seed and map dimensions."

## Relationship to Other Constrained Ports

The patterns established here are reusable:

| Pattern | GBA | Vita | C64 |
|---------|-----|------|-----|
| Feature-flag type sizing | `i16` / `i8` | `i32` / `i32` (same as desktop) | `i8` / `i8` |
| `no_std` core | Required | Not required (Vita has `std`) | Required |
| Fixed-size containers | `heapless` or `ArrayVec` | Not needed | Same as GBA, smaller caps |
| Compact save format | `postcard` over SRAM | vita-sdk save API (can use JSON) | Custom binary over disk |
| Hardware-specific renderer | Tiles + OAM | vita2d library | PETSCII + color RAM |
| `SimBudget` caps | 128 entities, 256 CA tiles/turn | 512 entities, 1000 CA tiles/turn | 32 entities, 64 CA tiles/turn |

The Vita port can skip the `no_std` refactor entirely — it has a full OS. The C64 port benefits directly from the GBA's `no_std` work, needing only tighter capacity bounds and `i8` type sizing. This is the "don't close doors" principle in action: the GBA port's infrastructure investment pays forward.
