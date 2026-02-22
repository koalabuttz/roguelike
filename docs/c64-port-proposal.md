# Commodore 64 Port Proposal

**Project:** Roguelike Dungeon Crawler — C64 Edition
**Date:** 2026-02-21
**Status:** Proposal — POC validated (rust-mos, 13 KB .PRG, playable on c64.emu)

---

## 1. Executive Summary

This document proposes a native Commodore 64 port of the roguelike dungeon
crawler currently implemented in Rust. The C64 port uses **rust-mos** — a fork
of the Rust compiler backed by the llvm-mos LLVM backend — to compile `no_std`
Rust directly to MOS 6502 machine code. This keeps the entire project in a
single language, enables shared game logic between the PC and C64 codebases, and
preserves Rust's type safety and ownership model on an 8-bit platform.

A working proof-of-concept has been built and tested. The POC implements the
complete game loop — procedural dungeon generation, Bresenham FOV, entity
system, melee combat, monster AI, PETSCII rendering, keyboard + joystick
input, and a message log — in **1,898 lines of `no_std` Rust** compiling to a
**13 KB .PRG binary**. It runs on both c64.emu (Android) and VICE.

The goal is a faithful adaptation — not a 1:1 clone. The C64 version preserves
the dungeon-crawling experience (procedural rooms, fog of war, three monster
types, HP regeneration, and tactical corridor combat) while making principled
trade-offs for the platform's constraints.

**Why rust-mos over cc65:** The original version of this proposal recommended
cc65 (a C cross-compiler for 6502) with hand-optimized assembly hot paths.
Rust-mos was chosen instead because:

1. **Shared language** — The entire project (PC, SSH, MCP, and C64) stays in
   Rust. Developers don't need to context-switch between Rust and C/asm.
2. **Shared algorithms** — Map generation, combat, monster spawning, room
   geometry, and the PRNG will live in `roguelike-core` organized as a
   **capability tier hierarchy**. The C64's game logic lives in a `tier_micro`
   module using `u8` types, fixed-size arrays, and `#![no_std]`. Pure game
   rules (damage formulas, balance constants, XP tables, item definitions)
   live in a `rules/` module compiled by all tiers. Higher-tier platforms
   (GBA, Vita, PC) include the micro tier alongside their native tier,
   enabling cross-platform play: when any platform runs a micro-tier seed, it
   uses the same algorithms as the C64. The same seed produces the same dungeon
   on every platform.
3. **Type safety** — Rust's ownership model catches bugs at compile time that
   would be runtime crashes on a 6502 (buffer overflows, use-after-free, etc.).
4. **POC validation** — The 13 KB binary proves code size is competitive with
   cc65 estimates (~12 KB for assembly, ~18 KB for C).
5. **Ecosystem** — The `mos-hardware` crate provides type-safe, volatile-correct
   access to VIC-II, SID, CIA, and Kernal — better than raw `poke` calls.

**Enhanced hardware target:** The proposal assumes an **Ultimate 64** (or a
stock C64 with an **Ultimate-II+ cartridge**) as the recommended platform,
which provides 10/100 Mbit Ethernet via a built-in network interface. This
unlocks online features — leaderboards, seed sharing, cloud saves, network
spectation, and even LLM integration via the existing MCP server — that would
be impossible on a stock C64. The core game runs on any C64; network features
gracefully degrade when no UII+ is present.

---

## 2. Toolchain: rust-mos and llvm-mos

### 2.1 Overview

[rust-mos](https://github.com/mrk-its/rust-mos) is a fork of the Rust compiler
maintained by Mariusz Krynski (mrk-its) that adds MOS 6502 target support via
the [llvm-mos](https://github.com/llvm-mos/llvm-mos) LLVM backend (511 stars,
actively developed). The Rust fork tracks upstream at approximately Rust 1.76-
1.78 with active rebase branches up to 1.87.

The toolchain runs via Docker:

```bash
# x86_64 hosts
docker pull mrkits/rust-mos

# ARM64 hosts (Apple Silicon, Raspberry Pi)
docker pull mrkits/rust-mos:13f2838f9-334fc98-8f3a80f8  # tagged ARM64 build
```

Build command (from the POC Makefile):

```bash
docker run --rm \
  -e PATH=/usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin \
  -v $(pwd):/work -w /work \
  mrkits/rust-mos:13f2838f9-334fc98-8f3a80f8 \
  cargo build --release
```

### 2.2 What Works

- **`core` crate**: Fully functional — algebraic data types, pattern matching,
  iterators, `Option`, `Result`, traits, generics, closures, `const fn`.
- **`alloc` crate**: Available via the `mos-alloc` crate (heap allocator). We
  choose NOT to use it — the game runs entirely on static arrays with no heap.
- **64-bit integer arithmetic**: Compiles correctly to 8-bit instruction
  sequences. chirp8-c64 confirmed this: "I was worried about the handling of
  64-bit variables by LLVM-MOS, but it compiled everything to 8-bit instructions
  like a champ." (Gergo Erdi, 2021)
- **FFI with C**: Functions seamlessly for Kernal call wrappers.
- **`no_std` ecosystem crates**: Any `no_std` crate compiles for MOS targets.
- **LTO**: Link-time optimization across the entire program, critical for good
  6502 code generation.

### 2.3 What Does Not Work

| Feature | Status | Workaround |
|---------|--------|------------|
| Inline assembly (`asm!`) | Not supported — no 6502 register constraints in Rust's asm infrastructure | Use C FFI wrappers for hardware access, or use `mos-hardware` crate |
| `std` library | Not available (bare metal) | `no_std` + `core` only |
| Floating point | Partial — soft float exists but `f64 as i32` casting has bugs | Integer-only algorithms (Bresenham FOV already avoids FP) |
| 128-bit division | LLVM legalization error | Enable LTO (already required) |
| Dynamic dispatch (trait objects) | Works but expensive — vtable indirection costs ~20 cycles per call | Use static dispatch (generics) exclusively |
| Recursion | Works but prevents static stack allocation optimization | Avoid — already mitigated by iterative algorithms |

### 2.4 llvm-mos Code Generation

The llvm-mos backend provides several 6502-specific optimizations:

- **Static stack allocation**: Whole-program call graph analysis at link time
  places non-recursive function "stack frames" in statically allocated global
  memory, eliminating the soft stack entirely. This is the single most
  important optimization — programs without recursion may need no soft stack
  at all.
- **Zero page register allocation**: LLVM's register allocator keeps temporary
  values in zero page ($00-$FF) and CPU registers (A, X, Y), minimizing memory
  traffic. Zero page accesses are ~1 cycle faster than absolute addressing.
- **16-bit index optimization**: Rewrites `base_16bit + loop_index_16bit` as
  `base_16bit + offset_8bit`, enabling efficient `LDA (zp),Y` addressing.
- **Calling convention**: First arguments in A/X registers, subsequent in zero
  page pairs RS1-RS7. Return values in A/X. Much more efficient than cc65's
  stack-based parameter passing.

**Code quality assessment**: llvm-mos produces code that is roughly comparable
to cc65 for most patterns, 10-11% smaller for simple functions, but can be
28-39% larger for complex functions (no shared helper library like cc65's
runtime). The POC's 13 KB binary is within the expected range.

**Critical performance note from chirp8-c64**: "Going through a slice for screen
manipulation is massively slower than using a raw pointer." For hot paths
(rendering, FOV), prefer raw pointer arithmetic with `write_volatile` over safe
Rust slice operations.

### 2.5 Toolchain Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Single maintainer (mrk-its) | High | High | llvm-mos (the LLVM backend) has a separate, active team; Mikael Lund maintains a parallel fork; Docker images are self-contained and version-pinned |
| No path to upstream Rust | Certain | Low | Acceptable for a retro hobby project; Docker pins the toolchain version |
| Rust version drift | Medium | Medium | Pin Docker image tag; avoid bleeding-edge Rust features |
| Code generation regressions | Medium | Medium | Pin Docker image; test on VICE before each release |

---

## 3. Ecosystem: mos-hardware and Prior Art

### 3.1 The mos-hardware Crate

[mos-hardware](https://github.com/mlund/mos-hardware) (v0.4.0, ~50 stars, by
Mikael Lund) is the de facto standard crate for C64 Rust development. It
provides type-safe, volatile-correct access to all C64 hardware:

| Chip | Module | Features |
|------|--------|----------|
| VIC-II (6567/6569) | `vic2` | Sprites, raster IRQs, scrolling, video modes, color constants |
| SID (6581/8580) | `sid` | 3 voices, ADSR, filters, PSID playback, hardware RNG via `SIDRng` |
| CIA (6526) | `cia` | Joystick (`JoystickPosition` enum), keyboard matrix, timers, TOD clock |
| CPU 6510 | `c64::cpu6510` | Memory banking (ROM/RAM/IO switching via typed flags) |
| Kernal | `cbm_kernal` | File I/O wrappers, device access, `genio::Read` implementation |
| PETSCII | `petscii` | Character encoding, compile-time `screen_codes!()` / `petscii_codes!()` macros |

**Design principles:**
- Hardware registers are `#[repr(C, packed)]` structs with `volatile-register`
  `RW<T>` / `RO<T>` wrappers. Reads are safe; writes require `unsafe`.
- All multi-bit fields use `bitflags!` — impossible to set invalid combinations.
- Bank/address calculations are `const fn` — zero runtime cost.
- Static size assertions verify struct layouts at compile time.
- Feature-gated modules — include only the chips you need.

**Example — joystick reading with mos-hardware:**

```rust
use mos_hardware::{c64, cia::JoystickPosition};

let controller = c64::cia1().port_a.read().into();
let (position, fire) = controller.read_joystick();
match position {
    JoystickPosition::Up        => cmd_move_n(),
    JoystickPosition::DownRight => cmd_move_se(),
    JoystickPosition::Middle if fire => cmd_wait(),
    // ...
}
```

**Example — SID sound effect with mos-hardware:**

```rust
use mos_hardware::{c64, sid};

unsafe {
    let s = c64::sid();
    s.voice1.frequency.write(0x1CD6);           // ~440 Hz (A4)
    s.voice1.attack_decay.write(0x09);          // fast attack, medium decay
    s.voice1.sustain_release.write(0xA0);       // medium sustain, fast release
    s.voice1.control.write(sid::VoiceControlFlags::TRIANGLE | sid::VoiceControlFlags::GATE);
}
```

The POC currently uses hand-rolled `poke`/`peek` wrappers for simplicity.
The production build should migrate to `mos-hardware` for type safety and
to leverage its SID, keyboard matrix, and PETSCII support.

### 3.2 Prior Art: rust-mos Projects

| Project | Author | Description | Key Insight |
|---------|--------|-------------|-------------|
| [chirp8-c64](https://github.com/gergoerdi/chirp8-c64) | Gergo Erdi | CHIP-8 emulator for C64, split engine/frontend | Proved `no_std` Rust + trait-based platform abstraction works on 6502; raw pointers >> slices for screen writes |
| [mos-hardware examples](https://github.com/mlund/mos-hardware) | Mikael Lund | 7 C64 demos: plasma, sprites, SID, raster IRQ, joystick, scrolling, 10PRINT | Demonstrates idiomatic Rust patterns for VIC-II/SID/CIA access |
| [llvm-mos-ferris-demo](https://github.com/mrk-its/llvm-mos-ferris-demo) | mrk-its | Animated Ferris on Atari 8-bit, 95% Rust | Shows rust-mos works on multiple 6502 platforms |
| [aoc2022](https://github.com/mrk-its/aoc2022) | mrk-its | Advent of Code 2022 on Atari 8-bit | Demonstrates complex algorithms compiled to 6502 |

**chirp8-c64 architecture** (most relevant to our project):

chirp8-c64 uses a split `engine` + `frontend` pattern. The engine crate
(`chirp8-engine`) is `#![no_std]` and defines a `Peripherals` trait:

```rust
pub trait Peripherals {
    fn set_pixel_row(&mut self, y: ScreenY, row: ScreenRow);
    fn get_keys(&self) -> u16;
    fn read_ram(&self, addr: Addr) -> Byte;
    fn write_ram(&mut self, addr: Addr, val: Byte);
}
```

The same engine runs on desktop (SDL), AVR microcontrollers, and the C64. Each
platform implements `Peripherals`. This demonstrates that a shared `no_std` game
core with platform-specific frontends is a proven pattern for rust-mos.

**Why we don't follow chirp8's trait pattern:** chirp8 needs platform
abstraction because the same engine runs on fundamentally different hardware
(SDL vs 6502 vs AVR). Our project has a different constraint: the PC and C64
engines differ in data structures (`Vec` vs static arrays, `HashSet` vs
bitfields) more than in algorithms. Instead of abstracting storage behind
traits — which adds indirection that defeats llvm-mos's static stack
allocation ([C64 platform guide §4](c64-platform-guide.md#4-static-stack-allocation)) — we share the algorithms directly by writing them at the
C64's abstraction level (`u8`, `&mut [u8]`, `&mut [Room]`). See §5 for the
full design.

### 3.3 Prior Art: C64 Roguelikes

- **Sword of Fargoal (1982)** — Procedural dungeons, fog of war, combat.
  Proved the concept works beautifully on C64 hardware.
- **Gateway to Apshai (1983)** — Real-time dungeon crawling with joystick.
- **Rogue (1980)** — 40x24 display, simpler LOS. The original.
- **Hack (1985)** — Complex inventory and interaction within tight memory.
- **Dungeon of the Rogue Daemon (2017-present)** — Leif Bloomquist's
  [MultiRogueLike](https://github.com/LeifBloomquist/MultiRogueLike), a
  multiplayer roguelike running on real C64 hardware (RR-Net / 64NIC+ / Ultimate
  64) with simultaneous web browser and Telnet clients. Uses a thin-client
  architecture: a Java server handles all game logic, rendering a 21x17 viewport
  per player and sending pre-rendered screen data over UDP (23-byte fixed
  packets). The C64 client is pure 6502 assembly (ca65) using the IP65 network
  library, with a raster IRQ-driven 3-phase game loop (input / network / screen
  copy). Key lessons from the project: (1) custom character sets work well as
  roguelike tilesets via VIC-II character mode — the server downloads a custom
  font via TFTP at boot; (2) UDP with a simple action counter for deduplication
  is simpler than TCP for latency-sensitive C64 communication; (3) VIC Bank 1
  ($4000-$7FFF) avoids conflicts with the IP65 network stack; (4) a two-item
  inventory (one per hand) provides meaningful gameplay depth with minimal UI
  and memory cost; (5) TFTP-based auto-update at boot elegantly solves
  distribution for a platform without easy internet access. Presented at
  Roguelike Celebration in 2018, 2019, and 2020. Our approach differs
  fundamentally — we use a fat client with local game logic rather than a thin
  networked terminal — but MultiRogueLike validates that the C64 hardware is
  capable of a compelling roguelike experience and informs several design
  decisions in this proposal (see §6.1, §6.7, §6.9, §6.13).

---

## 4. Platform Constraints vs. Current Design

| Resource | Tier standard (PC) | Tier micro (C64) |
|----------|-------------------|--------------|
| CPU | Multi-GHz, 64-bit | MOS 6510 @ 1.023 MHz, 8-bit |
| RAM | Gigabytes | 64 KB total (~38 KB usable) |
| Screen | 80x40+ terminal chars | 40x25 characters (1000 bytes) |
| Colors | 24-bit RGB | 16 fixed colors |
| Character set | Full Unicode / ASCII | PETSCII (shifted/unshifted) |
| Storage | SSD / RAM disk | 1541 floppy: 170 KB (~35 sec load) |
| Networking | TCP/IP (SSH, MCP servers) | UII+ Ethernet: 10/100 Mbit TCP/UDP |
| Integer types | i32 everywhere | 8-bit native, 16-bit emulated |
| Floating point | f64 (used in FOV slopes) | Software FP: buggy in rust-mos |
| Data structures | Vec, HashSet, String, HashMap | Static arrays, bitfields |
| Coordinate space | `type Coord = i32` (`types.rs`) | `u8` (0-63 x, 0-47 y; 40x21 viewport) |
| Stat values | `type Stat = i32` (`types.rs`) | `u8` (0-255) |
| Max entities | `MAX_ENTITIES = 1024` | `MAX_ENTITIES = 16` |
| PRNG | `rand::StdRng` (ChaCha20, u64 seed) | Galois LFSR (16-bit, u16 seed) |
| Code sharing | `roguelike-core` tier standard (native) + tier micro (cross-platform) | `roguelike-core::tier_micro` only (`no_std`) |
| **Language** | **Rust (std)** | **Rust (no_std, no_alloc) via rust-mos** |
| **Compiler** | **rustc (upstream)** | **rustc (rust-mos fork) + llvm-mos** |

### 4.1 The Memory Budget

The C64 has 64 KB of address space, but the Kernal ROM, BASIC ROM, I/O
registers, screen memory, and the zero page consume significant portions:

```
$0000-$00FF   Zero Page (256 bytes) — llvm-mos imaginary register file
$0100-$01FF   CPU Stack (256 bytes)
$0200-$03FF   Kernal/BASIC workspace
$0400-$07FF   Default screen memory (1000 bytes + 24 spare)
$0800-$9FFF   Free RAM (~38 KB) — our program + data
$A000-$BFFF   BASIC ROM (banked out = +8 KB free)
$C000-$CFFF   Free RAM (4 KB)
$D000-$DFFF   I/O registers / Char ROM
$E000-$FFFF   Kernal ROM (banked out = +8 KB free, but no Kernal calls)
```

By banking out BASIC ROM (we don't need it), we get ~46 KB for program + data.
The sweet spot is **banking out BASIC only: ~46 KB usable**.

### 4.2 Memory Budget Allocation (Updated for rust-mos)

```
Program code:           ~18 KB   (rust-mos Rust, release+LTO+opt-size)
  POC measured at 13 KB with full game loop; production adds SID,
  custom charset, save/load, dirty-rect rendering, items, XP (~5 KB extra)
  roguelike-core (no_std subset) will contribute ~1.5 KB (mapgen, prng,
  combat, rooms, items, leveling, depth scaling, wandering spawn, mood)
Map tile data:          3,072 B   (64 x 48 = 3,072 tiles, 1 byte/tile)
Structural wall bits:     384 B   (3,072 bits)
Explored bitfield:        384 B   (3,072 bits)
Visible bitfield:         105 B   (840 bits — viewport-sized, not map-sized)
Entity parallel arrays:   208 B   (16 entities x 13 bytes across arrays)
  Base (POC):  10 arrays x 16 = 160 B (x, y, hp, max_hp, atk, def,
               kind, ai, alive, sight)
  Gameplay:     3 arrays x 16 =  48 B (xp_value, mood, memory)
Room list:                 80 B   (20 rooms x 4 bytes)
Floor items (sparse):      97 B   (32 items x 3 bytes + count)
Player inventory:          12 B   (10 slots + weapon + armor, u8 type IDs)
Player state:               4 B   (xp: u16, level: u8, depth: u8)
Message log:              200 B   (4 lines x ~40 chars + metadata)
RNG state:                  2 B   (16-bit Galois LFSR — LfsrRng struct)
Custom charsets:          6 KB    (3 x 2 KB: dungeon tiles, UI font, message text)
  Per-zone charset switching via raster interrupts — see §6.9
Sprite animation data:    1 KB    (63 bytes/frame x 4 frames x 4 entities)
Raster interrupt code:   200 B    (IRQ handler, gradient table, zone setup)
Save buffer:              2 KB    (serialized game state for disk)
Sound/music data:         2 KB    (SID chip patterns)
Zero page (llvm-mos):     32 B    (16 register pairs RC0-RC31)
────────────────────────────────────
Viewport prev-frame:      840 B   (40 x 21 dirty-rect comparison buffer)
────────────────────────────────────
Total:                  ~34 KB    (~12 KB headroom remaining)
```

The POC validates the baseline: 13 KB code + ~2 KB static data = ~15 KB total.
The 64x48 scrolling map adds ~2.5 KB of data over the POC's 40x21 fixed map
(larger tile buffer, structural/explored bitfields, viewport buffer).
Gameplay features (items, XP, stairs, mood — see the
[gameplay implementation plan](design/gameplay-implementation-plan.md)) add ~1
KB of data and ~2 KB of code. The production build adds ~5 KB over the POC
baseline for raster effects, per-zone charsets, scrolling viewport code, and
the expanded rendering pipeline (see
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)).
Even with all production additions (SID, charsets, raster effects, scrolling,
saves, gameplay features), we stay under the 46 KB budget with ~12 KB headroom.

---

## 5. Code Sharing Strategy

The most compelling advantage of using rust-mos over cc65 is the ability to
share Rust code between the PC and C64 codebases. `roguelike-core` is organized
around a **capability tier hierarchy** — micro (C64, `u8`, `no_std`), compact
(GBA, `i16`, `no_std`), and standard (PC/Vita, `i32`, `std`) — where each
platform compiles its native tier plus all lower tiers. A `rules/` module
(always compiled, `no_std`) contains pure game rules shared by all tiers:
damage formulas, balance constants, item definitions, monster tables, seed
encoding, and structured `GameEvent` messages. When a player starts a new game,
the tier is inferred from the seed's numeric value (`seed <= 0xFFFF` → micro).
A micro-tier seed plays identically on every platform — same map, same FOV,
same AI, same entity count.

The C64 compiles only `tier_micro` + `rules` (with `default-features = false`).
The `tier_micro` module uses the C64's natural types directly — `u8`
coordinates, fixed-size arrays, concrete `LfsrRng` — producing a clean call
graph that llvm-mos can fully optimize. No traits, no generics, no cfg gates
within the module. The C64 crate is excluded from the Cargo workspace (it
builds via a separate rust-mos Docker toolchain) but depends on core via a
relative path.

For the complete architecture — directory tree, dependency graph, build
pipeline, design principles, module inventory, seed system, and tier
divergence — see the
[cross-platform architecture](architecture/cross-platform.md). For code
listings, platform integration examples, balance constants, and the sharing
matrix, see the
[capability tier reference](capability-tier-reference.md). For the
testing strategy, see [testing-strategy.md](testing-strategy.md).

---

## 6. Design Decisions

> **See also:** The [C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
> evaluates VIC-II demo scene techniques for the roguelike port, identifying
> raster interrupts, per-zone charset switching, atmospheric color gradients,
> border removal, and sprite overlays as high-value additions. Several sections
> below (§6.8 Rendering, §6.9 Charset, §6.11 Sound) and the Phase 2
> implementation plan (§8) have been updated to incorporate these techniques.

### 6.1 Map Size and Viewport

**Current:** 80x40 map, fully visible in terminal (no scrolling).

**C64 approach:** The C64 screen is 40x25. The
[gameplay implementation plan](design/gameplay-implementation-plan.md) adds
items, XP/leveling, and dungeon depth to both platforms. Reserving 1 row for a
dense status bar and 3 rows for the message log leaves a **40x21 visible play
area**.

```
┌────────────────────────────────────────┐
│              40x21 viewport            │  <- Map area (rows 0-20)
│         (scrolls to follow @)          │
│                                        │
│                                        │
├────────────────────────────────────────┤
│ HP██████░░24/30 Lv3 F4 /⚔ [🛡 K:7 XP:35│  <- Status bar (row 21)
├────────────────────────────────────────┤
│ You attack the Goblin for 5 damage.    │  <- Message log
│ The Goblin is dead!                    │  <- (rows 22-24, 3 lines)
│ You see a Healing Potion here.         │
└────────────────────────────────────────┘
```

The status bar packs HP bar (with green/yellow/red color coding), player level,
current floor, equipped weapon and armor glyphs, kill count, and XP into a
single 40-character row. This is possible because **inventory is modal** —
pressing `i` opens a NetHack-style fullscreen inventory overlay on top of the
map, dismissable with any key. No permanent screen space is needed for item
lists. Equipment is shown on the status bar as single-character glyphs (`/` for
sword, `[` for armor) — enough to remind the player what's equipped without
consuming a dedicated row.

The 3-line message log (up from the POC's 2 lines) provides enough space for
a full combat exchange ("You attack... / The Goblin dies! / You see a Healing
Potion here.") without scrolling off important information.

MultiRogueLike uses 8 rows of UI (21x17 viewport) because it's real-time and
multiplayer — it needs persistent displays for player count, both hand slots,
and a larger message area. Our turn-based, single-player design can get by
with less because the player controls the pace and can open modal screens at
will.

**Decision:** Scrolling 40x21 viewport over **64x48 maps** (tier micro map
size). The POC validated 40x22 fixed maps, but larger maps enable richer
dungeons with more rooms and longer corridors. The viewport scrolls to follow
the player. Memory cost: ~3 KB for the tile buffer (up from 840 B) — fits
within the 16 KB headroom (§4.2). Scrolling is implemented in Phase 2a step 7
(§8). Tier micro dimensions (64×48) are stable within a version — when any
platform runs a micro-tier seed, it uses these dimensions. Tier standard uses
80×40 (configurable); tier compact uses 128×96.

### 6.2 Map Generation

Map generation will use core's `map::generate()` function on both platforms,
ensuring identical room placement, corridor routing, and dungeon topology for
any given seed. See the
[capability tier reference §1.3-1.4](capability-tier-reference.md#13-tier-micro-map-generation)
for the shared function and platform integration examples.

All platforms call the tier-appropriate mapgen function. When running a
micro-tier seed, every platform uses `LfsrRng` and 64×48 dimensions. In
standard-tier play, `ChaCha20` and 80×40 dimensions are used.

Key parameters for C64 maps:
- **Map size**: 64x48 (scrolling 40x21 viewport — see §6.1)
- **Max rooms**: 20
- **Room sizes**: 3-7 tiles
- **PRNG**: `LfsrRng` (shared Galois LFSR)
- **Storage**: `[u8; 3072]` tile array (in core's `GameState`)
- **Structural walls**: Bitfield (`[u8; 384]`, in core's `GameState`)

### 6.3 Field of View

FOV algorithm is part of the tier definition. Within a tier, ALL platforms use
the same algorithm — no cross-platform divergence.

**Tier micro / compact — Bresenham raycasting** (integer-only) with
precomputed perimeter table:

```rust
// Precomputed perimeter offsets for radius 6 — 40 ray targets
const PERIMETER: [(i8, i8); 40] = [
    (6, 0), (6, 1), (6, 2), (5, 3), (5, 4), (4, 5), (3, 5), (2, 6),
    // ... (computed at compile time)
];
```

Visibility stored as `[u8; 110]` bitfield. Cost: ~150 tile checks per FOV
recompute = ~7,500 cycles = ~7.5 ms. Imperceptible.

**Tier standard — Recursive shadowcasting** with `f64` slopes and
`HashSet<(i32,i32)>`. Symmetric (if A sees B, B sees A), handles thin
diagonal walls cleanly.

When the PC runs a micro-tier seed, it uses Bresenham — the same algorithm as
the C64. This eliminates the FOV divergence that would otherwise make
cross-platform seeds produce different tactical experiences. Same seed, same
tier, same dungeon, same visibility — on every platform.

### 6.4 Entity System

**Tier-determined entity capacity:**

`MAX_ENTITIES` is part of the tier definition: micro = 16, compact = 128,
standard = 512–1024 (platform-tuned via SimBudget — e.g. Vita 512, PC 1024).
When the PC runs a micro-tier game, it uses 16 entities.

The C64 uses `core::tier_micro`'s struct-based `GameState` directly instead of
`static mut` parallel arrays. Tier micro's entity storage uses a fixed-size
array bounded by `MAX_ENTITIES` (16).

The POC used separate `static mut` arrays for each entity field (10 arrays x
16 entries = 160 bytes), which produced tight 6502 indexed addressing. The
production C64 will instead use core's `GameState` struct as a local in
`main()`. With llvm-mos's static stack allocation + LTO, a `GameState` local
in non-recursive `main()` gets a fixed static address — the same machine code
quality as `static mut` but without `unsafe`. If profiling on VICE reveals
overhead in hot loops, specific arrays can be extracted to statics as a
targeted optimization.

Total entity memory: **~208 bytes** (13 fields x 16 entities — see §4.2).

Stat tables are ROM constants from `roguelike_core::rules::balance`.
Names are `&'static [u8]` references to byte string literals — no heap.

Player-specific state (XP total, level, depth, inventory, equipment) lives
on `GameState` rather than on individual entities, keeping progression data
centralized.

### 6.5 Monster AI

Single-ray Bresenham LOS check per monster (`can_see()` in `fov.rs`), then
greedy chase with 3-candidate movement. Wander→Chase transition on player
detection. All implemented and validated in the POC (`ai.rs`, 120 lines).

### 6.6 Combat System

`damage = max(0, attacker_atk - defender_def)`. The formula is a `const fn` in
`roguelike_core::rules::damage` — the single source of truth. Both platforms use the
same function directly.

### 6.7 Items and Inventory (C64 Storage Design)

The [gameplay implementation plan](design/gameplay-implementation-plan.md)
Phase 3 adds items, a fixed-size inventory, and equipment. On the PC, floor
items use `HashMap<Pos, Vec<Item>>` and inventory uses `Vec<Option<Item>>` —
both require heap allocation. In tier micro, the same fixed-size item storage
is used — no heap needed. In tier standard, `Vec`-based storage is available.

**Floor items — sparse fixed-size array:**

Rather than a full map overlay (840 bytes for one-item-per-tile), core will use
a sparse list capped at 32 items per floor. This mirrors the approach used by
[MultiRogueLike](https://github.com/LeifBloomquist/MultiRogueLike), which
stores items in the same entity list rather than as a map layer.

Cost: **97 bytes** (vs. 840 bytes for a full map overlay). Item lookup at a
position is a linear scan over active entries — at 32 max items, this
is ~200 cycles. Negligible for a turn-based game.

Item type IDs map to stat lookup tables in `roguelike_core::rules::items` —
see [capability tier reference §1.7](capability-tier-reference.md#17-item-definitions)
for the complete code listing (`heal_amount()`, `atk_bonus()`, `def_bonus()`
const fns).

**Player inventory and equipment** will be part of core's `GameState`:
fixed-size arrays (10 inventory slots, weapon/armor type IDs).

Cost: **12 bytes.** The `effective_attack()` and `effective_defense()` helpers
in `roguelike_core::rules::damage` take base stats plus equipment type IDs and
return the effective values — used by both platforms directly.

**Inventory UI — modal overlay:** The inventory screen is a NetHack-style modal
overlay: pressing `i` writes an item list over the map area (rows 0-20),
showing slot letters, item names, and "(equipped)" markers. Any key dismisses
the overlay and redraws the map. This avoids dedicating permanent screen rows
to inventory display — the equipped weapon and armor are shown as single-glyph
indicators on the status bar (§6.1). Pickup (`g`), drop (`d`), use (`u`), and
equip (`e`) commands work without opening the inventory for common actions.

**Item spawning** uses core's `spawn` module: weighted random selection into
rooms, with a `max_items_per_room` cap. Item spawn weights and `min_depth`
thresholds live in `roguelike_core::rules::items`.

### 6.8 Rendering

The POC uses full-screen redraw each turn via `write_volatile` to screen
memory ($0400) and color RAM ($D800). Production should add **dirty-rectangle
tracking** — maintain a previous-frame buffer, only update changed cells.

**Color mapping:**

| GameColor | C64 Color | Index |
|-----------|-----------|-------|
| Black | Black | 0 |
| White | White | 1 |
| Grey | Light Grey | 15 |
| DarkGrey | Dark Grey | 11 |
| Red | Red | 2 |
| Brown | Brown | 9 |
| Green | Green | 5 |
| Yellow | Yellow | 7 |
| Blue | Blue | 6 |
| Cyan | Cyan | 3 |

**Status bar:** HP bar uses PETSCII block characters ($A0 = reverse space for
filled, $65 = light shade for empty) with color coding: green >60% HP,
yellow >30%, red ≤30%. Validated in the POC.

**Atmospheric lighting via raster effects:**

The static color mapping above is functional but flat. A raster interrupt chain
(see §8, Phase 2 step 8) enables per-scanline VIC-II register changes that add
dramatic atmosphere at negligible CPU cost. See the
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md) §1
and §5 for detailed implementation sketches.

1. **Torchlight gradient.** Change `$D021` (background color) on each raster
   line within the map area, creating a vertical warm-to-dark gradient centered
   on the player's Y position. Rows near the player use brown (9) or grey (12);
   rows further away fade to black (0). The gradient table is pre-computed (~25
   bytes) and shifted on vertical player movement. Cost: one `STA $D021` per
   raster line in the map area = ~110 cycles/frame. This simulates torchlight
   falloff *independently of the FOV system* — the FOV determines tile
   visibility, the gradient determines ambient mood.

2. **Damage flash.** When the player takes damage, set `$D020` (border) to red
   for 2-3 frames. The raster interrupt handles this independently of the game
   loop — the flash happens instantly, not on the next turn's render. The raster
   chain ensures only the border flashes, not the background.

3. **Low-HP warning.** When HP drops below 30%, pulse the border color between
   black and dark red on alternating frames. The raster interrupt checks player
   HP and modulates `$D020` accordingly — no game loop involvement.

4. **Zone-specific backgrounds.** The game area (rows 0-20), status bar
   (row 21), and message log (rows 22-24) each get different `$D021` values.
   The dungeon uses black, the status bar uses dark grey, the message log uses
   dark blue. Currently all three zones share one background color.

Raster effects are **orthogonal to dirty-rect rendering** — they modify VIC-II
registers, not screen RAM. They work correctly even with dirty-rect optimization
and add no per-turn rendering cost. See the
[C64 platform guide §5](c64-platform-guide.md#5-turn-timing-and-cycle-budgets)
for the complete cycle budget breakdown.

### 6.9 Custom Character Set

Design **three 2 KB charsets** (6 KB total — see §4.2) with per-zone switching
via raster interrupts. The VIC-II's charset pointer (`$D018` bits 1-3) is
changed at zone boundaries by the raster interrupt chain (§8, Phase 2 step 8),
allowing different screen zones to use different character sets simultaneously.
See the [C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
§4 for the full per-line charset switching technique.

**Charset 1 — Dungeon tileset (rows 0-20):**

```
Char $00: player '@' (stylized, recognizable)
Char $01: floor '.' (single dot, centered)
Char $02: wall '#' (solid block with edge detail)
Char $03-$06: monster glyphs (G, O, T, %)
Char $07: stairs down '>' (for Phase 2 dungeon levels)
Char $08-$0F: HP bar segments (empty to full, 8 gradations)
Char $10-$17: item glyphs (!, /, [, ?, potion, sword, armor, scroll)
Char $18-$1F: box-drawing characters for menus
Char $20-$5A: standard PETSCII uppercase letters (for inventory overlay)
```

**Charset 2 — UI font (row 21, status bar):**

Optimized for the dense status bar layout — narrower numerals, compact glyphs
for equipped weapon (`/`) and armor (`[`), clean HP bar segments. Designed for
readability at a glance rather than dungeon atmosphere.

**Charset 3 — Message text (rows 22-24):**

Clean, readable text font optimized for message log legibility. Distinct from
the dungeon tileset to visually separate game messages from the map.

The POC uses the default C64 charset. Production adds custom characters via
VIC-II bank switching (use `mos-hardware` `vic2::CharsetBank::from(addr)`
for type-safe configuration).

**Animated charset tiles:** Water, lava, torches, and magical effects animate
by cycling character definitions in charset RAM. Changing 8 bytes (one
character's pixel data) per frame cycles through animation frames. A bubbling
water tile or flickering torch costs **8 bytes of charset write per frame**
(~40 cycles) — and every instance of that character on screen updates
simultaneously. A dungeon with 50 water tiles animates with the same 8-byte
write as a dungeon with 1 water tile. See the demo techniques analysis §4.2
for details.

**VIC-II bank allocation:** Place screen memory and all three charsets in
**VIC Bank 1 ($4000-$7FFF)**. The VIC-II requires screen memory and the active
charset to reside in the same 16 KB bank — they cannot be split across banks.
Screen memory moves from the default $0400 (Bank 0) to **$4400** (Bank 1).
The three charsets occupy $4800, $5000, and $5800 (or similar 2 KB-aligned
positions within Bank 1). This leaves room for sprite data blocks if sprites
are used for the player character (see §12, open question #8).

Bank 1 avoids conflicts with the IP65-compatible network stack region and
the Kernal workspace below $0800. MultiRogueLike uses the same bank for its
custom font and reports no conflicts with networking code. Specify the bank
layout in Phase 2 (when the charset is added) to prevent conflicts when
Phase 3 networking is implemented.

### 6.10 Input Handling

The POC implements two input methods:

1. **Kernal keyboard buffer** ($C6/$0277): Relies on the Kernal IRQ handler to
   scan the keyboard matrix each frame. Works when loaded from BASIC (IRQs
   already running). Maps WASD, QEZC, arrow keys, and space to game commands.

2. **Joystick port 2** (CIA1 Port A, $DC00): Direct hardware read. Must write
   $FF to Port A first to deselect keyboard columns (Port A is shared between
   keyboard column selection and joystick port 2 — a hardware multiplexing
   subtlety discovered during POC debugging).

**POC lesson learned:** CIA1 Port B ($DC01) carries BOTH keyboard rows AND
joystick Port 1 signals on bits 0-4. Direct matrix scanning via Port B is
unreliable on emulators where virtual controls map to Port 1. The production
build should use the Kernal buffer for keyboard (reliable, handles debouncing)
and Port A for joystick Port 2 (no sharing conflict).

**Migration to mos-hardware:** The `cia::GameController` and `JoystickPosition`
enum provide type-safe joystick reading with proper inverted-logic handling.

### 6.11 Sound Design

**Current:** No sound (terminal-based).

The C64's SID chip (MOS 6581/8580) offers three voices with multiple waveforms,
filters, ADSR envelopes, and ring modulation.

Proposed sound effects:
- **Footsteps:** Soft noise-channel tick on each move
- **Attack hit:** Short pulse-wave stab (pitch varies by damage)
- **Attack miss:** Low thud
- **Monster death:** Descending pitch sweep
- **Player hurt:** Dissonant chord + noise burst
- **Player death:** Dramatic descending arpeggio
- **Level ambience:** Low droning pad (triangle wave + filter sweep)

`mos-hardware` provides full SID access including compile-time PSID file
parsing via the `SidTune` trait, and hardware RNG via `SIDRng` (implements
`rand_core::RngCore`).

**SID timing requires a raster interrupt.** The SID play routine (updating
voice registers for ongoing effects and ambience) must be called once per frame
at a consistent rate. The game loop is turn-based and fires at irregular
intervals — a player might act 60 times per second while running down a
corridor, or sit idle for minutes reading messages. Tying SID updates to the
game loop produces audibly inconsistent playback: effects speed up during fast
input and stall during pauses. The standard solution on the C64 is to call
the SID play routine from the raster interrupt chain at a fixed raster line,
ensuring exactly one call per frame regardless of game loop timing. This is
set up in Phase 2 step 8 (§8). See the
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md) §1
for the raster interrupt implementation sketch.

### 6.12 Save System

Binary serialization to 1541 floppy or SD2IEC. Save size: ~1,578 bytes.
Kernal file I/O via `mos-hardware`'s `cbm_kernal` module. C FFI wrappers
needed for `JSR $FFD8` (SAVE) and `JSR $FFD5` (LOAD) since rust-mos lacks
inline assembly.

Additional backends: UII+ network saves, AT Protocol via bridge — unchanged
from the original proposal. See §6.13.

### 6.13 Networking (Ultimate 64 / UII+)

Unchanged from the original proposal. The UII+ command interface and
all network features (leaderboards, daily seeds, cloud saves, spectation,
MCP client, AT Protocol bridge) remain as designed.

See [docs/design/c64-atproto-bridge.md](design/c64-atproto-bridge.md) for the
AT Protocol bridge specification.

**Auto-update via UII+:** MultiRogueLike uses TFTP-based auto-update at boot
to ensure C64 clients always run the latest version — the bootloader downloads
the current binary from the server before launching the game. The UII+ provides
a more capable equivalent: HTTP GET to fetch a .prg from the leaderboard/save
server. Implement a version check at startup: send the current version string
with the daily seed request, receive a `needs_update: bool` flag, and offer
the player a one-button update that writes the new .prg to the UII+ SD card.
This solves the distribution problem for hardware users and accelerates
iteration during development. Add to Phase 3 networking alongside cloud saves.

**Protocol consideration:** For latency-sensitive features like spectation
frame streaming, consider UDP alongside the TCP paths used for saves and
leaderboards. MultiRogueLike found UDP simpler and more reliable than TCP for
real-time C64 communication (23-byte fixed packets with action counter
deduplication). The AT Protocol bridge's binary wire protocol
(§c64-atproto-bridge.md) could also benefit from a UDP transport option for
spectation frames.

### 6.14 Seed System and Cross-Platform Seeds

The cross-platform seed design — tier inference from numeric value,
compatibility selection UI, daily seeds, and per-tier leaderboards — is
defined in the
[capability tier reference](capability-tier-reference.md#19-seed-system-and-cross-platform-seeds).

**C64 seed display:** The C64 natively generates micro-tier seeds (u16, 1–4
base36 characters). CIA timer jitter at first keypress provides the seed.
Displayed on title and death screens. A C64 player sees `r7z` on their death
screen, and any platform can replay that exact dungeon.

---

## 7. Architecture Mapping

The project has four frontends — terminal, SSH, MCP, and C64 — all driven by
`roguelike-core`. Core is organized around capability tiers: the C64 depends on
`core::tier_micro` (with `default-features = false`), the GBA on
`core::tier_compact`, and PC/Vita frontends on tier standard (top-level core
with `std`). Each platform compiles its native tier plus all lower tiers,
enabling cross-platform seed play.

### 7.1 How the Frontends Compare

```
                    PC (standard)           PC (micro-tier seed)    roguelike-c64
                    ──────────────        ──────────────          ─────────────
Depends on:         core (std)            core::tier_micro        core::tier_micro (no_std)
                    + serde, rand, toml   (via core, std)         + nothing (zero extra deps)
Entity storage:     Vec or fixed array    fixed array (16)        fixed array (16) (same)
Map storage:        core's Map (80×40)    tier_micro Map (64×48)  tier_micro Map (64×48) (same)
FOV:                f64 shadowcasting     Bresenham raycasting    Bresenham raycasting (same)
Pathfinding:        A* (HashMap, std)     greedy chase            greedy chase (same)
Data loading:       game.toml (runtime)   const ROM tables        const ROM tables (same)
PRNG:               ChaCha20              LFSR-16                 LFSR-16 (same)
Save format:        JSON (serde_json)     JSON (serde_json)       binary (manual serialization)
                    ──────────────        ──────────────          ─────────────
                         When PC runs a micro-tier seed, it uses tier_micro
                         algorithms — identical gameplay to the C64.
```

Shared across all tiers via `core::rules`:
damage formulas, balance constants, item definitions, leveling tables,
seed codes, MonsterKind enum, GameEvent messages, Direction enum.
Per-tier: entity system, PRNG, mapgen, spawn mechanics, FOV, AI, game loop.

### 7.2 Module-by-Module Mapping

The file-by-file mapping of core modules to C64 usage, including POC line
counts and production estimates, is in the
[C64 platform guide §1](c64-platform-guide.md#1-c64-module-mapping).
In summary: core modules (rules, tier_micro) are used directly; the C64 crate
provides only rendering (VIC-II), input (Kernal/CIA), sound (SID), saves
(floppy/UII+), and hardware initialization — estimated ~1,200 lines of
frontend code (down from the POC's ~1,900 lines that reimplemented the engine).

### 7.3 C64 as a Thin Frontend

With the tier system, the C64 crate is a thin frontend — comparable to the
terminal or SSH crates. It implements platform-specific rendering (VIC-II),
input (Kernal/CIA), sound (SID), and saves (floppy/UII+), but all game logic
comes from `core::tier_micro`.

The production C64 crate shrinks from the POC's ~1,900 lines (which
reimplemented the entire engine) to an estimated ~1,200 lines of frontend code.
Balance drift is eliminated: when monster stats change in `core::rules`, all
platforms and all tiers update automatically.

---

## 8. Implementation Plan

The plan is structured across three release milestones. The workspace is at
v0.3.0; the C64 port milestones use the next three minor versions. Each
milestone is shippable independently — v0.4.0 is a complete standalone game.

### Phase 0: POC (Complete)

**Status: Done.** rust-mos proof-of-concept validated:
- 13 KB binary with complete game loop
- Runs on c64.emu (Android) and VICE
- All core systems functional: map gen, FOV, entities, combat, AI, rendering,
  input, messages

---

### v0.4.0: C64 Core Game (Weeks 1-13, includes 2 weeks buffer)

#### Prep PR: Extract `rules/balance.rs`

0. **Extract `rules/balance.rs` from existing constants** — Move scattered
   balance constants (monster stats, player defaults, spawn weights, depth
   scaling, mood thresholds, map dimensions) from existing modules into
   `core/src/rules/balance.rs`. Add `Direction` enum in `core/src/command.rs`
   — `GameCommand::Move(Direction)` replaces coordinate offsets. Add
   `GameEvent` enum in `core/src/rules/message.rs` for structured messages.
   This is a standalone refactoring PR with no new features — existing tests
   must continue to pass.

#### Phase 1a: Add Tier Micro to Core (Week 1)

1. **Add `core/src/tier_micro/` module** — Create the tier micro module with
   `u8` coords, `u8` stats, fixed-size arrays bounded by `MAX_ENTITIES` (16),
   bitfield visibility storage, `LfsrRng` (Galois LFSR-16), Bresenham FOV,
   greedy chase pathfinding, and spawn mechanics. Move balance constants and
   leveling tables into `core/src/rules/`. Core's existing top-level code
   (i32 types, std collections) remains untouched as tier standard.

2. **Make tier_micro module `no_std`-compatible** — Add
   `#![cfg_attr(not(feature = "std"), no_std)]` to `lib.rs`. The tier_micro
   and rules modules compile without `std`. Gate serde derives behind
   `#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]`. Gate TOML
   loading behind `#[cfg(feature = "data-files")]` (already exists). Gate
   tier standard code (A* pathfinding, shadowcasting, `StdRng`) behind
   `#[cfg(feature = "std")]`. The `std` feature is on by default for PC
   frontends.

#### Phase 1b: C64 Wiring (Week 2, first half)

3. **Wire up C64 crate** — Replace POC's reimplemented engine with dependency
   on `roguelike-core` (`default-features = false`). The C64 crate depends on
   `core::tier_micro` + `core::rules` directly. Replace `static mut` parallel
   arrays with tier_micro's `MicroGameState`. The C64 crate becomes a thin
   frontend: rendering, input, sound, saves only. See
   [C64 platform guide §6](c64-platform-guide.md#6-c64-code-style-which-abstractions-help-on-the-6502)
   for C64 code style guidance.

#### Phase 1c: GameStep + PC Micro-Mode (Week 2, second half)

4. **Add `GameStep` trait** — Define `GameStep` trait in
   `core/src/game_step.rs` (`#[cfg(feature = "std")]`) so that FrameSink,
   MCP, and TUI can work with any tier uniformly. PC micro-mode wraps
   `MicroGameState` with a `GameStep` adapter (wrap, not reimplement).

5. **Add tier determinism tests** — Property tests for LFSR period, damage
   formula, room intersection symmetry, mapgen determinism golden snapshot for
   each tier. GameStep compliance test (adapter produces same GameEvents as
   direct calls). Direction roundtrip test. CI `thumbv6m-none-eabi` check
   for tier_micro: `cargo check -p roguelike-core --no-default-features
   --target thumbv6m-none-eabi`. Verify that tier micro mapgen on the host
   produces byte-identical output to the golden snapshot.

> **Smoke test:** Build C64 .PRG via Docker, run on VICE, verify core
> integration produces identical gameplay to POC.

#### Phase 2a: Core Production (Weeks 3-6)

6. **Migrate C64 crate to mos-hardware** — Replace hand-rolled `poke`/`peek`
   wrappers with `mos-hardware`'s type-safe VIC-II, CIA, and SID access.
   Adopt `JoystickPosition` enum, `screen_codes!()` macro, and
   `volatile-register` patterns. Vendor mos-hardware into the workspace (§12).

7. **Scrolling viewport** — Implement 40x21 camera over 64x48 map with
   player-follow logic and edge clamping. Expand tile buffer from 840 B to
   ~3 KB. Update `roguelike_core::rules::balance` map dimension constants. This is
   the most significant architectural change from the POC — rendering must
   now offset all screen writes by the camera position.

8. **Raster interrupt chain** — Set up VIC raster IRQ replacing the Kernal's
   default IRQ handler. Implement a 3-zone color split (game area / status bar
   / messages) with different `$D021` background colors per zone. Add a SID
   player callback at a fixed raster line for frame-rate-independent sound
   playback. Add per-zone charset pointer switching (`$D018`) for the three
   charsets (§6.9). **This step unlocks Phase 2b.** See the
   [demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
   §1 for implementation details. Cost: ~200 bytes of code, ~660
   cycles/frame continuous overhead.

9. **Dirty-rectangle rendering** — Add previous-frame buffer, compare+update
   only changed cells. Critical for scrolling — full viewport redraw is 840
   cells/frame; dirty-rect limits to the visible delta. Orthogonal to raster
   effects.

10. **Save/Load** — Binary serialization to 1541 floppy via Kernal file I/O.
   C FFI wrappers for `SAVE`/`LOAD` Kernal calls. Follow the `SaveBackend`
   trait pattern from `roguelike-saves` — the C64 won't implement the full
   trait, but the API shape (autosave, slots, metadata) guides the design.

> **Smoke test:** Verify binary size delta from mos-hardware migration; test
> scrolling on VICE; run save/load cycle.

#### Phase 2b: Visual Polish + Audio (Weeks 7-10)

11. **Custom character sets** — Design three charsets in CharPad: dungeon
    tileset (rows 0-20), UI font (row 21), message text (rows 22-24). Load
    into VIC Bank 1 ($4800/$5000/$5800) with screen memory at $4400. Per-zone
    switching handled by the raster chain (step 8). Add animated charset tiles
    for water, torches, and stairs (~8 bytes/frame, ~40 cycles). See §6.9.
    **Budget 2 weeks for this step** — charset design is an art/design task
    requiring CharPad iteration, readability testing on real hardware, and
    potentially multiple revision cycles (1 week design + 1 week integration).

12. **SID sound effects** — Implement combat sounds, footsteps, death jingle
    using `mos-hardware`'s SID module. The raster interrupt chain (step 8)
    handles SID playback timing — the play routine runs once per frame at a
    fixed raster line, independent of the turn-based game loop. See §6.11.

13. **Atmospheric effects + border removal** — Torchlight vertical gradient
    centered on the player's Y position via per-line `$D021` changes. Damage
    flash (border to red for 2-3 frames). Low-HP border pulse. Open
    top/bottom borders by toggling RSEL. See §6.8 and the demo techniques
    analysis §2, §5.

14. **Title screen** — Seed entry (keyboard hex input), "New Game" / "Continue"
    menu, PETSCII art enhanced with raster color bars and animated charset
    characters. Micro-tier seed display with platform compatibility indicator.

> **Smoke test:** Full playthrough on VICE with all effects enabled.

#### Phase 2c: PC Compatibility Selection (parallel with 2a/2b)

15. **PC compatibility selection UI** — Add new game screen offering
    standard/compact/micro compatibility choice. When the player selects micro,
    the terminal client wraps `MicroGameState` with the `GameStep` adapter
    (Phase 1c) — no second implementation needed. The adapter provides the
    same FrameSink/TUI interface as standard-tier `GameState`. Seed entry
    auto-detects tier from seed numeric value. Display platform compatibility
    ("Plays on: C64 · GBA · Vita · PC") in the seed confirmation screen.
    Cross-platform leaderboard per tier. Can begin any time after Phase 1
    completes.

#### Phase 3: Testing + Release (Weeks 11-13)

Testing is continuous — each sub-phase has a smoke test checkpoint. This final
phase focuses on cross-platform verification and hardware-specific edge cases.

16. **Playtesting** — Real hardware (Ultimate 64, C64 + UII+, stock C64) and
    emulators (VICE, c64.emu). PAL vs NTSC timing.
17. **Performance profiling** — VICE cycle counter for FOV + AI + render +
    raster chain overhead. Verify continuous raster costs match estimates
    (see [C64 platform guide §5](c64-platform-guide.md#5-turn-timing-and-cycle-budgets)).
18. **Tier determinism verification** — Generate micro-tier dungeons on C64
    and PC with the same seeds, verify identical tile layouts, entity
    placements, and FOV results. Repeat for compact-tier on GBA and PC.
19. **Packaging** — .d64 disk image, .prg for emulators, .crt if code fits
    16 KB cartridge.

---

### v0.5.0: C64 Networking (Weeks 14-19)

#### Phase 4: UII+ Networking Core

20. **UII+ driver layer** — Hardware detection, TCP primitives.
21. **Cloud saves** — HTTP PUT/GET to server endpoint.
22. **Leaderboard + daily seed** — POST scores, GET daily seed (u16,
    micro-tier). Per-tier leaderboards — micro-tier daily challenges are
    cross-platform across all platforms.
23. **Auto-update via UII+** — Version check at startup, one-button update
    that writes new .prg to UII+ SD card (see §6.13).

Also in v0.5.0 (deferred from v0.4.0):
- **Player sprite** — Hardware sprite overlay for smooth inter-tile movement
  and idle animation (see §12).
- **Level transition animation** — FLD wipe between dungeon floors (see §12).

---

### v0.6.0: Advanced Networking (Weeks 20-25)

#### Phase 5: Extended Network Features

24. **Spectation relay** — Binary frame streaming.
25. **MCP client mode** — Observation formatting, action parsing.
26. **AT Protocol bridge** — Per the design in
    [c64-atproto-bridge.md](design/c64-atproto-bridge.md).

---

## 9. What Gets Cut / What Gets Added

### What Gets Cut (Tier Micro vs. Tier Standard)

These are tier micro constraints, not C64 limitations. The PC in standard mode
has none of these cuts. When the PC runs a micro-tier seed, it uses the
micro-tier parameters.

| Feature | Tier standard | Tier micro | Reason |
|---------|--------------|-----------|--------|
| Map size | 80×40 (configurable) | 64×48 (scrolling 40×21 viewport) | Screen size + UI (§6.1) |
| FOV radius | 8 tiles | 6 tiles | CPU budget |
| FOV algorithm | Recursive shadowcasting | Bresenham raycasting | No FP, no recursion |
| Max rooms | 30 | 12 | Map size |
| Max entities | 512–1024 | 16 | Memory + CPU |
| Save format | JSON (serde) | Binary (compact) | Disk space/speed |
| Color palettes | 4 (accessibility) | 1 (fixed) | 16-color limit (C64 hardware) |
| Auto-explore | A* pathfinding | Simplified or cut | Memory |
| Message history | Scrollable | Last 3 messages | Screen space (§6.1) |

### What Gets Added

| Feature | C64 Only | Milestone |
|---------|----------|-----------|
| Scrolling viewport | 40×21 camera over 64×48 map with player follow | v0.4.0 |
| Raster interrupt chain | 3-zone color split, SID timing, charset switching (§6.8, §6.9, §6.11) | v0.4.0 |
| Atmospheric lighting | Torchlight gradient, damage flash, low-HP pulse via raster effects (§6.8) | v0.4.0 |
| Border removal | Open top/bottom borders for cleaner visual frame | v0.4.0 |
| Sound effects | SID chip combat sounds, ambience, death jingle — raster-timed (§6.11) | v0.4.0 |
| Custom charsets | 3 per-zone charsets: dungeon tiles, UI font, message text (§6.9) | v0.4.0 |
| Animated tiles | Water, torches via charset cycling (~40 cycles/frame) (§6.9) | v0.4.0 |
| Joystick control | Full 8-direction joystick with fire button | v0.4.0 |
| Title screen art | PETSCII art splash screen with raster color bars | v0.4.0 |
| Player sprite | Hardware sprite overlay for smooth movement + animation | v0.5.0 |
| Level transitions | FLD wipe animation between dungeon floors | v0.5.0 |
| Online leaderboards | Per-tier scores via UII+ | v0.5.0 |
| Daily challenge | Micro-tier daily seed — all platforms play identically | v0.5.0 |
| Cloud saves | Save/load game state over HTTP via UII+ | v0.5.0 |
| AT Protocol saves | Federated saves to PDS via bridge | v0.6.0 |
| Network spectation | Stream gameplay to SSH spectation relay | v0.6.0 |
| LLM auto-play | MCP client mode — watch an AI play on your C64 | v0.6.0 |

| Feature | Cross-Platform | Milestone |
|---------|---------------|-----------|
| Compatibility selection | New game screen: standard/compact/micro choice (§6.14) | v0.4.0 |
| Micro-tier play on all platforms | PC/Vita/GBA can run micro-tier seeds with C64-identical gameplay | v0.4.0 |

### What's Shared (via `roguelike-core`)

| Feature | Module | Tier |
|---------|--------|------|
| **Cross-platform seeds** | `core::rules::seed_code` | Rules — tier inferred from seed numeric value |
| **Balance constants** | `core::rules::balance` | Rules — single source of truth |
| **Combat formula** | `core::rules::damage::damage()` | Rules — `const fn` |
| **Monster tables** | `core::rules::monster_table` | Rules — MonsterKind, pick_monster(), stat tables |
| **Room geometry** | `core::rules::map::Room` | Rules — struct + intersection |
| **Item definitions** | `core::rules::items` | Rules — type IDs, stat lookup tables |
| **Leveling tables** | `core::rules::leveling` | Rules — XP thresholds, stat growth |
| **Depth scaling** | `core::rules::balance` | Rules — monster scaling per floor |
| **Mood thresholds** | `core::rules::balance` | Rules — flee/enrage trigger values |
| **GameStep trait** | `core::game_step` | Rules — `#[cfg(feature = "std")]`, uniform tier interface |
| **Direction enum** | `core::command::Direction` | Rules — 8-way movement, no coord offsets |
| **GameEvent enum** | `core::rules::message::GameEvent` | Rules — structured messages (Copy, no_std) |
| **Spawn mechanics** | `core::tier_micro::spawn` / `core::spawn` | Per-tier — placement using SpawnDirective |
| **Map generation** | `core::tier_micro::map` / `core::map` | Per-tier — same algorithm, tier-specific params |
| **Entity system** | `core::tier_micro::entity` / `core::entity` | Per-tier — fixed array (micro) vs Vec (standard) |
| **FOV** | `core::tier_micro::fov` / `core::fov` | Per-tier — Bresenham (micro/compact) vs shadowcasting (standard) |
| **AI** | `core::tier_micro::ai` / `core::ai` | Per-tier — greedy chase (micro/compact) vs A* (standard) |
| **Game loop** | `core::tier_micro::game` / `core::game` | Per-tier — GameState with tier-appropriate types |

### Tier Divergence

For the complete tier divergence table documenting intentional differences
between tiers (FOV, pathfinding, entity cap, messages, map storage, PRNG, XP
scaling, save format), see the
[capability tier reference](capability-tier-reference.md#110-tier-divergence).

---

## 10. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| rust-mos single maintainer abandons project | Medium | High | llvm-mos (LLVM backend) has separate active team; Mikael Lund maintains parallel fork; Docker images are self-contained; code can be ported to cc65 C if needed |
| Code generation produces slow hot paths | Medium | Medium | Profile on VICE; rewrite hot paths as C FFI calls with inline 6502 assembly; raw pointer arithmetic >> slice access |
| Code size exceeds budget | Low | Medium | POC measured 13 KB; LTO + `opt-level = "s"` + feature-gated mos-hardware keep size down; core provides game logic directly |
| No inline assembly for Kernal calls | Certain | Low | C FFI wrappers (proven by chirp8-c64); mos-hardware's `cbm_kernal` module; function pointer stubs for simple instructions |
| PRNG edge cases (POC bug: rejection sampling overflow) | Resolved | N/A | Fixed in POC — power-of-2 spans handled with early return; shared LfsrRng carries the fix |
| Keyboard input on emulators (POC bug: CIA Port B conflict) | Resolved | N/A | Fixed in POC — use Kernal buffer + Port A joystick only |
| Tier complexity in core | Low | Low | Tier micro is an additive submodule — core's standard-tier code is unchanged. No cfg gates on types or collections within a tier. Rules modules work across all tiers. |
| `GameState` struct access on 6502 | Low | Medium | With llvm-mos static stack allocation + LTO, a `GameState` local in `main()` gets a fixed address. Profile on VICE after integration; extract hot arrays to statics if needed |
| FOV too slow on real hardware | Low | High | Already budgeted at ~7,500 cycles; validated in POC |
| Custom charset looks bad | Medium | Low | Iterate with CharPad; study Sword of Fargoal, chirp8-c64's 2x2 tile approach |
| UII+ command interface underdocumented | Medium | Medium | UII+ firmware is open source; test on real hardware early |
| Network timeout blocks game loop | Medium | High | Non-blocking polls with timeout; game remains playable offline |
| Cross-platform seed divergence | Low | Medium | Golden snapshot tests in CI; shared mapgen is deterministic; LFSR period test catches PRNG regressions |
| Directive pattern complexity | Low | Low | SpawnDirective is a simple struct; pattern is well-established in ECS architectures |

---

## 11. Technical Reference

Companion documents provide detailed implementation references:

- **[Capability Tier Reference](capability-tier-reference.md)** — Cross-platform
  tier architecture, sharing matrix, type sizing, seed system, tier divergence.
- **[C64 Platform Guide](c64-platform-guide.md)** — C64-specific hardware
  guidance, including:
  - **PRNG overflow bug** (§2) — 8-bit rejection sampling fix carried into `LfsrRng`
  - **CIA port multiplexing** (§3) — why direct matrix scanning fails on emulators
  - **Static stack allocation** (§4) — how to keep llvm-mos's key optimization working
  - **Turn timing** (§5) — measured cycle budgets: ~10,000 cycles/turn, ~660 cycles/frame continuous raster overhead
  - **Code style guide** (§6) — which Rust abstractions help vs. hurt on the 6502
- **[Testing Strategy](testing-strategy.md)** — Project-wide testing approach
  (core tests, determinism, property tests, CI verification).

---

## 12. Decisions and Remaining Questions

### Resolved

1. ~~**Map size**~~ **Decided: Scrolling 40x21 viewport over 64x48 map.**
   Larger maps enable richer dungeons. Memory cost is ~3 KB tiles (up from
   840 B) — fits within the 16 KB headroom. The viewport uses a single dense
   status row plus 3 message lines, with modal inventory (§6.1). Map
   dimensions are locked — they flow through `roguelike_core::rules::balance` and
   affect cross-platform seeds. Scrolling added to Phase 2a (§8).

2. ~~**Multiple dungeon levels?**~~ **Resolved.** The
   [gameplay implementation plan](design/gameplay-implementation-plan.md)
   Phase 2 designs stairs and multi-level dungeons. On the C64, swap map data
   on descend (~3 KB per floor for 64x48); optionally hold 2 floors in memory
   (~6 KB) for quick backtracking. Derive floor seeds from the base
   seed: `LfsrRng::new(base_seed.wrapping_add(depth))`. MultiRogueLike
   validates this approach — it uses a 3D grid with stairs connecting levels.

3. ~~**mos-hardware version pinning**~~ **Decided: Vendor.** Copy
   `mos-hardware` into the workspace and pin the version. The Docker workflow
   and `cc` build dependency uncertainty make crates.io risky. Vendoring
   avoids upstream breakage and ensures reproducible builds.

4. ~~**alloc vs. no-alloc**~~ **Decided: No-alloc.** The POC validates this
   approach. Simpler, predictable memory, no heap fragmentation risk on a
   platform with ~46 KB usable RAM.

5. ~~**Player sprite**~~ **Decided: v0.5.0.** Charset-only rendering for
   v0.4.0; hardware sprite overlay after charsets are stable. A 24x21 sprite
   enables smooth inter-tile movement, idle animation, and dramatic visual
   distinction — the same approach used by *Sword of Fargoal* (1982). Cost:
   ~126 cycles/frame for sprite DMA + ~1 KB for animation data. Monster
   sprites could follow later. See the
   [C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
   §6 for the full design.

6. ~~**Level transition animation**~~ **Decided: v0.5.0.** Simple
   clear-and-redraw for v0.4.0; FLD wipe as polish in v0.5.0. Cost: ~3,150
   cycles total per transition. FLD achieves 80% of visual impact with less
   code than linecrunch. See the demo techniques analysis §3.

7. ~~**Separate `roguelike-rules` crate vs. unified core**~~ **Decided:
   Capability tier hierarchy in unified core.** Rather than creating a separate
   `roguelike-rules` crate or refactoring all of core to C64 types,
   `roguelike-core` is organized around capability tiers. Tier micro
   (`core::tier_micro`) uses `u8` coordinates, `u8` stats, fixed-size arrays,
   and bitfields — `no_std`-compatible. Tier standard (top-level core) retains
   `i32` types and `std` collections. Rules modules (damage, balance, items,
   leveling, monster tables, GameEvent, Direction) produce pure values and
   directive structs across all tiers. Spawn mechanics remain per-tier. The
   C64 depends on core with `default-features = false` and uses
   `core::tier_micro` + `core::rules` directly. This eliminates ~1,200 lines
   of C64 engine reimplementation while keeping the PC's standard-tier code
   unchanged.

### Remaining Open

8. **PAL vs. NTSC timing:** Turn-based so gameplay unaffected, but animation
   timing and SID tuning differ. Detect at startup with raster counter check.

9. **How much C FFI?** Currently zero in the POC. Kernal calls (disk I/O) will
   need C wrappers. mos-hardware's `cbm_kernal` module may handle this, but
   we should audit whether its `cc` build dependency works in the Docker
   workflow. Audit after mos-hardware migration (Phase 2a step 6).

10. **mos-hardware code size:** Budget ~500-1500 bytes for mos-hardware's
   contribution to the binary (volatile-register wrappers, bitflags structs).
   Measure after migration (Phase 2a step 6) and compare against the POC's
   13 KB baseline.

---

## 13. Conclusion

The rust-mos approach is validated. The POC proves that Rust compiles to viable
6502 machine code for a complete roguelike game loop in 13 KB — competitive
with the original cc65 estimate of 12-18 KB.

The main advantages over the cc65 approach:

1. **Single language** — The entire project (6 workspace crates + C64) stays
   in Rust. The C64 will be another frontend in the existing multi-frontend
   architecture (terminal, SSH, MCP, C64).
2. **Capability tier hierarchy** — `roguelike-core` is organized around
   capability tiers: tier micro (`u8` coords, LFSR-16, Bresenham FOV, `no_std`)
   for C64 and cross-platform play; tier compact (`i16` coords, LFSR-32) for
   GBA; tier standard (`i32` coords, ChaCha20, shadowcasting, `std`) for
   Vita/PC. Each platform compiles its native tier plus all lower tiers. Rules
   modules (damage, balance, items, leveling, monster tables, GameEvent,
   Direction) produce pure values and directive structs across all tiers.
   Core's standard-tier code is unchanged — tier micro is an additive
   submodule, not a rewrite. This ensures gameplay feature parity across
   platforms as the
   [gameplay implementation plan](design/gameplay-implementation-plan.md)
   features are implemented.
3. **Cross-platform seeds with zero divergence** — Within a tier, ALL platforms
   use the same PRNG, map generation, FOV, and pathfinding algorithms. A C64
   player's 4-character seed code plays identically on every platform — same
   map, same visibility, same tactical experience. No FOV divergence, no
   "challenge mode" caveat. Seeds are clean (no tier prefix); the game infers
   the tier from the seed's numeric value and shows platform compatibility in
   the UI. Players choose compatibility level when starting a new game. Daily
   challenges use micro-tier seeds, playable on everything from the C64 to
   the PC.
4. **Type safety** — Ownership model, pattern matching, and `Option`/`Result`
   prevent entire classes of bugs that plague 6502 C/assembly.
5. **Ecosystem** — `mos-hardware` provides production-quality hardware access;
   `mos-alloc` and `mos-test` are available if needed.
6. **Faster iteration** — Cargo workflow with Docker, `write_volatile` for
   hardware access, Rust's expressive type system for game logic.

The main risks:

1. **Single maintainer** — Mitigated by Docker version pinning and the active
   llvm-mos backend.
2. **No inline assembly** — Mitigated by C FFI wrappers and mos-hardware.
3. **Code generation quality** — Mitigated by raw pointer patterns in hot paths
   and the validated POC binary size.

The implementation plan is structured across three milestones:

- **v0.4.0** (~13 weeks including 2 weeks buffer) — Prep PR extracts
  `rules/balance.rs` and adds Direction + GameEvent. Core game with scrolling
  viewport, raster effects, SID audio, custom charsets, save/load, and the
  tier micro module in core. Shippable standalone release.
- **v0.5.0** (+6 weeks) — UII+ networking: cloud saves, leaderboards, daily
  seeds, auto-update. Plus deferred polish: player sprites, level transition
  animations.
- **v0.6.0** (+6 weeks) — Advanced networking: spectation relay, MCP client
  mode, AT Protocol bridge.

Timeline compression factors:
- Phase 0 (POC) is already complete
- Rust development is faster than C/assembly for game logic
- mos-hardware eliminates boilerplate hardware register code
- Tier micro as a submodule means zero disruption to existing PC code

The C64 roguelike wouldn't just be a downport — it would be the most
interesting client in the fleet. And with tier-encoded seeds, a C64 player and
a PC player compete on genuinely identical dungeons — same map, same FOV, same
algorithms.
