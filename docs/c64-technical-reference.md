# C64 Port: Technical Reference

**Companion document to the [C64 Port Proposal](c64-port-proposal.md).**
Contains implementation details, code listings, and platform-specific guidance
extracted from the proposal for reference during development.

---

## 1. Shared Crate: `roguelike-rules`

### 1.1 Crate Structure

```
roguelike/
  crates/
    rules/          (roguelike-rules)   # #![no_std], zero deps
      Cargo.toml
      src/
        lib.rs        # re-exports
        prng.rs       # LfsrRng — Galois LFSR (deterministic, shared)
        room.rs       # Room struct, intersection, center
        balance.rs    # numeric constants: HP, ATK, DEF, spawn weights, regen,
                      #   item stats, XP tables, depth scaling, wandering spawn
                      #   config, mood thresholds (see gameplay-implementation-plan.md)
        combat.rs     # const fn damage formula + effective_attack/defense helpers
        mapgen.rs     # generate() on &mut [u8] + &mut [Room]
        spawn.rs      # pick_monster(), spawn_into_rooms()
        items.rs      # item type IDs, stat lookup tables (heal amount, ATK/DEF bonus)
        leveling.rs   # xp_for_level() table, stat growth per level
        structural.rs # compute_structural_walls() on &[u8] → bitfield
        seed.rs       # seed code encode/decode (no_std, fixed buffers)
```

```toml
# crates/rules/Cargo.toml
[package]
name = "roguelike-rules"
version = "0.1.0"
edition = "2021"

# Zero dependencies — compiles on host, MOS, thumbv6m, anything
```

```rust
// crates/rules/src/lib.rs
#![no_std]

pub mod prng;
pub mod room;
pub mod balance;
pub mod combat;
pub mod mapgen;
pub mod spawn;
pub mod items;
pub mod leveling;
pub mod structural;
pub mod seed;

#[cfg(test)]
extern crate std;
```

**~550 lines total.** Every function takes concrete types and produces concrete
outputs. No `alloc`, no `serde`, no `cfg` switches. The gameplay modules
(`items`, `leveling`) follow the same pattern as the core modules: `u8` types,
`const` lookup tables, concrete functions. Both platforms import the same
balance data — the C64 uses it directly, the PC promotes to its own types.

### 1.2 Shared PRNG: `LfsrRng`

The PRNG is the foundation of cross-platform seed sharing. Both platforms use
the same 16-bit Galois LFSR — same seed, same random sequence, same dungeon.

```rust
// roguelike-rules/src/prng.rs

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
abstractions that genuinely improve 6502 code generation — see [§2.5](#25-c64-code-style-which-abstractions-help-on-the-6502) for why.

On the PC, `LfsrRng` is used for "challenge mode" ([proposal §6.14](c64-port-proposal.md#614-seed-system-and-cross-platform-seed-sharing)) while `StdRng`
remains the default for normal gameplay.

### 1.3 Shared Map Generation

The map generation algorithm is identical on both platforms — random room
placement with collision checks and L-shaped corridor carving. The shared
function operates on `&mut [u8]` (tile buffer) and `&mut [Room]` (room list):

```rust
// roguelike-rules/src/mapgen.rs

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

This is essentially the C64 POC's `map::generate()` with globals replaced by
parameters. No generics, no trait bounds — just a function that fills a byte
slice and a room array.

### 1.4 How Each Platform Uses Shared Code

**C64 — direct, zero-cost:**

```rust
// c64/src/map.rs
use roguelike_rules::{mapgen, room::Room, balance, prng::LfsrRng};

static mut TILES: [u8; 840] = [0; 840];
static mut ROOMS: [Room; 12] = [Room::EMPTY; 12];
static mut ROOM_COUNT: u8 = 0;

pub fn generate(rng: &mut LfsrRng) -> (u8, u8) {
    let (px, py, rc) = unsafe {
        mapgen::generate(
            &mut TILES, balance::C64_MAP_WIDTH, balance::C64_MAP_HEIGHT,
            &mut ROOMS, rng, 12, 3, 7,
        )
    };
    unsafe { ROOM_COUNT = rc; }
    compute_structural_walls(); // C64-specific bitfield version
    (px, py)
}
```

The C64 still owns its `static mut` storage. The shared function just fills it.

**PC — wraps shared code for challenge mode:**

```rust
// engine/src/map.rs (addition, not replacement)
use roguelike_rules::{mapgen, room::Room as SharedRoom, prng::LfsrRng};

impl Map {
    /// Generate a C64-compatible map (for cross-platform seed challenges).
    /// Same seed + same LFSR + same function = identical dungeon.
    pub fn generate_c64_compatible(&mut self, seed: u16) -> Pos {
        use roguelike_rules::balance;
        let mut rng = LfsrRng::new(seed);
        let w = balance::C64_MAP_WIDTH;
        let h = balance::C64_MAP_HEIGHT;
        let mut tiles_u8 = vec![0u8; (w as usize) * (h as usize)];
        let mut rooms_buf = [SharedRoom::EMPTY; 12];

        let (px, py, rc) = mapgen::generate(
            &mut tiles_u8, w, h,
            &mut rooms_buf, &mut rng, 12, 3, 7,
        );

        // Promote u8 tiles → Tile enum
        for (i, &t) in tiles_u8.iter().enumerate() {
            self.tiles[i] = if t == mapgen::TILE_FLOOR { Tile::Floor } else { Tile::Wall };
        }
        for i in 0..rc as usize {
            let r = rooms_buf[i];
            self.rooms.push(Rect::new(r.x as Coord, r.y as Coord,
                                       r.w as Coord, r.h as Coord));
        }

        self.compute_structural_walls();
        (px as Coord, py as Coord)
    }
}
```

The PC keeps its existing `Map::generate()` with `StdRng` for normal play.
The shared function only activates for cross-platform challenges. No
abstractions pollute the normal code path.

### 1.5 Sharing Matrix

| Category | Shared? | Where | Form |
|----------|---------|-------|------|
| **PRNG** | **Yes** | `rules/prng.rs` | `LfsrRng` struct — same algorithm on both platforms |
| **Map generation** | **Yes** | `rules/mapgen.rs` | `generate()` on `&mut [u8]` + `&mut [Room]` |
| **Room geometry** | **Yes** | `rules/room.rs` | `Room { x, y, w, h }`, `intersects()`, `center()` |
| **Combat formula** | **Yes** | `rules/combat.rs` | `const fn damage(atk: u8, def: u8) -> u8`, `effective_attack()`, `effective_defense()` |
| **Monster spawning** | **Yes** | `rules/spawn.rs` | `pick_monster()`, `spawn_into_rooms()` |
| **Structural walls** | **Yes** | `rules/structural.rs` | `compute()` on `&[u8]` → `&mut [u8]` bitfield |
| **Balance constants** | **Yes** | `rules/balance.rs` | All HP/ATK/DEF/sight/spawn_weight/regen values |
| **Item definitions** | **Yes** | `rules/items.rs` | Item type IDs, stat lookup tables (heal amount, ATK/DEF bonus, spawn weights) |
| **Leveling tables** | **Yes** | `rules/leveling.rs` | XP thresholds per level, HP/ATK/DEF growth per level |
| **Depth scaling** | **Yes** | `rules/balance.rs` | Monster stat scaling per floor, `min_depth` thresholds |
| **Wandering spawn config** | **Yes** | `rules/balance.rs` | Spawn interval, delay, max active constants |
| **Mood thresholds** | **Yes** | `rules/balance.rs` | Mood trigger values, decay rate, flee/enrage thresholds |
| **Seed codes** | **Yes** | `rules/seed.rs` | Encode/decode with `[u8; 16]` fixed buffers |
| FOV (compute_fov) | **No** | Separate impls | PC: f64 shadowcasting → HashSet; C64: Bresenham → bitfield |
| FOV (can_see) | **No** | Separate impls | Intentionally different — see [proposal §6.3](c64-port-proposal.md#63-field-of-view) |
| A* pathfinding | **No** | PC only | Requires heap (HashMap, BinaryHeap) |
| Rendering | **No** | Separate impls | crossterm vs VIC-II screen writes |
| Input handling | **No** | Separate impls | crossterm vs CIA keyboard/joystick |
| Save persistence | **No** | Separate impls | JSON vs binary; different backends |
| Data loading | **No** | Separate impls | TOML parse vs ROM constants |
| Entity storage | **No** | Separate impls | `Vec<Entity>` vs parallel arrays |
| Item storage | **No** | Separate impls | `HashMap<Pos, Vec<Item>>` vs sparse parallel arrays (see [proposal §6.7](c64-port-proposal.md#67-items-and-inventory-c64-storage-design)) |
| Message log | **No** | Separate impls | `Vec<String>` vs `[u8; 160]` circular buffer |

### 1.6 Shared Balance Constants

The shared crate is the single source of truth for game balance. Both platforms
import constants rather than defining them independently:

```rust
// roguelike-rules/src/balance.rs

// --- Monster stat constants ---
pub const GOBLIN_HP: u8 = 6;
pub const GOBLIN_ATK: u8 = 3;
pub const GOBLIN_DEF: u8 = 0;
pub const GOBLIN_SIGHT: u8 = 6;
pub const GOBLIN_SPAWN_WEIGHT: u8 = 60;
pub const GOBLIN_XP: u8 = 5;

pub const ORC_HP: u8 = 12;
pub const ORC_ATK: u8 = 4;
pub const ORC_DEF: u8 = 1;
pub const ORC_SIGHT: u8 = 7;
pub const ORC_SPAWN_WEIGHT: u8 = 30;
pub const ORC_XP: u8 = 15;

pub const TROLL_HP: u8 = 20;
pub const TROLL_ATK: u8 = 6;
pub const TROLL_DEF: u8 = 3;
pub const TROLL_SIGHT: u8 = 5;
pub const TROLL_SPAWN_WEIGHT: u8 = 10;
pub const TROLL_XP: u8 = 40;

// --- Player defaults ---
pub const PLAYER_HP: u8 = 30;
pub const PLAYER_ATK: u8 = 5;
pub const PLAYER_DEF: u8 = 2;

// --- Config constants ---
pub const REGEN_INTERVAL: u8 = 3;
pub const MAX_ROOMS_C64: u8 = 12;
pub const MAX_MONSTERS_PER_ROOM: u8 = 2;
pub const C64_MAP_WIDTH: u8 = 64;
pub const C64_MAP_HEIGHT: u8 = 48;

// --- Wandering spawn (gameplay-implementation-plan.md Phase 1) ---
pub const WANDERING_SPAWN_INTERVAL: u8 = 40;
pub const WANDERING_SPAWN_DELAY: u8 = 60;
pub const WANDERING_MAX_ACTIVE: u8 = 5;

// --- Depth scaling (gameplay-implementation-plan.md Phase 2) ---
pub const TARGET_DEPTH: u8 = 10;
pub const MONSTER_HP_PER_FLOOR: u8 = 1;
// ATK scaling uses integer math: +1 ATK every 2 floors
pub const MONSTER_ATK_PER_2_FLOORS: u8 = 1;

// --- Leveling (gameplay-implementation-plan.md Phase 4) ---
pub const XP_PER_LEVEL: [u8; 10] = [0, 20, 50, 100, 180, 255, 255, 255, 255, 255];
// Note: PC uses Stat (i32) for XP thresholds above 255; C64 caps at u8.
// Levels 6+ on C64 use the max u8 value — effectively unreachable on a
// single floor, requiring deeper descent. This is an acceptable divergence.
pub const HP_PER_LEVEL: u8 = 5;
pub const ATK_PER_LEVEL: u8 = 1;
pub const DEF_PER_2_LEVELS: u8 = 1;

// --- Mood (gameplay-implementation-plan.md Phase 5) ---
pub const MOOD_FLEE_THRESHOLD: i8 = -50;
pub const MOOD_DISENGAGE_THRESHOLD: i8 = -20;
pub const MOOD_ENRAGE_THRESHOLD: i8 = 80;
pub const MOOD_ALLY_DIES_SAME: i8 = -30;
pub const MOOD_ALLY_DIES_OTHER: i8 = -15;
pub const MOOD_TAKES_HIT: i8 = -5;
pub const MOOD_LANDS_HIT: i8 = 10;
pub const MOOD_LOW_HP: i8 = -20;
```

All types are `u8` or `i8` — the C64's natural width. The PC promotes to
`Stat` (`i32`) at the boundary when loading into `GameData`. The C64 uses the
values directly in its ROM stat tables.

**Relationship to `game.toml`**: The PC's data-file loading (`data.rs`) uses
these shared constants as compiled-in defaults. Modders can still override
values via `game.toml` — the shared crate is the baseline, not a constraint.
A CI test verifies that `game.toml` defaults match `roguelike-rules` constants.

**Relationship to gameplay implementation plan**: The constants above correspond
to Phases 1 (wandering spawns), 2 (depth scaling), 4 (leveling), and 5 (mood)
of the [gameplay implementation plan](design/gameplay-implementation-plan.md).
Phase 3 (items) has its own module (`rules/items.rs`) rather than constants in
`balance.rs`, because item definitions include lookup tables for stat bonuses.
Phase 6 (property bitfields) is PC-only for now — the C64 has no immediate use
for the `u64` property system, though a `u8` subset could be added later.

---

## 2. Technical Notes

### 2.1 PRNG: Lessons from the POC

The POC's `prng::range()` function had a critical bug: rejection sampling to
avoid modulo bias used `(256u16 - (256u16 % span)) as u8`, which overflows to
0 when span evenly divides 256 (span = 2, 4, 8, 16...). This caused infinite
loops during `spawn_monsters()` for rooms with odd widths/heights (span 2 or 4).

**Fix:** Early return when `256 % span == 0` (no bias exists, accept any value).

**Lesson:** The 8-bit boundary creates overflow traps that don't exist on 32/64-
bit targets. Every arithmetic expression must be audited for u8/u16 overflow.
Rust's type system catches many of these at compile time, but `as` casts are
silent truncation.

The production build moves the PRNG into `roguelike-rules::prng::LfsrRng`,
which carries this fix and is tested by the shared crate's property tests
(LFSR period verification, range distribution checks).

### 2.2 Input: CIA Port Multiplexing

CIA1 Port A ($DC00) is shared between:
- Keyboard column selection (output, active LOW)
- Joystick Port 2 (input, active LOW, overrides output drivers)

CIA1 Port B ($DC01) is shared between:
- Keyboard row results (input, active LOW)
- Joystick Port 1 (input, active LOW, bits 0-4)

**Consequence:** Direct keyboard matrix scanning (`write $00 to Port A, read
Port B`) picks up joystick Port 1 signals on bits 0-4. On emulators where
virtual controls map to Port 1, this appears as "key always pressed" and
creates infinite release-wait loops.

**Solution:** Use the Kernal keyboard buffer ($C6/$0277) for keyboard input
(the Kernal IRQ handler properly manages the multiplexing) and read Port A
with columns deselected ($FF) for joystick Port 2. Never read Port B
directly for input detection.

### 2.3 Static Stack Allocation

llvm-mos's static stack allocation is the key to acceptable performance. For
it to work optimally:

- **Avoid recursion** — all game algorithms are iterative (Bresenham FOV,
  greedy AI, room placement loop).
- **Minimize function pointers** — trait objects and dynamic dispatch prevent
  call graph analysis. Use static dispatch via generics.
- **Avoid trait-heavy abstractions** — generic functions with trait bounds
  create indirect references that can obstruct call graph analysis. The shared
  crate uses concrete types (`u8`, `&mut [u8]`, `&mut LfsrRng`) instead of
  trait bounds, ensuring the call graph is fully visible at link time.
- **Enable LTO** — whole-program optimization is required for static stack
  allocation to analyze the complete call graph.
- **Prefer `&mut` parameters over `static mut` globals** — Rust's borrow
  checker guarantees that `&mut` references don't alias, which helps the
  compiler reason about what memory is touched by each function call. See [§2.5](#25-c64-code-style-which-abstractions-help-on-the-6502).

### 2.4 Turn Timing and Cycle Budgets

Total turn processing measured on the POC (full redraw, no dirty-rect):
~20,000 cycles = ~20 ms. With dirty-rectangle rendering (~500 cycles) and
amortized AI costs, this drops to ~8,000 cycles = ~8 ms. Well under one
frame (16.7 ms NTSC / 20 ms PAL).

**Continuous per-frame raster overhead:** In addition to per-turn costs, the
raster interrupt chain ([proposal §8](c64-port-proposal.md#8-implementation-plan), Phase 2a step 7) introduces a continuous background
cost that runs every frame regardless of player input. See the
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
§Cycle Budget Impact for the complete breakdown.

```
Raster interrupt chain:                 ~200 cycles (3-zone split)
  - Background color gradient:          ~110 cycles (22 lines × 5 cyc)
  - Border removal:                      ~20 cycles (2 register writes)
  - SID player callback:                ~300 cycles (typical music driver)
  - Charset zone switching:              ~30 cycles (2 switches)
Total continuous overhead:              ~660 cycles/frame (~3.4%)
```

With dirty-rect rendering and raster effects combined:

```
Available per frame (PAL):             ~19,656 cycles
Continuous raster overhead:               ~660 cycles
Remaining for game logic:              ~18,996 cycles/frame
Per-turn costs (on player action):
  FOV:                                  ~7,500 cycles
  Dirty-rect render:                      ~500 cycles
  AI + combat:                          ~2,000 cycles
  Total per-turn:                      ~10,000 cycles (0.5 frames)
```

Plenty of headroom for sprite animation, smooth scrolling, and future gameplay
features. The continuous raster cost is independent of per-turn costs — it runs
in the background via interrupt while the game loop blocks on input.

### 2.5 C64 Code Style: Which Abstractions Help on the 6502

The C64 crate should feel like a C64 program written in Rust — idiomatic for
the hardware, not idiomatic for modern Rust. Most Rust abstractions add
overhead on the 6502 that doesn't exist on modern CPUs. This section documents
which patterns help and which hurt.

**Abstractions that help:**

| Pattern | Why | Cost |
|---------|-----|------|
| **`LfsrRng` struct (shared crate)** | Explicit data flow via `&mut LfsrRng` helps llvm-mos's alias analysis. The compiler can prove RNG calls don't affect tile arrays or entity arrays, enabling better optimization of surrounding code. Also required for the shared crate. | ~2 cycles/call for indirect addressing (zero-page pair) vs absolute. Negligible. |
| **Explicit parameters to AI** | Pass player position `(px, py)` to `run_monster_turns()` instead of reading from entity globals inside the function. Makes data flow visible, helps optimizer, makes the code easier to follow. | ~0 cost — 2 bytes on zero page. |
| **Module-level accessor functions** | `entity::x(i)`, `entity::hp(i)` etc. provide a clean interface around `static mut` arrays without struct overhead. The module boundary is the abstraction. | Zero — inlines to `LDA base,X`. |

**Abstractions that hurt or don't help:**

| Pattern | Why Not | 6502 Impact |
|---------|---------|-------------|
| **Entity struct wrapping parallel arrays** | `static mut ENT_X: [u8; 16]` produces `LDA ENT_X,X` — a single absolute indexed load (4 cycles). Struct field access through a pointer uses `LDA (zp),Y` (5 cycles). With LTO+inlining it probably optimizes to the same code, but "probably" is risky on the less-mature llvm-mos. | 1 cycle/access risk in hot loops |
| **`#[repr(u8)]` enums for AI/entity types** | Current `match behavior { entity::AI_CHASE => ... }` with `u8` constants generates `CMP #1 / BEQ`. A proper enum `match` should compile identically but risks branch tables on the immature compiler. The safety benefit is small in a ~2000-line codebase. | Potential code size increase |
| **Newtype wrappers** (`EntityIdx(u8)`) | Requires the compiler to prove transparency for every access. On upstream LLVM this is trivial; on llvm-mos it's unnecessary risk. | Potential codegen regression |
| **`&mut GameState` through call chain** | Kills absolute addressing — the 6502's fastest mode. A single state pointer means all field accesses go through `LDA (zp),Y` instead of `LDA absolute,X`. The 1-cycle penalty accumulates: in a loop over 16 entities checking 3-4 fields each, that's ~100 extra cycles per monster turn. | ~0.5% of turn budget |
| **Wrapping `static mut` in a struct** | `static mut ENTS: EntityStore { x: [u8; 16], ... }` doesn't change memory layout, but `ENTS.x[i]` requires the compiler to resolve the field offset. With LTO this should optimize away. Without, you pay for the offset calculation. | Marginal risk, marginal benefit |

**Summary:** The C64 crate uses two targeted abstractions (`LfsrRng` struct,
explicit AI parameters) and keeps everything else flat: `static mut` parallel
arrays, `u8` constants, module-level functions, liberal `unsafe`. The module
system provides clean organization without runtime cost. Hot paths (rendering,
FOV) use raw pointer arithmetic and `write_volatile` per the chirp8-c64
recommendation.
