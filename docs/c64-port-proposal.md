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
   geometry, and the PRNG live in a single `no_std` crate (`roguelike-rules`)
   compiled for both targets. The same seed produces the same dungeon on both
   platforms.
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
allocation (§11.3) — we share the algorithms directly by writing them at the
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

| Resource | Current (Rust/PC) | Commodore 64 |
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
| Coordinate space | `type Coord = i32` (`types.rs`) | `u8` (0-39 x, 0-21 y) |
| Stat values | `type Stat = i32` (`types.rs`) | `u8` (0-255) |
| Max entities | `MAX_ENTITIES = 1024` (types.rs notes "C64 = 16") | `MAX_ENTITIES = 16` |
| PRNG | `rand::StdRng` (ChaCha20, u64 seed) | Galois LFSR (16-bit, u16 seed) |
| Shared types | `roguelike-rules`: `u8` coords, `Room`, `LfsrRng` | Same — shared crate compiles for both |
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
  roguelike-rules contributes ~1.5 KB (mapgen, prng, combat, rooms,
  items, leveling, depth scaling, wandering spawn, mood thresholds)
Map tile data:            840 B   (40 x 21 = 840 tiles, 1 byte/tile)
Structural wall bits:     105 B   (840 bits)
Explored bitfield:        105 B   (840 bits)
Visible bitfield:         105 B   (840 bits)
Entity parallel arrays:   208 B   (16 entities x 13 bytes across arrays)
  Base (POC):  10 arrays x 16 = 160 B (x, y, hp, max_hp, atk, def,
               kind, ai, alive, sight)
  Gameplay:     3 arrays x 16 =  48 B (xp_value, mood, memory)
Room list:                 48 B   (12 rooms x 4 bytes)
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
Total:                  ~30 KB    (~16 KB headroom remaining)
```

The POC validates the baseline: 13 KB code + ~2 KB static data = ~15 KB total.
Gameplay features (items, XP, stairs, mood — see the
[gameplay implementation plan](design/gameplay-implementation-plan.md)) add ~1
KB of data and ~2 KB of code. The production build adds ~5 KB over the POC
baseline for raster effects, per-zone charsets, sprite data, and the expanded
demo-technique-driven rendering pipeline (see
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)).
Even with all production additions (SID, charsets, sprites, raster effects,
saves, networking, gameplay features), we stay well under the 46 KB budget.

---

## 5. Code Sharing Strategy

The most compelling advantage of using rust-mos over cc65 is the ability to
share Rust code between the PC and C64 codebases. This section describes a
practical approach grounded in one key insight: **write the shared code at the
C64's abstraction level, and let the PC call down to it.**

The PC engine (`roguelike-engine`) uses `Vec<Tile>`, `HashSet<Pos>`, `String`,
`HashMap`, and `rand::StdRng`. The C64 engine uses `static mut [u8; 840]`,
bitfields, `&'static [u8]`, and a 16-bit Galois LFSR. These storage layers
are fundamentally incompatible, and abstracting them behind traits would add
indirection that defeats llvm-mos's static stack allocation (§11.3).

But the **algorithms** are the same. The map generation loop, combat formula,
room intersection check, monster spawning, and PRNG are line-for-line identical
in both codebases — just operating on different types. The solution: implement
these algorithms once using primitive types (`u8`, `&mut [u8]`, `&mut [Room]`)
that both platforms can use directly.

### 5.1 Existing Architecture

The project is already a **6-crate workspace** with clean separation between
game logic, save persistence, rendering, and platform-specific binaries:

```
roguelike/
  Cargo.toml                        # workspace: 7 members + libudev patch
  crates/
    rules/        (roguelike-rules)    # NEW: shared #![no_std] game rules + balance data
    engine/       (roguelike-engine)   # PC game engine (library, std) — renamed from core/
    saves/        (roguelike-saves)  # SaveBackend trait abstraction
    tui/          (roguelike-tui)    # Terminal rendering (crossterm)
    terminal/     (roguelike-terminal) # Desktop app (keyboard + gamepad)
    ssh/          (roguelike-ssh)    # Multi-user SSH server
    mcp/          (roguelike-mcp)    # MCP server for AI integration
    c64/          (roguelike-c64)    # C64 port (standalone, no_std, no_alloc)
    libudev-sys-dlopen/             # dlopen libudev (patched dependency)
```

The **dependency graph** puts the shared crate at the bottom:

```
roguelike-rules  (#![no_std], zero deps — the shared soul)
    ↓                              ↓
roguelike-engine  (std, game engine)  roguelike-c64  (no_std, C64 binary)
    ↓
roguelike-saves (SaveBackend trait)
    ↓
roguelike-tui   (crossterm rendering + game loop)
    ↓
├── roguelike-terminal  (desktop: keyboard + gamepad + local saves)
├── roguelike-ssh       (SSH server: per-user sessions + accounts)
└── roguelike-mcp       (MCP server: AI tool interface, core only)
```

> **Naming rationale.** The shared crate is called `roguelike-rules` (not
> `roguelike-common`) because it contains the *game rules* — balance constants,
> algorithms, and type definitions that both platforms must agree on. The PC
> runtime is called `roguelike-engine` (not `roguelike-core`) because it is
> the *engine* that implements those rules with `std` facilities. The
> relationship reads naturally: "the engine implements the rules." Cargo's
> feature unification model prevents a single crate from being `std` for one
> consumer and `no_std` for another, which is why the shared rules live in
> their own crate rather than inside the engine.

### 5.2 Design Principle: Shared Code IS C64 Code

The shared crate uses the C64's natural types: `u8` coordinates, `&mut [u8]`
tile buffers, fixed-size `&mut [Room]` arrays, and a concrete `LfsrRng` struct.
No traits. No generics. No associated types. No type aliases.

This means:
- The C64 uses shared code **directly** — zero conversion cost, zero indirection.
- The PC **calls down** to shared code and promotes results to its own types
  (`u8` → `i32`, `Room` → `Rect`, `&[u8]` → `Vec<Tile>`).
- The call graph is fully transparent to both `rustc` and `llvm-mos`.
- Someone reading the shared crate sees a clear, textbook dungeon generator —
  not a framework.

**Why not type aliases?** The PC engine uses `type Coord = i32` and `type Stat
= i32` to distinguish spatial values from combat statistics — useful when both
are `i32`. In the shared crate, everything is `u8`: coordinates, HP, attack,
defense, tile types, entity indices. A `type Coord = u8` alias doesn't
distinguish coordinates from anything else — it just adds a layer of
indirection between the reader and the actual data width. In ~400 lines of
`no_std` code targeting the 6502, explicit `u8` is both clearer and more
honest. The PC's own type aliases remain unchanged — they're a concern of
`roguelike-engine`'s API surface, not the shared crate.

`cfg`-adaptive aliases (`#[cfg(c64)] type Coord = u8; #[cfg(not(c64))] type
Coord = i32;`) are worse: they make "shared" code compile with different
overflow semantics on each platform, defeating the purpose of sharing.

**Why not traits?** On the 6502, generic/trait code can prevent llvm-mos from
performing static stack allocation — the compiler needs to see the complete
call graph at link time. Trait methods through generics create indirect
references that obstruct this analysis. Concrete functions on primitive types
produce a clean call graph with guaranteed optimal code generation. See §11.3
and §11.5.

### 5.3 Shared Crate: `roguelike-rules`

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

### 5.4 Shared PRNG: `LfsrRng`

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
abstractions that genuinely improve 6502 code generation — see §11.5 for why.

On the PC, `LfsrRng` is used for "challenge mode" (§6.14) while `StdRng`
remains the default for normal gameplay.

### 5.5 Shared Map Generation

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

### 5.6 How Each Platform Uses Shared Code

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

### 5.7 What Can Be Shared

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
| FOV (can_see) | **No** | Separate impls | Intentionally different — see §6.3 |
| A* pathfinding | **No** | PC only | Requires heap (HashMap, BinaryHeap) |
| Rendering | **No** | Separate impls | crossterm vs VIC-II screen writes |
| Input handling | **No** | Separate impls | crossterm vs CIA keyboard/joystick |
| Save persistence | **No** | Separate impls | JSON vs binary; different backends |
| Data loading | **No** | Separate impls | TOML parse vs ROM constants |
| Entity storage | **No** | Separate impls | `Vec<Entity>` vs parallel arrays |
| Item storage | **No** | Separate impls | `HashMap<Pos, Vec<Item>>` vs sparse parallel arrays (see §6.7) |
| Message log | **No** | Separate impls | `Vec<String>` vs `[u8; 160]` circular buffer |

### 5.8 Shared Balance Constants

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
pub const C64_MAP_WIDTH: u8 = 40;
pub const C64_MAP_HEIGHT: u8 = 21;

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
Phase 6 (property bitfields) is PC-only for v1 — the C64 has no immediate use
for the `u64` property system, though a `u8` subset could be added later.

### 5.9 Testing Strategy

1. **Shared crate tests**: `cargo test -p roguelike-rules` on the host with
   standard rustc. No emulator needed.

2. **CI no_std verification**: Cross-compile for `thumbv6m-none-eabi` to catch
   accidental `std` dependencies:
   ```bash
   cargo check -p roguelike-rules --target thumbv6m-none-eabi
   ```

3. **Balance drift detection**: A CI test in `roguelike-engine` verifies that
   `game.toml` default values match `roguelike-rules::balance` constants.

4. **Cross-platform seed determinism**: A test generates a dungeon with a
   known seed using `roguelike-rules::mapgen::generate()` and compares the
   resulting tile layout against a stored golden snapshot. This catches any
   accidental changes to the generation algorithm or PRNG.

5. **Shared property tests**:
   ```rust
   #[test]
   fn damage_never_negative() {
       for atk in 0..=20u8 {
           for def in 0..=20u8 {
               assert!(combat::damage(atk, def) <= atk);
           }
       }
   }

   #[test]
   fn lfsr_has_full_period() {
       let mut rng = LfsrRng::new(0xACE1);
       let start = rng.state();
       for i in 0u32..65536 {
           rng.next_u8(); rng.next_u8();
           if rng.state() == start {
               assert_eq!(i + 1, 65535, "LFSR period too short");
               return;
           }
       }
       panic!("LFSR did not cycle");
   }

   #[test]
   fn room_intersection_is_symmetric() {
       let a = Room { x: 2, y: 2, w: 5, h: 5 };
       let b = Room { x: 4, y: 4, w: 5, h: 5 };
       assert_eq!(a.intersects(&b), b.intersects(&a));
   }
   ```

6. **C64-specific tests**: Use `mos-test` crate for target-specific code that
   must run on the MOS simulator.

7. **Integration testing**: Run the .PRG in VICE with automated input scripts
   (VICE supports `-keybuf` for automated key injection).

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

**Recommendation:** Start with fixed 40x21 maps (no scroll) for v1 — the POC
validated 40x22, and the 1-row reduction gains a third message line and richer
status information. Add scrolling viewport over 64x48 maps in v1.1. The map
dimensions must be set before cross-platform seeds are published, since they
flow through `roguelike-rules::balance` (see §5.8).

### 6.2 Map Generation

Map generation uses the shared `roguelike-rules::mapgen::generate()` function
on both platforms, ensuring identical room placement, corridor routing, and
dungeon topology for any given seed. See §5.5 for the shared function and §5.6
for platform integration.

The C64 calls the shared function directly with its `static mut` tile buffer.
The PC calls it in "challenge mode" for cross-platform seed compatibility, and
uses its own `Map::generate()` with `StdRng` for normal play.

Key parameters for C64 maps:
- **Map size**: 40x21 (fixed — see §6.1 for viewport rationale)
- **Max rooms**: 12
- **Room sizes**: 3-7 tiles
- **PRNG**: `LfsrRng` (shared Galois LFSR)
- **Storage**: `static mut TILES: [u8; 840]`
- **Structural walls**: Bitfield (`[u8; 105]`)

### 6.3 Field of View

**PC:** Recursive shadowcasting with `f64` slopes and `HashSet<(i32,i32)>`.

**C64:** Bresenham raycasting (integer-only) with precomputed perimeter table.

```rust
// Precomputed perimeter offsets for radius 6 — 40 ray targets
const PERIMETER: [(i8, i8); 40] = [
    (6, 0), (6, 1), (6, 2), (5, 3), (5, 4), (4, 5), (3, 5), (2, 6),
    // ... (computed at compile time)
];
```

Visibility stored as `[u8; 110]` bitfield. Cost: ~150 tile checks per FOV
recompute = ~7,500 cycles = ~7.5 ms. Imperceptible.

**FOV is intentionally not shared.** The two algorithms produce slightly
different visibility results: the PC's shadowcasting handles thin diagonal walls
and is symmetric (if A sees B, B sees A), while the C64's Bresenham raycasting
can have angular gaps between rays and uses a different algorithm for monster
LOS checks (`can_see()` via single ray) than for the player viewport
(`compute_fov()` via 40 rays).

For cross-platform challenges, this means the same dungeon layout plays
slightly differently on each platform — the C64's line-of-sight creates a
grittier, more unpredictable fog of war. This is documented as an intentional
platform characteristic, not a bug. The shared map and combat ensure the
dungeon is structurally identical; the FOV difference is what makes each
platform's experience distinctive.

### 6.4 Entity System

**C64 approach — parallel arrays:**

The POC uses separate `static mut` arrays for each entity field, which produces
tighter 6502 indexed addressing than an array-of-structs:

```rust
// --- Core arrays (POC, 10 arrays = 160 bytes) ---
static mut ENT_X:      [u8; 16]   = [0; 16];
static mut ENT_Y:      [u8; 16]   = [0; 16];
static mut ENT_HP:     [u8; 16]   = [0; 16];
static mut ENT_MAX_HP: [u8; 16]   = [0; 16];
static mut ENT_ATK:    [u8; 16]   = [0; 16];
static mut ENT_DEF:    [u8; 16]   = [0; 16];
static mut ENT_KIND:   [u8; 16]   = [0; 16];
static mut ENT_AI:     [u8; 16]   = [0; 16];
static mut ENT_ALIVE:  [bool; 16] = [false; 16];
static mut ENT_SIGHT:  [u8; 16]   = [0; 16];

// --- Gameplay arrays (gameplay-implementation-plan.md, +3 arrays = 48 bytes) ---
static mut ENT_XP_VALUE: [u8; 16] = [0; 16];   // Phase 4: XP awarded on death
static mut ENT_MOOD:     [i8; 16] = [0; 16];   // Phase 5: creature mood (-128..127)
static mut ENT_MEMORY:   [u8; 16] = [0; 16];   // Phase 5: bitflags (SAW_ALLY_DIE, etc.)
```

Total: **13 arrays x 16 entries = 208 bytes** (see §4.2 memory budget).

Stat tables are ROM constants populated from `roguelike-rules::balance`.
Names are `&'static [u8]` references to byte string literals — no heap.

Player-specific state (XP total, level, depth, inventory, equipment) lives
outside the entity arrays in dedicated `static mut` variables, matching the PC
engine's design of keeping progression data on `GameState` rather than `Entity`.

### 6.5 Monster AI

Single-ray Bresenham LOS check per monster (`can_see()` in `fov.rs`), then
greedy chase with 3-candidate movement. Wander→Chase transition on player
detection. All implemented and validated in the POC (`ai.rs`, 120 lines).

### 6.6 Combat System

`damage = max(0, attacker_atk - defender_def)`. The formula is a `const fn` in
`roguelike-rules::combat` — the single source of truth. Both platforms wrap it
with their respective logging and entity access patterns.

### 6.7 Items and Inventory (C64 Storage Design)

The [gameplay implementation plan](design/gameplay-implementation-plan.md)
Phase 3 adds items, a fixed-size inventory, and equipment. On the PC, floor
items use `HashMap<Pos, Vec<Item>>` and inventory uses `Vec<Option<Item>>` —
both require heap allocation. The C64 needs a `no_std` equivalent.

**Floor items — sparse parallel arrays:**

Rather than a full map overlay (840 bytes for one-item-per-tile), use a sparse
list capped at 32 items per floor. This mirrors the approach used by
[MultiRogueLike](https://github.com/LeifBloomquist/MultiRogueLike), which
stores items in the same entity list rather than as a map layer.

```rust
static mut ITEM_X:     [u8; 32] = [0; 32];   // x coordinate
static mut ITEM_Y:     [u8; 32] = [0; 32];   // y coordinate
static mut ITEM_TYPE:  [u8; 32] = [0; 32];   // item type ID (0 = empty slot)
static mut ITEM_COUNT: u8 = 0;               // active items on floor
```

Cost: **97 bytes** (vs. 840 bytes for a full map overlay). Item lookup at a
position is a linear scan over `ITEM_COUNT` entries — at 32 max items, this
is ~200 cycles. Negligible for a turn-based game.

Item type IDs map to stat tables in `roguelike-rules::items`:

```rust
// roguelike-rules/src/items.rs

pub const ITEM_NONE: u8 = 0;
pub const ITEM_HEALING_POTION: u8 = 1;
pub const ITEM_STRENGTH_POTION: u8 = 2;
pub const ITEM_SHORT_SWORD: u8 = 3;
pub const ITEM_LONG_SWORD: u8 = 4;
pub const ITEM_LEATHER_ARMOR: u8 = 5;
pub const ITEM_SCROLL_MAPPING: u8 = 6;

/// Heal amount for potion items (0 = not a potion).
pub const fn heal_amount(item_type: u8) -> u8 {
    match item_type {
        ITEM_HEALING_POTION => 10,
        _ => 0,
    }
}

/// ATK bonus for equipment items (0 = not a weapon).
pub const fn atk_bonus(item_type: u8) -> u8 {
    match item_type {
        ITEM_SHORT_SWORD => 2,
        ITEM_LONG_SWORD => 4,
        _ => 0,
    }
}

/// DEF bonus for equipment items (0 = not armor).
pub const fn def_bonus(item_type: u8) -> u8 {
    match item_type {
        ITEM_LEATHER_ARMOR => 2,
        _ => 0,
    }
}
```

**Player inventory and equipment:**

```rust
static mut INVENTORY: [u8; 10] = [0; 10];     // 10 slots, item type ID (0 = empty)
static mut EQUIPPED_WEAPON: u8 = 0;            // item type ID
static mut EQUIPPED_ARMOR: u8 = 0;             // item type ID
```

Cost: **12 bytes.** The `effective_attack()` and `effective_defense()` helpers
in `roguelike-rules::combat` take base stats plus equipment type IDs and
return the effective values — shared between both platforms.

**Inventory UI — modal overlay:** The inventory screen is a NetHack-style modal
overlay: pressing `i` writes an item list over the map area (rows 0-20),
showing slot letters, item names, and "(equipped)" markers. Any key dismisses
the overlay and redraws the map. This avoids dedicating permanent screen rows
to inventory display — the equipped weapon and armor are shown as single-glyph
indicators on the status bar (§6.1). Pickup (`g`), drop (`d`), use (`u`), and
equip (`e`) commands work without opening the inventory for common actions.

**Item spawning** uses the existing `roguelike-rules::spawn` pattern: weighted
random selection into rooms, with a `max_items_per_room` cap. Item spawn weights
and `min_depth` thresholds live in `roguelike-rules::items`.

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
(see §8, Phase 2 step 6) enables per-scanline VIC-II register changes that add
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
and add no per-turn rendering cost. The total continuous overhead for all
atmospheric effects is ~660 cycles/frame (~3.4% of the frame budget) — one of
the highest impact-to-cost ratios available on the VIC-II.

### 6.9 Custom Character Set

Design **three 2 KB charsets** (6 KB total — see §4.2) with per-zone switching
via raster interrupts. The VIC-II's charset pointer (`$D018` bits 1-3) is
changed at zone boundaries by the raster interrupt chain (§8, Phase 2 step 6),
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
set up in Phase 2 step 6 (§8). See the
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

### 6.14 Seed System and Cross-Platform Seed Sharing

The seed system has two layers: **platform-local seeds** for reproducible
dungeons within a platform, and **cross-platform challenge seeds** for
identical dungeons across PC and C64.

#### Platform-Local Seeds

**C64:** 16-bit seed (0-65535) from `LfsrRng`. Displayed as 4-character
base36 code on title and death screens. Seeded from CIA timer jitter at
first keypress. Validated in the POC.

**PC (normal mode):** 64-bit seed from `rand::StdRng` (ChaCha20). Displayed
as base36 seed code via the existing `seed_code.rs` system. The full seed
code format (`<base36_seed>[-<WxH>][<preset>]`) supports custom dimensions
and map presets.

These are **not cross-compatible** — the same numeric seed produces different
dungeons on each platform because they use different PRNG algorithms (ChaCha20
vs LFSR) and different map dimensions (80x40 vs 40x21).

#### Cross-Platform Challenge Mode

The shared `roguelike-rules` crate enables a **challenge mode** on the PC that
generates C64-compatible dungeons:

1. PC creates a `LfsrRng` from a u16 seed (same PRNG as C64).
2. PC calls `roguelike-rules::mapgen::generate()` with C64 parameters
   (40x21 map, 12 rooms, size 3-7).
3. Same seed + same LFSR + same function = **identical dungeon** on both
   platforms.

Since C64 seeds are u16 (0-65535), they encode as 3-4 base36 characters. A
C64 player sees `r7z` on their death screen, types it into the PC version's
challenge mode, and gets the same dungeon layout.

**Seed code format extension:**

```
r7z3kq              → PC normal mode: 80x40, StdRng (existing behavior)
r7z3kq-120x60       → PC normal mode: 120x60, StdRng
r7z                  → Challenge mode: u16 seed, 40x21, LfsrRng
r7z-c                → Explicit challenge suffix (optional — short codes
                       are unambiguous since u16 max = "1ekf" in base36)
```

The server's daily seed endpoint sends a u16, and both platforms generate from
it — the PC in challenge mode, the C64 natively. Leaderboards for daily
challenges are cross-platform; leaderboards for normal PC play are separate.

#### FOV Divergence in Challenges

Cross-platform challenges produce identical map layouts and monster placements,
but FOV differences mean the two platforms "see" the dungeon differently (§6.3).
This is intentional — the same dungeon is a different tactical experience on
each platform. Leaderboard rankings reflect the platform-specific challenge
rather than pixel-identical play.

---

## 7. Architecture Mapping

The project has four frontends — terminal, SSH, MCP, and C64 — all driven by
the same game engine (`roguelike-engine`). The C64 is unique: it reimplements
the engine in `no_std` Rust rather than importing `roguelike-engine` directly,
because core depends on `String`, `Vec`, `serde`, and `rand`. The shared
`roguelike-rules` crate bridges the gap by providing the core algorithms and
balance data that both engines consume.

### 7.1 How the Frontends Compare

```
                    roguelike-engine          roguelike-c64
                    ──────────────          ─────────────
Depends on:         serde, rand, toml       nothing (zero deps)
                    roguelike-rules        roguelike-rules
Entity storage:     Vec<Entity>             parallel static mut arrays
Map storage:        Vec<Tile>               static mut [u8; 840]
FOV:                f64 shadowcasting       integer Bresenham raycasting
Pathfinding:        A* (HashMap)            greedy chase (no heap)
Data loading:       game.toml (runtime)     const ROM tables (from balance.rs)
Message log:        Vec<String>             [u8; 160] circular buffer
Save format:        JSON (serde_json)       binary (manual serialization)
PRNG (normal):      StdRng (ChaCha20)       LfsrRng (shared crate)
PRNG (challenge):   LfsrRng (shared crate)  LfsrRng (shared crate)
                    ──────────────          ─────────────
Shared via:              roguelike-rules
                    (PRNG, mapgen, combat, room geometry, spawn logic,
                     structural walls, balance constants, seed codes,
                     item defs, leveling tables, depth scaling, mood)
```

### 7.2 Module-by-Module Mapping

```
Workspace Crate / Module     →  C64 Module (Rust)            Size Est.  POC Actual
──────────────────────────────────────────────────────────────────────────────────
NEW: rules/src/              →  used by both                 ~1.5 KB   (pending)
  roguelike-rules               (PRNG, mapgen, combat, rooms, balance, seeds,
                                  items, leveling, depth scaling, mood thresholds)

engine/src/map.rs   (22 KB)  →  c64/src/map.rs               ~0.5 KB   (wraps rules)
  Map::generate()                (calls rules::mapgen::generate())
  Map::is_walkable()             (tile lookup on static mut buffer)

engine/src/fov.rs   (6.9 KB) →  c64/src/fov.rs               ~1.5 KB    190 lines
  compute_fov()                  (Bresenham raycasting — platform-specific)
  can_see()                      (single-ray LOS — platform-specific)

engine/src/entity.rs (4 KB)  →  c64/src/entity.rs            ~1 KB      248 lines
  Entity struct                  (parallel arrays, stat tables from balance.rs)

engine/src/combat.rs (3.7 KB) → c64/src/combat.rs            ~0.3 KB     38 lines
  melee_attack()                 (wraps rules::combat::damage())

engine/src/ai.rs    (13 KB)  →  c64/src/ai.rs                ~0.8 KB    138 lines
  run_monster_turns()            (LOS + greedy chase + wander)

engine/src/spawn.rs (4.4 KB) →  c64/src/entity.rs            ~0.5 KB    (included)
  spawn_monsters()               (calls rules::spawn functions)

engine/src/game.rs  (91 KB)  →  c64/src/main.rs              ~1.5 KB    185 lines
  GameState::step()              (main turn loop — drastically simplified)

engine/src/message_log.rs    →  c64/src/msglog.rs            ~0.8 KB    152 lines
  MessageLog                     (4-slot circular buffer, &[u8] not String)

engine/src/data.rs  (27 KB)  →  c64/src/entity.rs            embedded   (included)
  GameData / MonsterDef          (const ROM tables from rules::balance)
  game.toml parsing              (not needed — values from rules crate)

tui/src/render.rs            →  c64/src/render.rs            ~1.5 KB    239 lines
  CrosstermRenderer              (VIC-II screen + color RAM writes)

saves/src/lib.rs             →  c64/src/save.rs              ~0.8 KB    (pending)
  SaveBackend trait              (simplified: binary to floppy / UII+ HTTP)

N/A                          →  c64/src/c64.rs               ~1 KB      172 lines
  (no PC equivalent)             (C64 hardware registers — migrate to mos-hardware)

N/A                          →  c64/src/input.rs             ~1 KB      179 lines
  (crossterm in tui/)            (Kernal keyboard buffer + CIA joystick Port 2)

NEW: engine/src/item.rs      →  c64/src/items.rs             ~0.5 KB   (pending)
  Item, ItemKind                 (sparse arrays + pickup/drop/use/equip logic,
                                  stat lookups from rules::items)

NEW: engine/src/game.rs (XP) → c64/src/main.rs              embedded  (pending)
  player_xp, player_level       (XP tracking + level-up in main loop,
                                  tables from rules::leveling)

N/A (new)                    →  c64/src/sid.rs               ~1 KB      (pending)
  (no sound on PC)               (SID register writes via mos-hardware)
──────────────────────────────────────────────────────────────────────────────────
POC total:                       11 source files              1,898 lines = 13 KB
Production estimate:             15 source files             ~2,600 lines ≈ 18 KB
  (well within 46 KB budget; shared crate reduces C64-specific code by ~400 lines
   and ensures gameplay feature parity with the PC version)
```

### 7.3 Notable Size Ratios

The C64 reimplementations are dramatically smaller than their PC counterparts
because they drop heap allocation, serde, TOML parsing, menu systems, settings,
analytics, and accessibility features. The most dramatic example: `game.rs` goes
from 91 KB to 185 lines — the C64 game loop has no autorun, no menu state
machine, no undo/redo, no spectation frame capture.

With `roguelike-rules`, the C64 crate shrinks further — `map.rs` becomes a
thin wrapper around the shared `mapgen::generate()`, and `combat.rs` wraps
the shared `damage()` formula. The shared crate also eliminates the balance
drift risk: when monster stats change, both platforms update automatically.

---

## 8. Implementation Plan

### Phase 0: POC (Complete)

**Status: Done.** rust-mos proof-of-concept validated:
- 13 KB binary with complete game loop
- Runs on c64.emu (Android) and VICE
- All core systems functional: map gen, FOV, entities, combat, AI, rendering,
  input, messages

### Phase 1: Shared Crate + C64 Refactor (Weeks 1-2)

1. **Create `roguelike-rules` crate** — `#![no_std]`, zero dependencies.
   Implement `LfsrRng`, `Room`, `mapgen::generate()`, `combat::damage()`,
   `spawn::pick_monster()`, `structural::compute()`, balance constants, and
   `seed::encode()`/`seed::decode()`. See §5.3-5.5 for design.

2. **Wire up C64 crate** — Replace POC's inline PRNG, map generation, combat
   formula, and balance constants with imports from `roguelike-rules`. Refactor
   `prng.rs` to use `LfsrRng` struct (passed as `&mut` instead of `static mut`
   global). See §11.5 for C64 code style guidance.

3. **Wire up PC crate** — Add `roguelike-rules` dependency to `roguelike-engine`.
   Add `Map::generate_c64_compatible()` for challenge mode. Add CI test that
   `game.toml` defaults match `roguelike-rules::balance` constants.

4. **Add shared tests** — Property tests for LFSR period, damage formula,
   room intersection symmetry, mapgen determinism golden snapshot. CI
   `thumbv6m-none-eabi` check for `no_std` compliance.

5. **Migrate C64 crate to mos-hardware** — Replace hand-rolled `poke`/`peek`
   wrappers with `mos-hardware`'s type-safe VIC-II, CIA, and SID access.
   Adopt `JoystickPosition` enum, `screen_codes!()` macro, and
   `volatile-register` patterns.

### Phase 2: Polish and Production Features (Weeks 3-8)

The scope of Phase 2 has expanded based on the
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md),
which identified raster interrupts as a foundational prerequisite for SID
timing, atmospheric effects, and per-zone charset switching. The raster
interrupt chain is now the first step, unlocking all subsequent work.

6. **Raster interrupt chain** — Set up VIC raster IRQ replacing the Kernal's
   default IRQ handler. Implement a 3-zone color split (game area / status bar
   / messages) with different `$D021` background colors per zone. Add a SID
   player callback at a fixed raster line for frame-rate-independent sound
   playback. Add per-zone charset pointer switching (`$D018`) for the three
   charsets (§6.9). **This step unlocks steps 8-11.** See the demo techniques
   analysis §1 for implementation details. Cost: ~200 bytes of code, ~500
   cycles/frame.

7. **Dirty-rectangle rendering** — Add previous-frame buffer, compare+update
   only changed cells. Reduces per-turn screen writes from 1000 to ~20.
   Orthogonal to raster effects — dirty-rect optimizes screen RAM writes,
   raster effects modify VIC-II registers.

8. **Custom character set** — Design three charsets in CharPad: dungeon
   tileset (rows 0-20), UI font (row 21), message text (rows 22-24). Load
   into VIC Bank 1 ($4800/$5000/$5800) with screen memory at $4400. Per-zone
   switching handled by the raster chain (step 6). Add animated charset tiles
   for water, torches, and stairs (~8 bytes/frame, ~40 cycles). See §6.9.

9. **SID sound effects** — Implement combat sounds, footsteps, death jingle
   using `mos-hardware`'s SID module. The raster interrupt chain (step 6)
   handles SID playback timing — the play routine runs once per frame at a
   fixed raster line, independent of the turn-based game loop. See §6.11.

10. **Top/bottom border removal** — Open the vertical borders by toggling
    RSEL at the exact raster lines where the VIC-II checks the border
    flip-flop. Two register writes per frame (~20 cycles). The border renders
    as black — cleaner than the default C64 frame and signals a polished
    production. See the demo techniques analysis §2.

11. **Atmospheric color gradient** — Torchlight vertical gradient centered on
    the player's Y position via per-line `$D021` changes (~110 cycles/frame).
    Damage flash (border to red for 2-3 frames). Low-HP border pulse. See
    §6.8 and the demo techniques analysis §5.

12. **Save/Load** — Binary serialization to 1541 floppy via Kernal file I/O.
    C FFI wrappers for `SAVE`/`LOAD` Kernal calls. Follow the `SaveBackend`
    trait pattern from `roguelike-saves` — the C64 won't implement the full
    trait (it uses `GameState` which requires `serde`), but the API shape
    (autosave, slots, metadata) guides the design.

13. **Title screen** — Seed entry (keyboard hex input), "New Game" / "Continue"
    menu, PETSCII art enhanced with raster color bars and animated charset
    characters. Challenge mode seed display.

14. **PC challenge mode UI** — Add 40x21 "C64 Challenge" mode to the terminal
    client. Accept u16 seed codes, generate via shared mapgen, display on
    cross-platform leaderboard.

### Phase 3: Networking — UII+ Features (Weeks 9-12)

15. **UII+ driver layer** — Hardware detection, TCP primitives.
16. **Cloud saves** — HTTP PUT/GET to server endpoint.
17. **Leaderboard + daily seed** — POST scores, GET daily seed (u16).
    Cross-platform leaderboard for challenge mode.
18. **Spectation relay** — Binary frame streaming.
19. **MCP client mode** — Observation formatting, action parsing.
20. **AT Protocol bridge** — Per the design in
    [c64-atproto-bridge.md](design/c64-atproto-bridge.md).

### Phase 4: Testing and Release (Weeks 13-14)

21. **Playtesting** — Real hardware (Ultimate 64, C64 + UII+, stock C64) and
    emulators (VICE, c64.emu). PAL vs NTSC timing.
22. **Performance profiling** — VICE cycle counter for FOV + AI + render +
    raster chain overhead. Verify continuous raster costs match estimates
    (~660 cycles/frame — see §11.4).
23. **Cross-platform seed verification** — Generate challenge dungeons on both
    platforms with the same seeds, verify identical tile layouts.
24. **Packaging** — .d64 disk image, .prg for emulators, .crt if code fits
    16 KB cartridge. Publish atproto bridge Docker image.

---

## 9. What Gets Cut / What Gets Added

### What Gets Cut

| Feature | Rust Version | C64 Version | Reason |
|---------|-------------|-------------|--------|
| Map size | 80x40 (configurable) | 40x21 (fixed) | Screen size + UI (§6.1) |
| FOV radius | 8 tiles | 6 tiles | CPU budget |
| FOV algorithm | Recursive shadowcasting | Bresenham raycasting | No FP, no recursion |
| Max rooms | 30 | 12 | Map size |
| Max monsters | ~20-30 | 15 | Memory + CPU |
| Save format | JSON (serde) | Binary (compact) | Disk space/speed |
| Color palettes | 4 (accessibility) | 1 (fixed) | 16-color limit |
| Auto-explore | A* pathfinding | Simplified or cut | Memory |
| Message history | Scrollable | Last 3 messages | Screen space (§6.1) |

### What Gets Added

| Feature | C64 Only |
|---------|----------|
| Raster interrupt chain | 3-zone color split, SID timing, charset switching (§6.8, §6.9, §6.11) |
| Atmospheric lighting | Torchlight gradient, damage flash, low-HP pulse via raster effects (§6.8) |
| Border removal | Open top/bottom borders for cleaner visual frame (§8, step 10) |
| Sound effects | SID chip combat sounds, ambience, death jingle — raster-timed (§6.11) |
| Custom charsets | 3 per-zone charsets: dungeon tiles, UI font, message text (§6.9) |
| Animated tiles | Water, torches via charset cycling (~40 cycles/frame) (§6.9) |
| Joystick control | Full 8-direction joystick with fire button |
| Title screen art | PETSCII art splash screen with raster color bars |
| Online leaderboards | Cross-platform scores (C64 + PC) via UII+ |
| Daily challenge | Shared daily seed fetched from server |
| Cloud saves | Save/load game state over HTTP via UII+ |
| AT Protocol saves | Federated saves to PDS via bridge |
| Network spectation | Stream gameplay to SSH spectation relay |
| LLM auto-play | MCP client mode — watch an AI play on your C64 |

### What's Shared (New)

| Feature | How |
|---------|-----|
| **Cross-platform seeds** | Same LFSR + same mapgen = identical dungeon on PC and C64 |
| **Balance constants** | Single source of truth in `roguelike-rules::balance` |
| **Map generation** | `roguelike-rules::mapgen::generate()` — shared algorithm |
| **Combat formula** | `roguelike-rules::combat::damage()` — shared `const fn` |
| **Monster spawning** | `roguelike-rules::spawn` — shared weighted selection |
| **Room geometry** | `roguelike-rules::room::Room` — shared struct + intersection |
| **Seed codes** | `roguelike-rules::seed` — encode/decode for sharing |
| **Item definitions** | `roguelike-rules::items` — type IDs, stat lookup tables |
| **Leveling tables** | `roguelike-rules::leveling` — XP thresholds, stat growth |
| **Depth scaling** | `roguelike-rules::balance` — monster scaling per floor |
| **Mood thresholds** | `roguelike-rules::balance` — flee/enrage trigger values |

---

## 10. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| rust-mos single maintainer abandons project | Medium | High | llvm-mos (LLVM backend) has separate active team; Mikael Lund maintains parallel fork; Docker images are self-contained; code can be ported to cc65 C if needed |
| Code generation produces slow hot paths | Medium | Medium | Profile on VICE; rewrite hot paths as C FFI calls with inline 6502 assembly; raw pointer arithmetic >> slice access |
| Code size exceeds budget | Low | Medium | POC measured 13 KB; LTO + `opt-level = "s"` + feature-gated mos-hardware keep size down; shared crate reduces total C64 code |
| No inline assembly for Kernal calls | Certain | Low | C FFI wrappers (proven by chirp8-c64); mos-hardware's `cbm_kernal` module; function pointer stubs for simple instructions |
| PRNG edge cases (POC bug: rejection sampling overflow) | Resolved | N/A | Fixed in POC — power-of-2 spans handled with early return; shared LfsrRng carries the fix |
| Keyboard input on emulators (POC bug: CIA Port B conflict) | Resolved | N/A | Fixed in POC — use Kernal buffer + Port A joystick only |
| Shared crate adds complexity | Low | Low | Shared crate is ~550 lines of concrete `no_std` functions — no traits, no generics. Includes gameplay features (items, leveling, mood). Both platforms can vendor a local copy as fallback |
| FOV too slow on real hardware | Low | High | Already budgeted at ~7,500 cycles; validated in POC |
| Custom charset looks bad | Medium | Low | Iterate with CharPad; study Sword of Fargoal, chirp8-c64's 2x2 tile approach |
| UII+ command interface underdocumented | Medium | Medium | UII+ firmware is open source; test on real hardware early |
| Network timeout blocks game loop | Medium | High | Non-blocking polls with timeout; game remains playable offline |
| Cross-platform seed divergence | Low | Medium | Golden snapshot tests in CI; shared mapgen is deterministic; LFSR period test catches PRNG regressions |

---

## 11. Detailed Technical Notes

### 11.1 PRNG: Lessons from the POC

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

### 11.2 Input: CIA Port Multiplexing

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

### 11.3 Static Stack Allocation

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
  compiler reason about what memory is touched by each function call. See §11.5.

### 11.4 Turn Timing

Total turn processing measured on the POC (full redraw, no dirty-rect):
~20,000 cycles = ~20 ms. With dirty-rectangle rendering (~500 cycles) and
amortized AI costs, this drops to ~8,000 cycles = ~8 ms. Well under one
frame (16.7 ms NTSC / 20 ms PAL).

**Continuous per-frame raster overhead:** In addition to per-turn costs, the
raster interrupt chain (§8, Phase 2 step 6) introduces a continuous background
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

### 11.5 C64 Code Style: Which Abstractions Help on the 6502

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

---

## 12. Open Questions

1. **Map size:** Fixed 40x21 (no scrolling) vs. 64x48 with scrolling viewport?
   Scrolling is more work but enables richer dungeons. The 40x21 baseline uses
   a single dense status row plus 3 message lines, with modal inventory (§6.1).

2. ~~**Multiple dungeon levels?**~~ **Resolved.** The
   [gameplay implementation plan](design/gameplay-implementation-plan.md)
   Phase 2 designs stairs and multi-level dungeons. On the C64, swap map data
   on descend (840 bytes per floor); optionally hold 2 floors in memory
   (1,680 bytes) for quick backtracking. Derive floor seeds from the base
   seed: `LfsrRng::new(base_seed.wrapping_add(depth))`. MultiRogueLike
   validates this approach — it uses a 3D grid with stairs connecting levels.

3. **mos-hardware version pinning:** Should we vendor `mos-hardware` or depend
   on crates.io? Vendoring avoids breakage from upstream changes; crates.io
   gets updates automatically.

4. **PAL vs. NTSC timing:** Turn-based so gameplay unaffected, but animation
   timing and SID tuning differ. Detect at startup with raster counter check.

5. **alloc vs. no-alloc:** The POC runs without `alloc`. Should we stay
   no-alloc (simpler, predictable memory) or adopt `mos-alloc` for convenience
   (Vec, String)? Recommendation: stay no-alloc.

6. **How much C FFI?** Currently zero in the POC. Kernal calls (disk I/O) will
   need C wrappers. mos-hardware's `cbm_kernal` module may handle this, but
   we should audit whether its C build dependency (`cc` crate) works in the
   Docker workflow.

7. **mos-hardware code size:** The proposal recommends migrating from hand-
   rolled `poke`/`peek` to `mos-hardware` for type safety. Budget ~500-1500
   bytes for mos-hardware's contribution to the binary (volatile-register
   wrappers, bitflags structs). Measure after migration and compare against
   the POC's 13 KB baseline.

8. **Player sprite:** Should the player character use a hardware sprite
   overlaid on the character-mode dungeon map? A 24x21 sprite enables smooth
   inter-tile movement (4-frame glide instead of instant snap), idle animation
   (breathing, torch flicker), and a dramatic visual distinction between
   the player and the environment — the same approach used by *Sword of
   Fargoal* (1982). Cost: ~126 cycles/frame for sprite DMA + ~1 KB for
   animation data (63 bytes/frame x 4 frames x 4 entities if monsters also
   get sprites). The sprite must reside in the active VIC bank (Bank 1, see
   §6.9). **Phase 2 or v1.1?** Adding the player sprite alongside the custom
   charset (Phase 2 step 8) is a natural pairing — the charset handles the
   dungeon, the sprite handles the player. Monster sprites could follow in
   v1.1. See the [C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md)
   §6 for the full design.

9. **Level transition animation:** When the player descends stairs (gameplay
   plan Phase 2), should there be a visual transition effect? An FLD wipe
   pushes the current floor off the bottom of the screen over 4-8 frames,
   then reveals the new floor — much more dramatic than a simple clear-and-
   redraw. Cost: ~3,150 cycles total per transition (~10 frames × ~315
   cycles/frame for 21 crunched lines). Two approaches: **FLD** (simpler —
   pushes the whole display as one block, like a curtain) or **linecrunch**
   (more flexible — selectively removes rows, like a dissolve). FLD achieves
   80% of the visual impact with less code. See the demo techniques analysis
   §3 and §12.

---

## 13. Conclusion

The rust-mos approach is validated. The POC proves that Rust compiles to viable
6502 machine code for a complete roguelike game loop in 13 KB — competitive
with the original cc65 estimate of 12-18 KB.

The main advantages over the cc65 approach:

1. **Single language** — The entire project (7 workspace crates + C64) stays
   in Rust. The C64 is another frontend in the existing multi-frontend
   architecture (terminal, SSH, MCP, C64).
2. **Shared algorithms** — Map generation, combat, spawning, room geometry,
   item definitions, leveling tables, depth scaling, mood thresholds, and the
   PRNG live in `roguelike-rules`, a ~550-line `#![no_std]` crate with zero
   dependencies and zero generics. The shared code is written at the C64's
   abstraction level (`u8`, `&mut [u8]`, `&mut [Room]`, `&mut LfsrRng`) — the
   C64 uses it directly, and the PC calls down to it for challenge mode. This
   ensures gameplay feature parity across platforms as the
   [gameplay implementation plan](design/gameplay-implementation-plan.md)
   features are implemented.
3. **Cross-platform seed sharing** — The shared Galois LFSR and shared map
   generation function mean the same seed produces the same dungeon on both
   platforms. A C64 player's 4-character seed code works in the PC's challenge
   mode, enabling cross-platform daily challenges and leaderboards.
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

The implementation plan spans **14 weeks** (vs. the original 20 weeks for
cc65), expanded from the initial 12-week estimate to incorporate demo scene
techniques (raster interrupts, atmospheric effects, per-zone charsets, border
removal) identified in the
[C64 demo techniques analysis](design/c64-demo-techniques-for-roguelike.md).
The additional 2 weeks are well spent — the raster interrupt chain alone
unlocks proper SID timing, atmospheric lighting, and per-zone charset
switching, all for under 700 cycles/frame of continuous overhead.

Timeline compression factors:
- Phase 0 (POC) is already complete
- Rust development is faster than C/assembly for game logic
- mos-hardware eliminates boilerplate hardware register code
- The shared `roguelike-rules` crate reduces duplication and prevents drift

The C64 roguelike wouldn't just be a downport — it would be the most
interesting client in the fleet. And with cross-platform seed sharing, a C64
player and a PC player can compete on the same dungeon.

Estimated remaining effort: **12-14 weeks** for production features +
networking, **8-10 weeks** for the core game + sound + raster effects without
networking.
