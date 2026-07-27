# C64 Optimization Log

Every algorithmic change, function rewrite, and codegen workaround made to accommodate
the Commodore 64's 50 KB RAM and 1 MHz 6502 CPU. Organized chronologically within
categories.

**Measurement notes:** Savings marked "measured" were retroactively determined by
building `commit^` and `commit` with `ghcr.io/koalabuttz/rust-mos:ac2fb2277` and
diffing `memmap.py` output. Absolute sizes may differ from historical builds (different
compiler) but deltas are consistent. All other figures are from the original commit
messages.

---

## Memory Layout & Architecture

Structural changes to how the binary uses the C64's address space.

### Enforced reserves + progression cold-code overlay

- **Date:** 2026-07-27
- **Measured result:** normal RAM 181 → 515 B free; HIRAM 25 → 153 B free;
  I/O overlay 1,088 → 153 B free
- `memmap.py` now reports RAM, I/O-overlay, and HIRAM usage as text and JSON,
  compares each region with a reviewed reference, and enforces the floors in
  `memory-budget.json`. CI runs the pinned Rust-MOS map build, publishes the
  deltas in the job summary, and uploads the map plus symbol analysis.
- The recovery target is 512 B normal RAM and 128 B in both constrained
  overlays. Hard floors are 384 B normal and 128 B for I/O/HIRAM so minor
  codegen movement does not erase the reserve unnoticed.
- Micro's pickup, use, drop, and equip handlers were outlined and placed under
  I/O alongside map generation and FOV. The tiny pure `is_walkable` helper is
  there as well. Combine/unequip/drop-equipped remain in normal RAM because
  moving every cold handler overflowed the 4 KB overlay.
- `EntityStore::alive` was removed: HP zero already represents death.
  Awareness uses the otherwise-unused high bit of the sight-radius byte. The
  old alive/awareness bytes remain in the save stream, so existing save tags
  and layouts still decode. Full bit-packing of every entity/item flag was
  rejected after measurement because accessor code overflowed normal RAM.
- Strength and Toughness now share a compact consumable-effect descriptor and
  `StatBoost` event. Save tags 22 and 26 remain unchanged. The C64 formatter
  still prints the full potion name and explicit ATK/DEF result; descriptive
  item names are unchanged across platforms.

### Shrink to fit + KERNAL unmap
- **Commit:** `52c2024` (2026-02-28)
- **Savings:** −10 KB code + 8 KB freed RAM (was 9,116 B over budget)
- Frontend simplification removed ~10 KB of code (draw_box → fill_row, shortened
  format_event messages, merged draw_number variants). KERNAL ROM unmapped at $E000–$FFFF
  by replacing KERNAL keyboard routines with direct CIA1 matrix scanning, disabling
  interrupts, writing RTI stubs to RAM vectors, flipping CPU port. Soft stack relocated
  to the freed KERNAL region so stack and data no longer compete.

### 4-bit tile packing
- **Commit:** `743f3ea` (2026-02-28)
- **Savings:** ~9 KB .noinit
- Pack map tiles into 4-bit nibbles (2 tiles per byte), eliminating the separate
  structural wall bitfield. Halves the per-cell storage from a full byte + wall bit
  to half a byte encoding both tile type and wall status.

### Custom linker script for HIRAM game state
- **Commit:** `3f16230` (2026-03-01)
- **Savings:** ~5.3 KB freed in main RAM
- Custom `link.ld` defines a `hiram` region at $E000–$FFF7 (the unmapped KERNAL area)
  and places MicroGameState and DiffState there via `#[link_section]`. Game state
  lives in a region that would otherwise be wasted.

### I/O banking overlay ($D000)
- **Commit:** `51aff52` (2026-03-04)
- **Savings:** 3.6 KB freed in main RAM (141 B → 3,653 B free)
- Place compute_fov (1,351 B) and generate (2,294 B) in `.overlay` section at
  $D000–$DFFF. While overlay code runs, I/O is banked out, but all three IRQ handlers
  save/restore CPU port so raster interrupts (spinner, music) continue at 50 Hz.

### Move render functions to HIRAM code section
- **Commit:** `403321e` (2026-03-14)
- **Savings:** 657 bytes freed in main RAM (.text −2,076 B)
- New `.hiramcode` linker section at $E000+ for render_end_screen, render_seed_input,
  render_loading, help_chrome, help_page_controls, help_page_bestiary (2,317 B total).
  LMA overlaps .noinit — zero additional RAM cost at runtime.

### Extract shared helpers to reduce .text duplication
- **Commit:** `fdbaa1f` (2026-03-14)
- **Savings:** 333 bytes freed (+11%) — .text −278 B, .noinit −55 B
- Extract duplicated patterns from the monolithic game_loop into `#[inline(never)]`
  helpers: render_and_snapshot (8 call sites), apply_combat_feedback, render_after_step,
  start_and_present_game.

### Relocate DiffState to RAM, render_inventory to HIRAM
- **Commit:** `29ea7a0` (2026-03-14)
- **Savings:** 405 bytes freed in main RAM
- DiffState (~810 B) moved from hiram .noinit.state back to main .noinit (it's only
  accessed during rendering when I/O is visible). Freed hiram space used for
  render_inventory (1,214 B) — a cold-path function only called when inventory opens.

### Overlap SAVE_BUF and DiffState in union
- **Commit:** `a8219e7` (2026-03-24)
- **Savings:** 809 bytes .noinit (RAM free: 801 → 1,610)
- SAVE_BUF (4,096 B for disk I/O) and DiffState (809 B for rendering) are never live
  simultaneously. A Rust `union` overlaps them in the same memory.

### Move render buffers into SharedBuf union
- **Commit:** `57588f5` (2026-03-24)
- **Savings:** 331 bytes (RAM free: 67 → 398)
- Add dirty bitfield (110 B) and msg_buf (40 B) fields to the SharedBuf union. The
  compiler wasn't sharing these static stack slots across functions.

---

## Compiler Runtime Elimination

Removing entire compiler-supplied runtime functions by rewriting the Rust source
to avoid the operations that pull them in.

### Eliminate u64 multiply from seed decode
- **Commit:** `7150872` (2026-03-14)
- **Savings:** 1,858 bytes (measured: .text −1,826 B, .noinit −32 B)
- Rewrote base-36 seed decoding in `rules/seed_code.rs` to use a u32 accumulator
  with overflow checking instead of u64 arithmetic. Eliminated `__muldi3` and the
  entire chain of u64 support functions (multiply, shift, comparison helpers).

### Eliminate __mulsi3 from seed decode
- **Commit:** `e4ea693` (2026-03-15)
- **Savings:** 109 bytes (measured: .text −108 B, .noinit −1 B)
- Replaced `checked_mul` in seed decode with a manual overflow guard
  (`acc > u32::MAX / 36`). `checked_mul` generates a call to `__mulsi3` even
  when the compiler could prove the multiply is within bounds, because the overflow
  check itself requires the full multiply.

### Use shift-based map indexing to eliminate __mulhi3
- **Commit:** `e615e8a` (2026-03-15)
- **Savings:** 83 bytes
- `row_col_idx()` shifts by 6 when width=64 (the common case, since 64 = 2^6) instead
  of multiplying. Falls back to multiply for custom seed dimensions. Removes __mulhi3
  (u16 multiply) from tile_at/set_tile/FOV hot paths.

### Eliminate __ashlqi3 entirely
- **Commit:** `4cf2fd3` (2026-03-22)
- **Savings:** 71 bytes + 3.1M cycles/50 turns eliminated
- Pre-computed `ROW_MASK` lookup table for keyboard scanner row selection and `BIT`
  table for column detection and BFS pathfinding visited set. No variable-count shifts
  remain in the binary — linker dead-code-eliminated `__ashlqi3`.

### Eliminate __udivhi3 from render + hoist FOV stack
- **Commit:** `4cb9966` (2026-03-23)
- **Savings:** 167 bytes
- Replace `cell_idx / 40` divmod in render_diff with running u8 row/column counters.
  Hoist scan_octant stack into compute_fov so it's initialized once instead of 8 times.

### Eliminate u16 division builtins via multiply and bitmask rewrites
- **Commit:** `2f84b79` (2026-03-24)
- **Savings:** 154 bytes net (RAM free: 398 → 552; eliminated __umodhi3 159 B + __umodqi3 64 B)
- health_tier: `hp*4 > max_hp*3` instead of `hp*100/max_hp > 75`. prng range_u8:
  bitmask `& mask` instead of `% span`. div_ceil(2) and div_ceil(8) replaced with
  shift equivalents.

### Eliminate __udivhi3 from HP bar rendering
- **Commit:** `846d0ac` (2026-03-25)
- **Savings:** 72 bytes net (RAM free: 552 → 624; __udivhi3 192 B eliminated)
- HP bar fill: threshold loop instead of `hp*8/max_hp`. HP color: `hp*5 > max_hp*3`
  instead of `hp*100/max_hp > 60`.

### Replace is_power_of_two() and modulo with bitwise ops
- **Commit:** `a0b37a5` (2026-03-25)
- **Savings:** 36 bytes (RAM free: 624 → 660)
- `is_power_of_two()` generates `count_ones()` which uses `% 3` on 6502. Replaced with
  `(n & (n-1)) == 0`. Replaced `% MSG_COUNT` with `& (MSG_COUNT - 1)` since MSG_COUNT
  is a power of two.

---

## Hand-Written Assembly Overrides

6502 assembly replacements for llvm-mos-sdk compiler builtins, linked via `global_asm!`.

### Division builtins: subtraction-loop implementations
- **Commit:** `68e9055` (2026-03-26)
- **Savings:** 278 bytes (.text) — originals 388 B → overrides 110 B
- Override __udivqi3 (90 B), __umodqi3 (64 B), __udivmodhi4 (234 B)
  with subtraction-loop assembly totaling 110 B. Written in assembly so they're opaque to LTO — LLVM
  can't inline them or convert loops back to division.

### __mulhi3, __ashlqi3, __memset overrides
- **Commit:** `18a1856` (2026-03-26)
- **Savings:** 23 bytes — __mulhi3 55→47 B, __ashlqi3 11→9 B, __memset 43→30 B
- Tightened assembly: removed dead JMPs, redundant cpx-after-dex, page-based fill
  for memset.

### memcpy, memmove, memcmp, __udivhi3, __umodhi3 overrides + source revert
- **Commit:** `96ffe0f` (2026-03-26)
- **Savings:** 436 bytes total vs baseline (377 → 813 bytes RAM free)
- memcpy 52→51 B, memmove 145→106 B, memcmp 67→57 B, __udivhi3 192→55 B, __umodhi3
  (new, for reverted callers). Reverted source-level division workarounds back to clean
  division/percentage form — the compact assembly overrides handle these now. Added
  31 algorithm verification tests including exhaustive u8 div/mod.

---

## Iterator & Pattern Rewrites

Replacing idiomatic Rust patterns that generate expensive monomorphized state machines
on 6502.

### Replace iterator patterns with direct array ops
- **Commit:** `8afb8ce` (2026-03-15)
- **Savings:** 2,821 bytes (−295 B .text, −2,526 B .noinit)
- `*buf = [b' '; 40]` instead of `iter_mut().for_each()` in format_event. Array literal
  instead of `iter_mut()` in FOV clear_visible and DiffState snapshot. On 6502,
  `iter_mut()` generates an IterMut state machine with per-function static stack frames
  in .noinit; array assignment compiles to a memset.

### Replace Iterator::nth with direct loop
- **Commit:** `399f627` (2026-03-15)
- **Savings:** 313 bytes (RAM free: 5,516 → 5,829)
- `FilterMap<Enumerate<Iter<Option<InvSlot>>>>::nth` monomorphizes into a 473-byte
  iterator state machine. Replaced with `Inventory::nth_occupied()` — a simple for-loop
  that compiles to tight 6502 code.

### Replace RangeInclusive iterators and inventory adaptors
- **Commit:** `3c179a0` (2026-03-24)
- **Savings:** 496 bytes (.text 39,485 → 38,989; RAM free: 305 → 801)
- Converted 5 `for x in a..=b` loops to `while` loops in ai.rs and map.rs — eliminates
  `RangeInclusive<i8>/<u8>` state machine monomorphizations. Replaced `Inventory::len()`
  filter/count and `is_full()` all() adaptors with manual counting loops.

---

## Rendering Optimizations

### Differential rendering and sparse scroll
- **Commit:** `cf0666b` (2026-03-01)
- **Savings:** ~5× faster non-scroll turns, ~44% faster scrolls
- Row-major render_map eliminates ~4 per-cell multiplies by pre-computing FOV/tile
  linear indices. DiffState snapshots visibility, entity/item positions each frame;
  render_diff redraws only dirty cells. Viewport scrolls use render_map_sparse to skip
  unexplored cells.

### Dead-zone viewport scrolling with memory-copy optimization
- **Commit:** `e95b911` (2026-03-01)
- **Savings:** Performance (measured: .text +4,203 B, .noinit +139 B — large feature addition)
- Viewport only scrolls when player nears the edge (dead-zone), then shifts screen RAM
  rows with `ptr::copy` and only redraws the newly exposed strip. Left only 7 bytes free.

### Replace per-cell volatile diagonal scroll with row-wise ptr::copy
- **Commit:** `32f6ce5` (2026-03-01)
- **Savings:** −94% scroll cycles (measured: scroll work 8.9M → 553K self-cycles/50 turns; .text +93 B)
- The old per-cell volatile approach (~178K cycles/call) far exceeded the PAL vblank budget (~7K).
  Per-cell `read_volatile`/`write_volatile` in nested loop (~819 cells × 4 volatile ops)
  replaced with per-row `ptr::copy` (21 rows × 39 bytes). Screen rows copied first to
  stay ahead of raster scan during vblank.

### Vblank sync and unified draw API
- **Commit:** `9e46660` (2026-03-01)
- **Savings:** Eliminated visual tearing (net +~14 B — overflowed RAM by 7 B from 7 B free)
- `sync_frame()` helper waits for VIC-II vblank before render and scroll memory copies.
  Fixed viewport_pos_lazy to use direct boundary checks instead of `wrapping_sub`
  (eliminated unsigned wraparound bug). Split `draw_char` into `draw_sc` (raw screen
  codes) and `draw_char` (ASCII with internal conversion).

### Clamp loop bounds to eliminate bounds checks
- **Commit:** `f5ef735` (2026-03-15)
- **Savings:** ~422 bytes .text
- Adding `.min(MAX_ENTITIES)` / `.min(MAX_ITEMS)` / `.min(MAX_BITFIELD_SIZE)` gives
  the optimizer proof that runtime counts are within array bounds, so it elides
  `panic_bounds_check` calls and associated panic string infrastructure.

### Eliminate redundant bounds checks in tile_at/set_tile/FOV
- **Commit:** `e8a9359` (2026-03-15)
- **Savings:** 108 bytes (measured: .text −108 B)
- Use `get_unchecked` / `get_unchecked_mut` where the caller already validates indices
  (e.g., FOV bitfield ops where index < MAX_BITFIELD_SIZE is invariant).

### Replace copy_from_slice with direct array ops
- **Commit:** `7d75e93` (2026-03-15)
- **Savings:** 138 bytes (measured: .text −138 B)
- `copy_from_slice` generates a length-equality check and panic on mismatch. Direct
  array assignment avoids this when source and destination are the same known size.

---

## FOV Algorithm Rewrites

### Replace Bresenham raycasting with iterative shadowcasting
- **Commit:** `8b7314c` (2026-03-01)
- **Savings:** Correctness + 40% faster FOV (measured: .text +444 B, .noinit +91 B; compute_fov −39.6% cycles)
- Trades +567 B code for correctness and speed. compute_fov: 13.0M → 7.8M cycles/20 turns.
  Bresenham cast 64 rays to perimeter points, leaving dark gaps between adjacent rays.
  Shadowcasting scans each octant row-by-row using integer rational slopes (i8 num/den,
  i16 cross-multiply) and an explicit 16-entry stack instead of recursion. Fully
  no_std, no heap, fits 6502 constraints.

### Quarter-square multiply for FOV slope comparison
- **Commit:** `9346af7` (2026-03-22)
- **Savings:** Eliminated __mulhi3 calls from FOV inner loop (replaced with table lookup)
- `a*b = QS[a+b] − QS[|a−b|]` using a 134-byte .rodata lookup table. Exact arithmetic
  (identical FOV results, golden replays pass). This is a 17th-century multiplication
  technique — particularly suited to CPUs without hardware multiply.

### Optimize FOV and render hot paths for 6502 codegen
- **Commit:** `26ac716` (2026-03-22)
- **Savings:** 357 bytes .text, render_after_step −14%, __mulqi3 −85%
- BIT lookup table replaces `1u8 << (x & 7)`. Conditional negate replaces generic 8-bit
  multiply for octant transforms. Power-of-2 fast path replaces 16-bit divide. Skip FOV
  recomputation when player doesn't move.

### Bound FOV clear_visible to actual map dimensions
- **Commit:** `1d1a3b1` (2026-03-23)
- **Savings:** ~36% fewer bytes zeroed per FOV call (384 vs 600 bytes)
- Only zero the bitfield bytes covering `width × height` tiles instead of the full
  `MAX_BITFIELD_SIZE`. For 64×48 maps, this saves 216 bytes per memset call.

### Autorun with FOV skip optimization
- **Commit:** `d9d10b0` (2026-03-04)
- **Savings:** FOV computed every 3rd step during autorun (vs every step)
- `step_skip_fov()` skips FOV computation on non-third steps to reduce cost on 6502.
  Stop reasons reported through GameEvent log. No_std BFS stepper with fixed-size buffers.

---

## Static Stack / Codegen Optimizations

Workarounds for rust-mos's static stack allocation model, where each function's local
variables get a permanent .noinit allocation instead of using the hardware stack.

### In-place game state initialization (new_into pattern)
- **Commit:** `90547ca` (2026-03-16)
- **Savings:** 7,778 bytes (.noinit −5,274 B, .text −2,504 B; RAM free: 4.3 KB → 12.1 KB)
- `MicroGameState::new_into(*mut Self)` writes fields directly to the destination,
  bypassing return-by-value. On rust-mos, `fn new() -> Self` allocates a full-size
  temporary in .noinit for the return value. Chained constructors (MicroMap::new,
  EntityStore::new, MicroFov::new) each add their own temporaries — new_into eliminates
  the entire chain.

### MIR optimization level reduction
- **Commit:** `f93c4f1` (2026-03-15)
- **Savings:** 205 bytes (RAM free: 5,311 → 5,516)
- `mir-opt-level=1` instead of default level 2. Level-2 MIR passes (JumpThreading, GVN,
  SROA) trade code size for speed — counterproductive on 6502 where extra live values
  spill to zero page. Level 1 keeps basic simplification while letting LLVM-MOS handle
  target-aware optimization.

### Fix -Oz overlay corruption by splitting main/game_loop
- **Commit:** `1d94b37` (2026-03-13)
- **Savings:** Enabled `opt-level="z"` (was "s") — smaller binary
- Under -Oz the compiler hoists static-stack initialization into main()'s prologue,
  before overlay copy. These addresses overlap the overlay LMA region, corrupting
  overlay bytes. Split main() into a zero-local wrapper that calls init_hardware()
  first, then delegates to `#[inline(never)] game_loop()`.

---

## Hardware Technique Optimizations

### Hardware sprite loading spinner
- **Commit:** `7d56443` (2026-03-02)
- **Savings:** Visual quality (character-cell → hardware sprite)
- 8-frame spinning sword in VIC-II sprite 0 via 48-byte raster IRQ handler. Sprite data
  stored in cassette buffer ($0340–$053F). No runtime patching — all addresses are
  compile-time constants.

### Compress spinner sprite data via vflip
- **Commit:** `89a694c` (2026-03-16)
- **Savings:** Net +59 bytes free (192 B .rodata saved − 133 B .text for vflip)
- Store 5 of 8 sword frames; derive frames 3, 4, 7 via vertical flip from frames 1, 0, 5
  at runtime. Attempted hflip first but `reverse_bits()` compiles to ~300 B of 6502 code,
  making bit-manipulation transforms a net loss. Only row-reorder (vflip) is cost-effective.

### Screen shake via VIC-II raster IRQ
- **Commit:** `99e5162` (2026-03-02)
- **Savings:** 59 bytes .text (feature, not savings — but extremely compact)
- 59-byte IRQ handler alternates XSCROLL between 0 and 2 pixels for 4 frames (~80 ms)
  on Attack or Kill events. Auto-stops and restores scroll register.

### Single-pass keyboard scanning
- **Commit:** `20ce416` (2026-03-22)
- **Savings:** memcpy cycles 5.59M → 1.51M over 50 turns (−73%)
- Combine CIA matrix scan, edge detection, and PREV_KEYS update into one loop. Eliminates
  temp array and memcpy.

### Raster poll bit-7 check for music timing
- **Commit:** `9dc5c6e` (2026-03-05)
- **Savings:** Eliminated input lag during music playback
- Replaced exact raster line poll with VIC-II bit-7 check for vblank detection, preventing
  missed vblank windows that caused input lag.

---

## Foundational Architecture (enabling the port)

These aren't optimizations per se, but algorithmic/structural changes in `roguelike-core`
that made the C64 port possible by enabling `no_std` compilation.

### Extract rules/ module (no_std, always compiled)
- **Commits:** `4bf9c95`, `beaa4a0`, `c388f33`, `74e32ef`, `f953827`, `3d670b2`,
  `6e1908d`, `93c3c89`, `ea7fb6e`, `89db43e` (2026-02-23 through 2026-02-24)
- Pure game rules extracted to `rules/` — balance constants, damage formulas, item
  definitions, monster tables, GameEvent enum, Direction enum, GameColor, seed encoding.
  All `no_std`, all `#[repr(u8)]` enums, all const fn where possible.

### Gate standard-tier code behind std feature
- **Commit:** `b0c6a4e` (2026-02-25)
- 20 standard-tier modules gated behind `#[cfg(feature = "std")]`. C64 frontend depends
  on just `rules/`, `tier_micro/`, `tier_compact/`, and `command`.

### Create tier_micro module
- **Commit:** `bec2f65` (2026-02-25)
- Complete no_std game engine: u8 coords, fixed arrays, LFSR-16 PRNG, iterative
  shadowcasting. Every data structure is stack-allocated with known compile-time sizes.

### Rewrite C64 crate as thin frontend over roguelike-core
- **Commit:** `e944b3a` (2026-02-26)
- Replaced ~1,000 lines of duplicated game logic with imports from `core::tier_micro`.
  Binary size: 14,952 bytes. All game logic shared with other platforms.

---

## Analysis

### Cumulative size impact

Totals below reflect the final state — the assembly overrides (`96ffe0f`, 436 B)
supersede the earlier source-level division workarounds (`2f84b79`, `846d0ac`) which
were reverted. Those two commits are not counted separately.

**Code/data made smaller** (actual binary reduction):

| Category | Bytes saved | Key wins |
|----------|-----------|----------|
| Frontend simplification | ~10,000 | `52c2024`: strip draw_box, shorten messages |
| .noinit reduction | ~19,908 | `743f3ea` 9 KB tile packing, `90547ca` 7.8 KB new_into, `8afb8ce` 2.8 KB iter→array |
| Compiler runtime elimination | 2,324 | `7150872` 1.9 KB (__muldi3 chain), 6 smaller wins |
| Assembly overrides (net) | 436 | `96ffe0f`: compact mem*/div replacements |
| Iterator/pattern rewrites | 809 | `399f627` 313 B, `3c179a0` 496 B |
| Bounds check/panic elision | 668 | `f5ef735` 422 B, `e8a9359` 108 B, `7d75e93` 138 B |
| FOV codegen | 357 | `26ac716`: lookup tables, skip-when-stationary |
| Union overlaps | 1,140 | `a8219e7` 809 B, `57588f5` 331 B |
| Helper extraction | 333 | `fdbaa1f`: dedup 8 call sites |
| MIR opt level | 205 | `f93c4f1`: level 2 → 1 |
| Sprite compression | 59 | `89a694c`: vflip derivation |
| **Subtotal** | **~36,239** | **~35.4 KB** |

**New address space unlocked** (ROM unmap, linker sections):

| Technique | Bytes freed | Source |
|-----------|-----------|--------|
| KERNAL ROM unmap ($E000–$FFFF) | ~8,000 | `52c2024` |
| HIRAM game state ($E000+) | ~5,300 | `3f16230` |
| I/O banking overlay ($D000) | 3,600 | `51aff52` |
| HIRAM code section | 1,062 | `403321e` 657 B, `29ea7a0` 405 B |
| **Subtotal** | **~17,962** | **~17.5 KB** |

**Features that added code** (traded bytes for functionality/correctness):

| Commit | Cost | Purpose |
|--------|------|---------|
| `e95b911` | +4,342 B | Dead-zone viewport scrolling |
| `8b7314c` | +567 B | Shadowcasting FOV (correctness) |
| `32f6ce5` | +93 B | ptr::copy scroll (perf) |
| `99e5162` | +59 B | Screen shake |
| `9e46660` | +~14 B | Vblank sync |
| **Subtotal** | **+5,075 B** | **~5.0 KB** |

**Net RAM capacity gained: ~49.1 KB** (36.2 KB smaller code + 17.5 KB new space − 5.0 KB features)

The C64 has 50 KB usable RAM ($0801–$CFFF). The port started 9,116 B over budget and
now has 813 B free — meaning the game's actual content (map gen, FOV, rendering, AI,
items, inventory, save/load, menus, SID music, sprites) lives within those 50 KB
entirely because of these optimizations.

### Cumulative performance impact

These are not additive — they target different subsystems measured independently.

| Optimization | Metric | Improvement |
|-------------|--------|------------|
| Shadowcasting FOV rewrite | compute_fov cycles/20 turns | 13.0M → 7.8M (**−40%**) |
| ptr::copy diagonal scroll | scroll_diagonal self-cycles/50 turns | 8.9M → 553K (**−94%**) |
| FOV/render hot path codegen | render_after_step | **−14%**; __mulqi3 **−85%** |
| Single-pass keyboard scan | memcpy cycles/50 turns | 5.59M → 1.51M (**−73%**) |
| Eliminate __ashlqi3 | variable-shift cycles/50 turns | 3.1M → 0 (**eliminated**) |
| Differential rendering | non-scroll turn render | **~5× faster** |
| Bound FOV clear | memset per FOV call | 600 → 384 bytes (**−36%**) |
| Autorun FOV skip | FOV calls during autorun | every step → every 3rd (**−67%**) |

The largest wins came from replacing generic algorithms with 6502-aware implementations.
The diagonal scroll optimization is the most dramatic: per-cell volatile access (178K
cycles/call) was replaced with bulk `ptr::copy` (11K cycles/call), a 16× speedup that
brought the operation within the PAL vblank budget of ~7K cycles for tear-free rendering.

### Where the savings came from

The optimization work falls into three tiers of leverage:

1. **High leverage (>1 KB each, 5 commits = ~30 KB):** Frontend simplification (10 KB),
   tile packing (9 KB), new_into pattern (7.8 KB), iterator→array rewrites (2.8 KB),
   __muldi3 elimination (1.9 KB). These are the structural wins — changing data
   representations or eliminating entire classes of compiler runtime.

2. **Medium leverage (100 B–1 KB each, 15 commits = ~5 KB):** Bounds check elision,
   iterator replacements, union overlaps, assembly overrides, shift-based indexing.
   Each individually small, but collectively they reclaimed enough headroom for features
   like save/load, inventory, and the property system.

3. **Address space engineering (4 commits = ~17.5 KB):** KERNAL unmap, custom linker
   sections, I/O banking overlay. No code was changed — just the memory map. These
   techniques are unique to the C64's bank-switching architecture.

---

## Summary Table

| Commit | Date | Optimization | Savings |
|--------|------|-------------|---------|
| `52c2024` | 02-28 | Shrink to fit + KERNAL unmap | −10 KB code + 8 KB RAM |
| `743f3ea` | 02-28 | 4-bit tile packing | ~9 KB .noinit |
| `3f16230` | 03-01 | Custom linker script for HIRAM | ~5.3 KB freed |
| `cf0666b` | 03-01 | Differential rendering | ~5× faster |
| `8b7314c` | 03-01 | Shadowcasting FOV rewrite | +567 B, −40% FOV cycles |
| `e95b911` | 03-01 | Dead-zone viewport scrolling | +4,342 B (feature) |
| `32f6ce5` | 03-01 | Row-wise ptr::copy for scroll | +93 B, −94% scroll cycles |
| `9e46660` | 03-01 | Vblank sync | +~14 B (visual quality) |
| `7d56443` | 03-02 | Hardware sprite spinner | Visual quality |
| `99e5162` | 03-02 | Screen shake via raster IRQ | +59 B (feature) |
| `51aff52` | 03-04 | I/O banking overlay | 3.6 KB freed |
| `d9d10b0` | 03-04 | Autorun FOV skip | FOV every 3rd step |
| `9dc5c6e` | 03-05 | Raster bit-7 timing | No input lag |
| `1d94b37` | 03-13 | Split main/game_loop for -Oz | Enabled opt-level=z |
| `403321e` | 03-14 | HIRAM code section | 657 B freed |
| `fdbaa1f` | 03-14 | Extract shared helpers | 333 B freed |
| `29ea7a0` | 03-14 | DiffState→RAM, inventory→HIRAM | 405 B freed |
| `7150872` | 03-14 | Eliminate __muldi3 (u64 mul) | 1,858 B |
| `f93c4f1` | 03-15 | mir-opt-level=1 | 205 B |
| `399f627` | 03-15 | Replace Iterator::nth | 313 B |
| `8afb8ce` | 03-15 | Replace iter patterns → array ops | 2,821 B |
| `f5ef735` | 03-15 | Clamp bounds → elide panic | ~422 B .text |
| `e4ea693` | 03-15 | Eliminate __mulsi3 | 109 B |
| `e615e8a` | 03-15 | Shift-based map indexing | 83 B |
| `e8a9359` | 03-15 | Unsafe bounds check elimination | 108 B |
| `7d75e93` | 03-15 | copy_from_slice → array assign | 138 B |
| `90547ca` | 03-16 | new_into pattern (in-place init) | 7,778 B |
| `89a694c` | 03-16 | Sprite vflip compression | 59 B net |
| `9346af7` | 03-22 | Quarter-square multiply for FOV | Elim __mulhi3 |
| `20ce416` | 03-22 | Single-pass keyboard scan | −73% memcpy cycles |
| `26ac716` | 03-22 | FOV/render hot path codegen | 357 B, −14% render |
| `4cf2fd3` | 03-22 | Eliminate __ashlqi3 entirely | 71 B + 3.1M cycles |
| `4cb9966` | 03-23 | Eliminate __udivhi3 from render | 167 B |
| `1d1a3b1` | 03-23 | Bound FOV clear to map size | −36% memset |
| `3c179a0` | 03-24 | Replace RangeInclusive iterators | 496 B |
| `a8219e7` | 03-24 | SAVE_BUF/DiffState union | 809 B .noinit |
| `57588f5` | 03-24 | SharedBuf union for render bufs | 331 B |
| `2f84b79` | 03-24 | Multiply/bitmask div rewrites | 154 B net (398→552) |
| `846d0ac` | 03-25 | HP bar division elimination | 72 B net (552→624) |
| `a0b37a5` | 03-25 | Bitwise power-of-two + & mask | 36 B |
| `68e9055` | 03-26 | Division assembly overrides | 278 B |
| `18a1856` | 03-26 | mulhi3/ashlqi3/memset overrides | 23 B |
| `96ffe0f` | 03-26 | Full mem*/div override suite | 436 B total |
