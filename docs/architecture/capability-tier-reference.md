# Capability Tier System: Technical Reference

**Cross-platform reference for the capability tier hierarchy in `roguelike-core`.**
Defines per-tier types, algorithms, and the sharing boundary between tiers.
See the [C64 Port Proposal](../platforms/c64-port-proposal.md) §5 for motivation and the
[cross-platform architecture](cross-platform.md) for workspace layout.
For C64-specific hardware guidance (cycle budgets, CIA multiplexing, code style),
see the [C64 platform guide](../platforms/c64-platform-guide.md).

---

## 1. Core Structure

### 1.1 Directory Layout and Module Organization

`roguelike-core` is organized around three capability tiers, plus a `rules/` module containing pure game rules shared by all tiers:

- **rules** (all platforms): Pure functions and constants — damage formulas, balance constants, `MonsterKind` enum, `GameEvent` structured messages, `no_std` seed encoding. No game state interaction. AI decisions (`rules/ai.rs`), spawn selection (`rules/spawn.rs`), dungeon geometry (`rules/dungeon.rs`), and combat resolution (`rules/combat.rs`) are pure shared functions; each tier wraps them with its own state types.
- **tier micro** (C64): `u8` coords/stats, 64 entities, 64x48 maps, LFSR-16, iterative shadowcasting FOV, `no_std`
- **tier compact** (GBA, NDS): `i32` coords (ARM7-native), `u8` stats, 128 entities, 80x40 maps, LFSR-32, iterative integer shadowcasting FOV, `no_std` — built from standard tier patterns (same i32 coords) with fixed arrays instead of Vec
- **tier standard** (Vita/PC): `i32` coords/stats, 512-1024 entities, 80x40+ maps, ChaCha20, shadowcasting FOV, `std`

The distinction between **game rules** and **game mechanics** is key: rules are pure functions (damage calculation, item stat lookups, enchantment caps) that produce values; mechanics are stateful operations (applying damage to entities, inserting spawned monsters) that remain per-tier.

Tier micro and tier compact code live in `tier_micro/` and `tier_compact/`
respectively, and are always compiled. Standard-tier code (recursive shadowcasting
FOV, A\* pathfinding, `Vec`-based collections) is gated behind the `std` feature.
The C64 and GBA depend on core with `default-features = false`. Pure game rules
(damage, balance, items, AI decisions, spawn selection, dungeon geometry, combat
resolution, properties, interactions, seed encoding) live in `rules/` and are
used by all tiers:

```
roguelike/
  crates/
    core/           (roguelike-core)   # #![no_std] by default, optional std feature
      Cargo.toml
      src/
        lib.rs        # #![cfg_attr(not(feature = "std"), no_std)]
        rules/        # Pure functions + constants, always compiled, no_std
          mod.rs
          ai.rs       # ai_mode(), chase_step(), wander_step() — pure AI decisions
          balance.rs  # numeric constants: HP, ATK, DEF, spawn weights, regen,
                      #   item stats, depth scaling, wandering spawn config
          combat.rs   # resolve_melee() → CombatOutcome — shared combat resolution
          color.rs    # GameColor enum (#[repr(u8)]), palette management
          command.rs  # GameCommand enum — platform-independent input abstraction
          damage.rs   # const fn damage(), effective_attack(), effective_defense()
          direction.rs # Direction enum (#[repr(u8)]), to_offset/from_offset/from_index
          dungeon.rs  # rooms_intersect(), room_center(), corridor_between() — shared geometry
          game_view.rs # GameView trait — minimal query interface for all tiers
          health.rs   # HealthTier enum, health_tier(), health_description()
          interactions.rs # 38-rule interaction table, chain reactions (depth 3)
          items.rs    # ItemKind enum, stat lookup tables, Inventory, InvSlot
          message.rs  # GameEvent enum — structured message events (Copy, no_std)
          monster_table.rs # MonsterKind enum, stat lookup by kind
          properties.rs # Property enum (16 variants), PropertyBag (8 bytes packed nibbles)
          save_common.rs # Shared save format constants and helpers
          seed_code.rs # no_std encode/decode, Tier enum, tier_from_seed()
          spawn.rs    # weighted_select(), total_weight(), depth_bonus() — shared spawn math
          tiles.rs    # TileKind enum, pure tile display definitions (glyph, color)
        tier_micro/   # u8 types, LFSR-16, shadowcasting FOV, fixed arrays — no_std
          mod.rs
          types.rs    # Coord = u8, Stat = u8, Pos = (u8, u8)
          prng.rs     # LfsrRng — 16-bit Galois LFSR
          entity.rs   # fixed-size entity array (max 64 entities)
          game.rs     # MicroGameState with fixed-size arrays
          map.rs      # generate() on fixed-size tile arrays (64×48)
          fov.rs      # Iterative shadowcasting FOV → bitfield (Bresenham LOS)
          ai.rs       # monster turn orchestration (delegates to rules::ai)
          combat.rs   # melee attack resolution
          spawn.rs    # monster/item spawning (delegates to rules::spawn)
          item_store.rs # fixed-size item storage (floor items and inventory)
          pathfinding.rs # BFS pathfinding with fixed-size buffers (1.1 KB)
          autorun.rs  # MicroBfsStepper for BFS-guided autorun with stop conditions
          save.rs     # Binary save/load for MicroGameState
          msglog.rs   # Circular buffer for GameEvent values (no string formatting)
        tier_compact/ # Complete compact-tier engine — i32 coords, fixed arrays, no_std
          mod.rs
          types.rs    # Coord = i32 (ARM7-native), Stat = u8, 80×40 map, 128 entities
          prng.rs     # LfsrRng32 — 32-bit Galois LFSR
          entity.rs   # fixed-size entity array (max 128 entities)
          game.rs     # CompactGameState with fixed-size arrays, new_into() placement
          map.rs      # room generation, corridor carving on 80×40 tile arrays
          fov.rs      # Iterative shadowcasting FOV with i32 coords, Bresenham LOS
          ai.rs       # monster turn orchestration (delegates to rules::ai)
          combat.rs   # melee attack resolution
          spawn.rs    # monster/item spawning (delegates to rules::spawn)
          item_store.rs # fixed-size item storage (floor items and inventory)
          pathfinding.rs # BFS pathfinding with fixed-size buffers
          autorun.rs  # CompactBfsStepper for BFS-guided autorun with stop conditions
          save.rs     # Binary save/load for CompactGameState (GBA SRAM)
          msglog.rs   # Circular buffer for GameEvent values
        game_step.rs  # #[cfg(feature = "std")] trait GameStep — cross-tier interface;
                      #   MicroGameStateAdapter (u8→i32), CompactGameStateAdapter (native i32);
                      #   create_game() factory routes by seed tier
        # --- tier standard (top-level modules): i32 types, ChaCha, Vec — std ---
        types.rs      # Coord = i32, Stat = i32
        entity.rs     # Vec<Entity> (512-1024 entities)
        game.rs       # GameState with Vec-based collections
        ai.rs         # monster turn orchestration (delegates to rules::ai)
        fov.rs        # recursive shadowcasting FOV (gated behind std)
        pathfinding.rs # A* — requires HashMap, BinaryHeap (gated behind std)
        spawn.rs      # monster/item spawning (delegates to rules::spawn)
        ...           # other existing modules
```

```toml
# crates/core/Cargo.toml (tier system)
# Tier micro and rules are always compiled (no feature gate).
# Standard-tier code is gated behind `std`.
[package]
name = "roguelike-core"
version = "0.5.0"
edition = "2024"

[features]
default = ["dev-tools", "data-files", "serde", "std"]
std = ["dep:rand", "serde"]
serde = ["dep:serde", "dep:serde_json"]
data-files = ["dep:toml", "std"]
dev-tools = ["std"]
c64-overlay = []

[dependencies]
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
rand = { version = "0.9", optional = true }
toml = { version = "0.8", optional = true }
```

```rust
// crates/core/src/lib.rs (tier system)
#![cfg_attr(not(feature = "std"), no_std)]

// --- Always compiled (no_std compatible) ---
pub mod rules;         // pure game rules: ai, spawn, dungeon, combat, damage,
                       //   balance, items, properties, interactions, game_view,
                       //   seed_code, monster_table, GameEvent — no state interaction
pub mod command;       // re-exports GameCommand + Direction from rules
pub mod tier_micro;    // u8 types, LFSR-16, shadowcasting FOV, fixed arrays
pub mod tier_compact;  // i32 types, LFSR-32, 80×40 maps, 128 entities, fixed arrays

// --- Cross-tier interface (requires std) ---
#[cfg(feature = "std")]
pub mod game_step;     // GameStep trait — uniform interface for any tier's state

// --- Tier standard (requires std) ---
#[cfg(feature = "std")]
pub mod types;         // i32 coords, i32 stats
#[cfg(feature = "std")]
pub mod entity;        // Vec<Entity>
#[cfg(feature = "std")]
pub mod game;          // GameState with Vec-based collections
#[cfg(feature = "std")]
pub mod ai;
#[cfg(feature = "std")]
pub mod fov;           // shadowcasting FOV — Vita/PC
#[cfg(feature = "std")]
pub mod pathfinding;   // A* — requires HashMap, BinaryHeap
#[cfg(feature = "data-files")]
pub mod data;          // game.toml loading
```

Each tier uses concrete types appropriate to its capability level. Tier micro
uses `u8` coords and fixed-size arrays; tier compact uses `i32` coords
(ARM7-native) with `u8` stats and fixed arrays; tier standard uses `i32` types
and `Vec`-based collections. Game rules in `rules/` (AI decisions, spawn
selection, dungeon geometry, combat resolution, damage formulas, balance
constants, item definitions, properties, interactions, `GameView` trait,
`MonsterKind` enum, `GameEvent` messages, seed encoding) are pure functions and
constants — tier-agnostic, `no_std`, used by all platforms. Game mechanics
(entity iteration, state mutation, FOV computation) remain per-tier but delegate
algorithmic decisions to `rules/` via the scalar boundary pattern. The only
`cfg` gates are on `std` features (serde derives, TOML loading, A\* pathfinding,
`StdRng`) and the `GameStep` cross-tier trait.

### 1.2 Tier Micro PRNG: `LfsrRng`

The PRNG is the foundation of cross-platform seed sharing. Tier micro uses a
16-bit Galois LFSR; tier compact uses a 32-bit variant. Any platform running a
micro-tier seed uses `LfsrRng` — same seed, same random sequence, same dungeon.

```rust
// crates/core/src/tier_micro/prng.rs

/// 16-bit Galois LFSR pseudo-random number generator.
/// Polynomial: x^16 + x^14 + x^13 + x^11 + 1 (taps: 0xB400).
/// Maximal-length: cycles through all 65535 non-zero states.
pub struct LfsrRng {
    state: u16,
}

impl LfsrRng {
    pub const fn new(seed: u16) -> Self {
        Self { state: if seed == 0 { 0xACE1 } else { seed } }
    }

    pub fn next_u8(&mut self) -> u8 {
        let lsb = self.state & 1;
        self.state >>= 1;
        if lsb != 0 { self.state ^= 0xB400; }
        self.state as u8
    }

    pub fn next_u16(&mut self) -> u16 {
        let lo = self.next_u8() as u16;
        let hi = self.next_u8() as u16;
        (hi << 8) | lo
    }

    /// Random value in [min, max] inclusive. Rejection sampling for no bias.
    pub fn range(&mut self, min: u8, max: u8) -> u8 {
        if min >= max { return min; }
        let span = max - min + 1;
        let reject = (256u16 % span as u16) as u8;
        if reject == 0 {
            return min + (self.next_u8() % span);
        }
        let threshold = (256u16 - reject as u16) as u8;
        loop {
            let r = self.next_u8();
            if r < threshold {
                return min + (r % span);
            }
        }
    }

    /// 50/50 coin flip.
    pub fn coin(&mut self) -> bool { self.next_u8() & 1 != 0 }

    /// Current state (for seed display / save).
    pub fn state(&self) -> u16 { self.state }
}
```

On the C64, the `LfsrRng` struct (2 bytes) is passed as `&mut LfsrRng`,
replacing the POC's `static mut RNG_STATE` global. This is one of two
abstractions that genuinely improve 6502 code generation — see the
[C64 code style guide](../platforms/c64-platform-guide.md#6-c64-code-style-which-abstractions-help-on-the-6502) for why.

On PC/Vita, `LfsrRng` is used when playing a micro-tier seed (short seed codes
that are compatible with all platforms). Standard-tier seeds use ChaCha20 via
`StdRng`. The seed's numeric value determines the tier (`seed <= 0xFFFF` →
micro) — seed decode determines which tier to instantiate.

### 1.3 Tier Micro Map Generation

Map generation is per-tier (each tier operates on its own map representation).
The tier micro algorithm — random room placement with collision checks and
L-shaped corridor carving — operates on `&mut [u8]` (tile buffer) and
`&mut [Room]` (room list):

```rust
// crates/core/src/tier_micro/map.rs (mapgen section)

use crate::prng::LfsrRng;
use crate::room::Room;

pub const TILE_WALL: u8 = 0;
pub const TILE_FLOOR: u8 = 1;

/// Generate a dungeon into a flat tile buffer.
/// Returns (player_x, player_y, rooms_placed).
pub fn generate(
    tiles: &mut [u8],
    width: u8,
    height: u8,
    rooms: &mut [Room],
    rng: &mut LfsrRng,
    max_rooms: u8,
    min_size: u8,
    max_size: u8,
) -> (u8, u8, u8) {
    for t in tiles.iter_mut() { *t = TILE_WALL; }

    let mut room_count: u8 = 0;
    let mut start_x = width / 2;
    let mut start_y = height / 2;

    for _ in 0..max_rooms {
        let w = rng.range(min_size, max_size);
        let h = rng.range(min_size, max_size);
        if w + 2 >= width || h + 2 >= height { continue; }
        let x = rng.range(1, width - w - 2);
        let y = rng.range(1, height - h - 2);

        let new_room = Room { x, y, w, h };

        let mut overlaps = false;
        for i in 0..room_count {
            if new_room.intersects(&rooms[i as usize]) {
                overlaps = true;
                break;
            }
        }
        if overlaps { continue; }

        carve_room(tiles, width, &new_room);

        if room_count == 0 {
            start_x = new_room.cx();
            start_y = new_room.cy();
        } else {
            let prev = rooms[(room_count - 1) as usize];
            if rng.coin() {
                carve_h_tunnel(tiles, width, prev.cx(), new_room.cx(), prev.cy());
                carve_v_tunnel(tiles, width, prev.cy(), new_room.cy(), new_room.cx());
            } else {
                carve_v_tunnel(tiles, width, prev.cy(), new_room.cy(), prev.cx());
                carve_h_tunnel(tiles, width, prev.cx(), new_room.cx(), new_room.cy());
            }
        }

        rooms[room_count as usize] = new_room;
        room_count += 1;
    }

    (start_x, start_y, room_count)
}
```

This is the tier micro map generator — the C64 POC's `map::generate()` with
globals replaced by parameters. No generics, no trait bounds — just a function
that fills a byte slice and a room array. The compact and standard tiers have
their own `map::generate()` with wider coordinate types and larger map sizes.

Shared sub-algorithms live in `rules/dungeon.rs`: `rooms_intersect()` (AABB
overlap with wall padding), `room_center()`, and `corridor_between()` (returns
`CorridorSegment` pairs for L-shaped corridors). Each tier's map generator calls
these shared functions, widening coords at the call site if needed (micro widens
`u8` → `i32`), then carves the returned segments into its own tile storage.

### 1.4 Platform Usage Patterns

**C64 — production frontend over `tier_micro` + `rules`:**

```rust
// c64/src/main.rs (simplified)
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_core::command::GameCommand;

static mut STATE: MaybeUninit<MicroGameState> = MaybeUninit::uninit();

fn main() {
    // Hardware init...
    let seed = get_seed_from_user();
    unsafe { STATE.write(MicroGameState::new(seed)); }

    loop {
        render_viewport(unsafe { STATE.assume_init_ref() });  // C64-specific VIC-II rendering
        let cmd = read_input();   // Returns GameCommand/Direction from core
        unsafe { STATE.assume_init_mut() }.step(cmd);         // Tier micro game logic from core
    }
}
```

The C64 crate is a production frontend (~4,300 lines) — all game logic
comes from `roguelike-core::tier_micro` and `roguelike-core::rules`.
See [cross-platform architecture](../architecture/cross-platform.md) for the
full feature list. With llvm-mos's static stack allocation + LTO, `state` gets
a fixed static address — field access uses absolute addressing.

**GBA — dual-tier via EWRAM union:**

```rust
// gba/src/game_loop.rs (simplified)
use roguelike_core::tier_compact::game::CompactGameState;
use roguelike_core::tier_micro::game::MicroGameState;

// EWRAM union holds either CompactGameState or MicroGameState
static mut COMPACT_STATE: MaybeUninit<CompactGameState> = MaybeUninit::uninit();
static mut MICRO_STATE: MaybeUninit<MicroGameState> = MaybeUninit::uninit();
static mut IS_MICRO: bool = false;

fn start_game(seed: u64) {
    if seed <= 0xFFFF {
        unsafe { MICRO_STATE.write(MicroGameState::new(seed as u16)); }
        IS_MICRO = true;
    } else {
        unsafe { CompactGameState::new_into(COMPACT_STATE.as_mut_ptr(), seed as u32); }
        IS_MICRO = false;
    }
}
```

The GBA crate is a full production frontend (~2,800 lines) with animated title
screen, inventory UI, SRAM saves, pause/settings menus, help screen, message
history, and game over screens.

**NDS — hardware 3D over `tier_compact`:**

```rust
// nds/src/main.rs (simplified)
use roguelike_core::tier_compact::game::CompactGameState;

// CompactGameState lives in EWRAM, 3D rendering via DS GX hardware
static mut STATE: MaybeUninit<CompactGameState> = MaybeUninit::uninit();

fn main() {
    // ARM9 bootstrap: MPU, caches, VRAM banking, GX init
    unsafe { CompactGameState::new_into(STATE.as_mut_ptr(), seed); }
    // Top screen: hardware 3D (Engine A) — GX FIFO commands
    // Bottom screen: 2D automap + HUD + touch buttons (Engine B)
}
```

**PC/Vita — tier switching based on seed:**

```rust
// game_step.rs (simplified)
use roguelike_core::game_step::{create_game, GameStep};

fn start_game(seed: u64, w: i32, h: i32) -> Box<dyn GameStep> {
    // create_game() routes by seed tier automatically:
    //   seed <= 0xFFFF     → MicroGameStateAdapter (u8→i32 widening)
    //   seed <= 0xFFFFFFFF → CompactGameStateAdapter (native i32)
    //   else               → GameState (standard tier)
    create_game(seed, w, h, preset, game_data)
}
```

Seed decode determines which tier to instantiate. The numeric seed value
determines the tier (`seed <= 0xFFFF` → micro, `<= 0xFFFF_FFFF` → compact,
else → standard); encoding length is a consequence of numeric range, not the
detection mechanism. The PC runs micro seeds by instantiating `MicroGameState`
with an adapter (wrap, not reimplement) — producing the same dungeon a C64
would generate. The GBA runs both micro and compact seeds via an EWRAM union
with runtime tier selection. The PC gets additional features (serde
serialization, TOML loading, A\* pathfinding) via the `std` feature. The C64
and GBA use `default-features = false` and only access `tier_micro`,
`tier_compact`, and `rules`.

### 1.5 Sharing Matrix

| Category | Tier | Shared? | Where | Form |
|----------|------|---------|-------|------|
| **PRNG (16-bit)** | micro | Per-tier | `core/tier_micro/prng.rs` | `LfsrRng` — 16-bit Galois LFSR |
| **PRNG (32-bit)** | compact | Per-tier | `core/tier_compact/prng.rs` | `LfsrRng32` — 32-bit Galois LFSR |
| **PRNG (ChaCha20)** | standard | Per-tier | `rand` crate (std) | `StdRng` — cryptographic PRNG |
| **Map generation** | Per-tier | **Rules** + Per-tier | `core/rules/dungeon.rs` + `core/tier_*/map.rs` | Shared geometry (`rooms_intersect`, `room_center`, `corridor_between`); per-tier `generate()` loops (RNG coupling prevents full extraction) |
| **AI decisions** | All | **Rules** + Per-tier | `core/rules/ai.rs` + `core/tier_*/ai.rs` | `ai_mode()`, `chase_step()`, `wander_step()` shared; per-tier wrappers handle entity iteration and FOV |
| **Combat resolution** | All | **Rules** | `core/rules/combat.rs` | `resolve_melee()` → `CombatOutcome` with events |
| **Spawn selection** | All | **Rules** + Per-tier | `core/rules/spawn.rs` + `core/tier_*/spawn.rs` | `weighted_select()`, `total_weight()`, `depth_bonus()` shared; per-tier wrappers apply to own state |
| **Damage formula** | All | **Rules** | `core/rules/damage.rs` | `const fn damage(atk: u8, def: u8) -> u8`, `effective_attack()`, `effective_defense()` |
| **Balance constants** | All | **Rules** | `core/rules/balance.rs` | All HP/ATK/DEF/sight/spawn_weight/regen values |
| **Item definitions** | All | **Rules** | `core/rules/items.rs` | Item type IDs, stat lookup tables (heal amount, ATK/DEF bonus, spawn weights) |
| **Inventory** | All | **Rules** | `core/rules/items.rs` | `Inventory` struct (26-slot Brogue-style, slots a–z), `InvSlot` type with stacking; `MAX_INVENTORY = 26` |
| **Properties** | All | **Rules** | `core/rules/properties.rs` | `Property` enum (16 variants), `PropertyBag` (8 bytes packed nibbles) |
| **Interactions** | All | **Rules** | `core/rules/interactions.rs` | 38-rule interaction table, chain reactions (depth 3), `Effect`/`EffectType` |
| **Depth scaling** | All | **Rules** | `core/rules/balance.rs` | Monster stat scaling per floor, `min_depth` thresholds |
| **Wandering spawn config** | All | **Rules** | `core/rules/balance.rs` | Spawn interval, delay, max active constants |
| **Monster table** | All | **Rules** | `core/rules/monster_table.rs` | `MonsterKind` enum, stat lookup by kind |
| **GameEvent messages** | All | **Rules** | `core/rules/message.rs` | `GameEvent` enum — structured, `Copy`, `no_std` |
| **Seed codes** | All | **Rules** | `core/rules/seed_code.rs` | `no_std` `encode_to_buf()`/`decode_from_bytes()`, `Tier` enum; `core/seed_code.rs` has std `SeedParams`/format parsing |
| **Direction** | All | **Rules** | `core/rules/direction.rs` | `Direction` enum, `to_offset()`/`from_offset()`/`from_index()`/`opposite()` |
| **Tile display** | All | **Rules** | `core/rules/tiles.rs` | `TileKind` enum, pure glyph/color lookups shared across tiers |
| **Color system** | All | **Rules** | `core/rules/color.rs` | `GameColor` enum (`#[repr(u8)]`), palette management |
| **Health display** | All | **Rules** | `core/rules/health.rs` | `HealthTier` enum, `health_tier()`, `health_description()` |
| **GameStep trait** | std | **Cross-tier** | `core/game_step.rs` | `#[cfg(feature = "std")]` — uniform interface; `MicroGameStateAdapter` + `CompactGameStateAdapter` wrap lower tiers |
| **GameView trait** | All | **Cross-tier** | `core/rules/game_view.rs` | Minimal query interface for rendering and MCP — implemented by all three tiers |
| **Entity system** | Per-tier | Per-tier | `core/tier_*/entity.rs` | micro: 64-entry parallel arrays (`EntityStore`); compact: 128-entry parallel arrays (`EntityStore`); standard: `Vec<Entity>` |
| **Game state** | Per-tier | Per-tier | `core/tier_*/game.rs` | `MicroGameState` / `CompactGameState` / `GameState` |
| **FOV** | Per-tier | Per-tier | `core/tier_*/fov.rs` | micro/compact: iterative shadowcasting with integer slopes → bitfield (`no_std`); standard: recursive shadowcasting with `f64` slopes → `HashSet` (`std`). Constrained tiers use iterative because standard's recursive impl depends on `f64` and heap — not available in `no_std`. |
| **BFS pathfinding** | micro, compact | Per-tier | `core/tier_*/pathfinding.rs` | Fixed-size buffers, enables auto\_explore and pathfind\_to on micro and compact tiers |
| A\* pathfinding | standard | **No** | PC only (`std`) | Requires heap (HashMap, BinaryHeap) |
| Rendering | N/A | **No** | Separate impls | crossterm vs VIC-II vs GBA Mode 0 vs NDS GX vs vita2d |
| Input handling | N/A | **No** | Separate impls | crossterm vs CIA keyboard/joystick vs GBA joypad vs NDS touch vs Vita buttons |
| Save persistence | N/A | **No** | Separate impls | JSON vs binary; different backends (filesystem, SRAM, floppy) |
| Data loading | standard | **No** | PC only (`data-files`) | TOML parse; C64/GBA/NDS use compiled-in balance constants |

### 1.6 Balance Constants

**Target file:** `crates/core/src/rules/balance.rs`

Core's balance module is the single source of truth for game balance. All tiers
and platforms use the same constants directly. The module defines:

- **Monster stats** (per kind: Goblin, Orc, Troll): HP, ATK, DEF, sight range, spawn weight — all `u8`
- **Player defaults**: HP (`30`), ATK (`5`), DEF (`2`) — all `u8`
- **Config constants**: regen interval, max monsters per room — `u8`
- **Per-tier map dimensions**: micro (`64×48`, 12 rooms, 64 entities), compact (`80×40`, 12 rooms, 128 entities), standard (`80×40`) — `u8`
- **Wandering spawn** (Phase 1): spawn interval (`40`), delay (`60`), max active (`5`) — `u8`
- **Depth scaling** (Phase 2): target depth (`5`), HP per floor (`+1`), ATK per 2 floors (`+1`) — `u8`
- **Enchantment** (Phase 4): max enchant level (`5`), enchantment bonus per level (`+1`) — `u8`
- **Mood thresholds** (Phase 5): flee (`-50`), disengage (`-20`), enrage (`80`), ally-dies/takes-hit/lands-hit/low-HP triggers — `i8`

All balance types are `u8` or `i8` — values that fit in the smallest tier's
natural width. These constants are shared across all tiers; each tier's game
state uses its own coordinate and stat types, but balance values are universal.

**Relationship to `game.toml`**: The PC's data-file loading (`data.rs`, gated
behind `#[cfg(feature = "data-files")]`) will use these balance constants as
compiled-in defaults. Modders can still override values via `game.toml` — the
constants are the baseline, not a constraint. A CI test verifies that
`game.toml` defaults match `roguelike_core::rules::balance` constants.

**Relationship to gameplay implementation plan**: The constants above correspond
to Phases 1 (wandering spawns), 2 (depth scaling), 4 (enchantment), and 5 (mood)
of the [gameplay implementation plan](../design/gameplay-implementation-plan.md).
Phase 3 (items) and Phase 4 (item-based progression, including enchantment scrolls
and permanent consumables) share the items module (`core/rules/items.rs`) which
includes lookup tables for stat bonuses, enchantment caps, and permanent consumable
effects. Phase 6 (property bitfields) is standard-tier only for now — tier micro
has no immediate use for the `u64` property system, though a `u8` subset could be
added later.

### 1.7 Item Definitions

**File:** `crates/core/src/rules/items.rs`

Core's items module defines the `ItemKind` enum and stat lookup tables as
`const fn` functions — shared across all tiers. The standard tier's `item.rs`
builds on these for the full item system (inventory, floor items, equipment).

The `ItemKind` enum and pure lookup functions (glyph, color, stat bonuses)
are `no_std` compatible. The `Inventory` struct (26-slot Brogue-style with
letter-based slots a–z) and `InvSlot` type with stacking logic also live here,
shared across all tiers. Both micro tier (`MicroGameState`) and standard tier
(`GameState`) use the same `Inventory` struct for pickup, use, drop, and equip
operations. The standard tier adds `Item` struct, `Equipment`, and `EquipSlot`
types for the full equipment system (`crates/core/src/item.rs`, gated behind `std`).

The `effective_attack()` and `effective_defense()` helpers in
`roguelike_core::rules::damage` take base stats plus equipment bonuses and
return the effective values. Item spawn weights and `min_depth` thresholds
are data-driven via `[[items]]` entries in `game.toml`.

### 1.8 Tier Comparison

Each tier defines its own types, algorithms, and storage representations:

| Aspect | Tier micro | Tier compact | Tier standard |
|--------|-----------|-------------|--------------|
| Coord | `u8` | `i32` (ARM7-native) | `i32` |
| Stat | `u8` | `u8` | `i32` |
| Entity storage | 64 (fixed array) | 128 (fixed array) | 512-1024 (Vec) |
| Map | 64x48, flat `[u8; W*H]` | 80x40, flat `[u8; W*H]` | 80x40+, `Vec<Vec<Tile>>` |
| PRNG | LFSR-16 | LFSR-32 | ChaCha20 |
| FOV | Iterative shadowcasting (integer slopes, 6502-optimized) | Iterative shadowcasting (integer slopes, i32, bitfield — fresh rewrite, not adapted from micro) | Recursive shadowcasting (`f64` slopes, `HashSet`) |
| Pathfinding | BFS (fixed buffers) | BFS (fixed buffers) | A* |
| Messages | GameEvent enum (Copy) | GameEvent enum (Copy) | GameEvent → String formatting |
| Enchantment cap | 5 (u8) | 5 (u8) | 5 (configurable) |
| Save format | Binary (platform-specific) | Binary (platform-specific) | JSON via serde |

All three tiers are fully implemented. The compact tier uses no heap allocator —
fixed arrays and `no_std` integer algorithms throughout. This was an explicit
architectural decision for the GBA port (see #206, resolved).

When a higher platform runs a lower-tier game, it uses the lower tier's types
and algorithms. Tier compatibility is downward only — higher tiers play
lower-tier seeds, not vice versa. A PC running a tier micro seed instantiates
`MicroGameState` with an adapter (wrap, not reimplement) — using `u8` coords,
LFSR-16, and iterative shadowcasting FOV to produce the same dungeon a C64
would generate. The GBA runs both micro seeds (via `MicroGameState`) and compact
seeds (via `CompactGameState`) through an EWRAM union with runtime tier
selection. The NDS runs compact seeds only. Standard seeds are not supported on
GBA or NDS.

### 1.9 Seed System and Cross-Platform Seeds

Seeds are clean — no tier prefix. The game infers the tier from the seed's
numeric value and shows platform compatibility in the UI.

#### Seed Numeric Value → Tier Mapping

The decoded numeric value of the seed determines the tier:

- **seed <= 0xFFFF** (u16 range) → **micro**: 64×48, LFSR-16, iterative shadowcasting FOV
- **seed <= 0xFFFFFFFF** (u32 range) → **compact**: 80×40, LFSR-32, iterative shadowcasting FOV
- **seed > 0xFFFFFFFF** (u64 range) → **standard**: 80×40, ChaCha20, shadowcasting FOV

The base36 encoding length is a *consequence* of the numeric range, not the
detection mechanism. A seed that decodes to `0x1A3F` is micro regardless of
whether it was entered as `r7z` or with leading zeros. Detection uses the
numeric value directly: `seed <= 0xFFFF` → micro.

```
Seed: r7z              (value: 0x2E93, fits u16)
Plays on: C64 · GBA · NDS · Vita · PC
Map: 64×48 · 64 entities · Iterative shadowcasting FOV

Seed: r7z3kq           (value: 0x3A1B_C4E2, fits u32)
Plays on: GBA · NDS · Vita · PC
Map: 80×40 · 128 entities · Iterative shadowcasting FOV

Seed: r7z3kq9ab2x      (value: 0x1F3C_7A2B_0E91_D4F6, u64)
Plays on: Vita · PC
Map: 80×40 · 512 entities · Shadowcasting FOV
```

The seed code format (`<base36_seed>[-<WxH>][<preset>]`) continues to support
custom dimensions and map presets for standard-tier seeds. The dimension suffix
only applies within tier standard — micro and compact tiers have fixed
dimensions as part of the tier specification.

#### Choosing Compatibility at New Game Start

On platforms above micro, the new game screen offers a compatibility choice:

```
New Game
  Standard (80×40, full features)
  Compact  (80×40, GBA-compatible)
  Micro    (64×48, plays on everything)    ← cross-platform daily challenges use this
```

This is not a "challenge mode" — it's a compatibility choice. The player
selects how widely their seed should be playable. On seed entry, the tier is
auto-detected from the seed's numeric value.

#### Daily Seeds and Leaderboards

The daily seed server sends a micro-tier u16 seed. All platforms generate
identically — same PRNG, same map, same FOV, same entity count. No divergence
within a tier.

Leaderboards are per-tier. A micro-tier daily challenge leaderboard includes
scores from C64, GBA, NDS, Vita, and PC players competing on the same dungeon
with the same algorithms. Standard-tier leaderboards include Vita and PC players.

### 1.10 Tier Divergence

All intentional tier differences are documented in the comparison table in §1.8
above. These reflect principled trade-offs for each tier's hardware constraints,
not bugs or limitations.

The `rules/` module provides both tier-independent data (damage amounts, item
stats, monster stats, balance constants) and shared algorithmic logic (AI
decisions, spawn selection, dungeon geometry, combat resolution) that all tiers
use identically. Per-tier code extracts scalars from its data structures, calls
the shared `rules/` functions, and applies the results back — the "scalar
boundary pattern." Divergence occurs in how tiers *store and iterate* their
state — entity arrays, map storage layout, FOV computation, message formatting,
and save serialization remain per-tier mechanics.
