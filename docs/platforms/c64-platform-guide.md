# C64 Platform Guide

**Hardware-specific technical guidance for the Commodore 64 port.**
Covers the C64 module mapping, 6502 code generation patterns, CIA hardware,
cycle budgets, and which Rust abstractions help or hurt on the 6502. For the
cross-platform tier architecture, see the
[capability tier reference](../architecture/capability-tier-reference.md). For the port
proposal and implementation plan, see the
[C64 port proposal](c64-port-proposal.md).

---

## 1. C64 Module Mapping

Most modules are used directly from core. Tier-specific modules (map, FOV,
pathfinding, entity storage) use the algorithms defined by each tier. The C64
crate is a thin frontend — only rendering, input, sound, saves, and hardware
access are C64-specific.

```
Core Module                  →  C64 Usage                    Tier     Notes
───────────────────────────────────────────────────────────────────────────────────────
core/src/rules/damage.rs     →  used directly from core      Rules    damage formula
core/src/rules/balance.rs    →  used directly from core      Rules    all game constants
core/src/rules/items.rs      →  used directly from core      Rules    item types, stats, enchantment
core/src/rules/seed_code.rs  →  used directly from core      Rules    seed encode/decode
core/src/rules/monster_table.rs → used directly from core    Rules    MonsterKind, pick_monster()
core/src/rules/message.rs    →  used directly from core      Rules    GameEvent enum
core/src/command.rs          →  used directly from core      Rules    GameCommand, Direction enum
core/src/game_step.rs        →  (PC only, #[cfg(feature = "std")])    GameStep trait
core/src/tier_micro/map.rs   →  used directly from core      Micro    64×48 mapgen, tile types
core/src/tier_micro/fov.rs   →  used directly from core      Micro    Iterative shadowcasting
core/src/tier_micro/entity.rs→  used directly from core      Micro    16-entity fixed array
core/src/tier_micro/prng.rs  →  used directly from core      Micro    LFSR-16 (Galois LFSR)
core/src/tier_micro/game.rs  →  used directly from core      Micro    MicroGameState, turn loop
core/src/tier_micro/ai.rs    →  used directly from core      Micro    LOS + greedy chase + wander
core/src/tier_micro/spawn.rs →  used directly from core      Micro    spawn mechanics (placement)
core/src/tier_micro/message.rs → used directly               Micro    GameEvent formatting

C64-Specific Modules         →  C64 Module (Rust)            Size Est.  POC Actual
───────────────────────────────────────────────────────────────────────────────────────
(crossterm in tui/)          →  c64/src/render.rs            ~1.5 KB    239 lines
                                 (VIC-II screen + color RAM writes)

(crossterm in tui/)          →  c64/src/input.rs             ~1 KB      179 lines
                                 (Kernal keyboard buffer + CIA joystick Port 2)

(no PC equivalent)           →  c64/src/c64.rs               ~1 KB      172 lines
                                 (C64 hardware registers — migrate to mos-hardware)

saves/src/lib.rs             →  c64/src/save.rs              ~0.8 KB    (pending)
                                 (simplified: binary to floppy / UII+ HTTP)

(no sound on PC)             →  c64/src/sid.rs               ~1 KB      (pending)
                                 (SID register writes via mos-hardware)

(no PC equivalent)           →  c64/src/main.rs              ~0.5 KB    (thin)
                                 (hardware init, game loop glue, C64 Renderer impl)
───────────────────────────────────────────────────────────────────────────────────────
POC total:                       11 source files              1,898 lines = 13 KB
Production estimate:             ~8 source files             ~1,200 lines (frontend only)
  (well within 46 KB budget; core::tier_micro provides all game logic directly,
   ensuring gameplay feature parity across platforms for micro-tier seeds)
```

Note: FOV is no longer C64-specific — iterative shadowcasting lives in
`core::tier_micro::fov` and is used by ALL platforms running micro-tier seeds.

---

## 2. PRNG: Lessons from the POC

The POC's `prng::range()` function had a critical bug: rejection sampling to
avoid modulo bias used `(256u16 - (256u16 % span)) as u8`, which overflows to
0 when span evenly divides 256 (span = 2, 4, 8, 16...). This caused infinite
loops during `spawn_monsters()` for rooms with odd widths/heights (span 2 or 4).

**Fix:** Early return when `256 % span == 0` (no bias exists, accept any value).

**Lesson:** The 8-bit boundary creates overflow traps that don't exist on 32/64-
bit targets. Every arithmetic expression must be audited for u8/u16 overflow.
Rust's type system catches many of these at compile time, but `as` casts are
silent truncation.

The production build places the PRNG in `roguelike_core::tier_micro::prng::LfsrRng`,
which carries this fix and is tested by core's property tests (LFSR period
verification, range distribution checks).

## 3. Input: CIA Port Multiplexing

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

## 4. Static Stack Allocation

llvm-mos's static stack allocation is the key to acceptable performance. For
it to work optimally:

- **Avoid recursion** — all game algorithms are iterative (shadowcasting FOV,
  greedy AI, room placement loop).
- **Minimize function pointers** — trait objects and dynamic dispatch prevent
  call graph analysis. Use static dispatch via generics.
- **Avoid trait-heavy abstractions** — generic functions with trait bounds
  create indirect references that can obstruct call graph analysis. The rules
  module uses concrete types (`u8`, `&mut [u8]`, `&mut LfsrRng`) instead of
  trait bounds, ensuring the call graph is fully visible at link time.
- **Enable LTO** — whole-program optimization is required for static stack
  allocation to analyze the complete call graph.
- **Prefer `&mut` parameters over `static mut` globals** — Rust's borrow
  checker guarantees that `&mut` references don't alias, which helps the
  compiler reason about what memory is touched by each function call. See [§6](#6-c64-code-style-which-abstractions-help-on-the-6502).
- **Struct-based `MicroGameState` is compatible** — Using the tier micro
  module, the C64 uses `MicroGameState` as a local in `main()`. With
  static stack allocation + LTO, a local in non-recursive `main()` gets a
  fixed static address, making field access use absolute addressing — the
  same machine code quality as `static mut`. This is testable: profile on
  VICE after integration. If hot loops show overhead, extract specific
  arrays to statics as a targeted optimization.

## 5. Turn Timing and Cycle Budgets

Total turn processing measured on the POC (full redraw, no dirty-rect):
~20,000 cycles = ~20 ms. With dirty-rectangle rendering (~500 cycles) and
amortized AI costs, this drops to ~8,000 cycles = ~8 ms. Well under one
frame (16.7 ms NTSC / 20 ms PAL).

**Continuous per-frame raster overhead:** In addition to per-turn costs, the
raster interrupt chain ([proposal §8](c64-port-proposal.md#8-implementation-plan), Phase 2a step 7) introduces a continuous background
cost that runs every frame regardless of player input. See the
[C64 demo techniques analysis](c64-demo-techniques-for-roguelike.md)
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

## 6. C64 Code Style: Which Abstractions Help on the 6502

The C64 crate should feel like a C64 program written in Rust — idiomatic for
the hardware, not idiomatic for modern Rust. Most Rust abstractions add
overhead on the 6502 that doesn't exist on modern CPUs. This section documents
which patterns help and which hurt.

**Abstractions that help:**

| Pattern | Why | Cost |
|---------|-----|------|
| **`LfsrRng` struct (in core)** | Explicit data flow via `&mut LfsrRng` helps llvm-mos's alias analysis. The compiler can prove RNG calls don't affect tile arrays or entity arrays, enabling better optimization of surrounding code. | ~2 cycles/call for indirect addressing (zero-page pair) vs absolute. Negligible. |
| **Explicit parameters to AI** | Pass player position `(px, py)` to `run_monster_turns()` instead of reading from entity globals inside the function. Makes data flow visible, helps optimizer, makes the code easier to follow. | ~0 cost — 2 bytes on zero page. |

**Tradeoffs worth making:**

| Pattern | Analysis | Mitigation |
|---------|----------|------------|
| **`&mut MicroGameState` through call chain** | Using the tier micro module, the C64 uses `MicroGameState` — passed as `&mut MicroGameState` through functions. On the 6502, this could mean `LDA (zp),Y` (5 cycles) instead of `LDA absolute,X` (4 cycles) for field access. However: with llvm-mos static stack allocation + LTO, a `MicroGameState` local in non-recursive `main()` gets a fixed static address, making field access use absolute addressing. The benefit of sharing the engine (no separate reimplementation, automatic feature parity, eliminated balance drift) far outweighs the potential 1-cycle cost per field access. | **Testable and recoverable.** Profile on VICE after integration. If hot loops (entity iteration, FOV) show measurable overhead, extract specific arrays to statics as a targeted optimization. The ~100 extra cycles per monster turn (worst case) is ~0.5% of the turn budget. |

**Abstractions that hurt or don't help:**

| Pattern | Why Not | 6502 Impact |
|---------|---------|-------------|
| **`#[repr(u8)]` enums for AI/entity types** | Current `match behavior { entity::AI_CHASE => ... }` with `u8` constants generates `CMP #1 / BEQ`. A proper enum `match` should compile identically but risks branch tables on the immature compiler. The safety benefit is small in a ~2000-line codebase. | Potential code size increase |
| **Newtype wrappers** (`EntityIdx(u8)`) | Requires the compiler to prove transparency for every access. On upstream LLVM this is trivial; on llvm-mos it's unnecessary risk. | Potential codegen regression |

**Summary:** The C64 crate uses `tier_micro::MicroGameState` directly. The
C64-specific frontend code (rendering, input, sound, saves) keeps
C64-appropriate patterns: raw pointer arithmetic, `write_volatile` for hardware
access. FOV uses the tier micro iterative shadowcasting implementation. The core game logic
(entities, combat, AI, mapgen) comes from `roguelike-core::tier_micro` via
`&mut MicroGameState` — a tradeoff that eliminates ~1,200 lines of C64 engine
reimplementation in exchange for a testable and recoverable potential
1-cycle-per-access cost.

---

## 7. C64-Specific Tests

1. **C64-specific tests**: Use `mos-test` crate for target-specific code that
   must run on the MOS simulator.

2. **Integration testing**: Run the .PRG in VICE with automated input scripts
   (VICE supports `-keybuf` for automated key injection).

For the full project-wide testing strategy, see [testing-strategy.md](../testing-strategy.md).
