# Commodore 64 Port Proposal

**Project:** Roguelike Dungeon Crawler — C64 Edition
**Date:** 2026-02-20
**Status:** Proposal / Feasibility Study

---

## 1. Executive Summary

This document proposes a native Commodore 64 port of the roguelike dungeon
crawler currently implemented in Rust. The C64 port would be a ground-up
reimplementation of the core game mechanics in 6502 assembly (with a cc65 C
fallback for prototyping), targeting the C64's 64 KB memory, 1 MHz MOS 6510
CPU, and 40x25 PETSCII text display.

The goal is a faithful adaptation — not a 1:1 clone. The C64 version preserves
the dungeon-crawling experience (procedural rooms, fog of war, three monster
types, HP regeneration, and tactical corridor combat) while making principled
trade-offs for the platform's constraints.

**Enhanced hardware target:** The proposal assumes an **Ultimate 64** (or a
stock C64 with an **Ultimate-II+ cartridge**) as the recommended platform,
which provides 10/100 Mbit Ethernet via a built-in network interface. This
unlocks online features — leaderboards, seed sharing, cloud saves, network
spectation, and even LLM integration via the existing MCP server — that would
be impossible on a stock C64. The core game runs on any C64; network features
gracefully degrade when no UII+ is present.

---

## 2. Platform Constraints vs. Current Design

| Resource          | Current (Rust/PC)             | Commodore 64                     |
|-------------------|-------------------------------|----------------------------------|
| CPU               | Multi-GHz, 64-bit             | MOS 6510 @ 1.023 MHz, 8-bit     |
| RAM               | Gigabytes                     | 64 KB total (~38 KB usable)      |
| Screen            | 80x40+ terminal chars         | 40x25 characters (1000 bytes)    |
| Colors            | 24-bit RGB                    | 16 fixed colors                  |
| Character set     | Full Unicode / ASCII          | PETSCII (shifted/unshifted)      |
| Storage           | SSD / RAM disk                | 1541 floppy: 170 KB (~35 sec load) |
| Networking        | TCP/IP (SSH, MCP servers)     | UII+ Ethernet: 10/100 Mbit TCP/UDP |
| Integer types     | i32 everywhere                | 8-bit native, 16-bit emulated    |
| Floating point    | f64 (used in FOV slopes)      | Software FP: ~1000x slower       |
| Data structures   | Vec, HashSet, String, HashMap | Static arrays, bitfields          |
| Coordinate space  | `i32` (Coord)                 | `u8` (0-255) or `i8` (-128..127) |

### 2.1 The Memory Budget

The C64 has 64 KB of address space, but the Kernal ROM, BASIC ROM, I/O
registers, screen memory, and the zero page consume significant portions:

```
$0000-$00FF   Zero Page (256 bytes) — fastest storage, critical registers
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
If we also bank out the Kernal (using our own IRQ handler), we can reach ~50 KB,
but lose Kernal routines for disk I/O, keyboard scanning, etc. The sweet spot
is **banking out BASIC only: ~46 KB usable**.

### 2.2 Memory Budget Allocation

```
Program code:           ~12 KB   (6502 assembly, tightly written)
Map tile data:            1.2 KB  (40 x 24 = 960 tiles, 1 byte/tile + flags)
Explored bitfield:        120 B   (960 bits = 120 bytes)
Visible bitfield:         120 B   (960 bits)
Entity table:             256 B   (16 entities x 16 bytes each)
Message log:              512 B   (4 lines x 40 chars x ~3 messages)
RNG state:                  4 B   (16-bit LFSR or xorshift)
Dungeon generation temp:   2 KB   (room list, corridor scratch)
FOV scratch tables:       512 B   (precomputed slope tables)
Screen buffer:            1 KB    (40 x 25 = 1000 bytes)
Color RAM mirror:         1 KB    (40 x 25 = 1000 nybbles, packed)
Custom charset:           2 KB    (256 chars x 8 bytes)
Save buffer:              2 KB    (serialized game state for disk)
Sound/music data:         2 KB    (SID chip patterns)
Stack headroom:          256 B    (hardware stack)
─────────────────────────────────
Total:                  ~24 KB    (~22 KB headroom remaining)
```

This leaves comfortable room for expansion (more monster types, items, deeper
dungeons, larger maps via scrolling viewport, and network features).

Note: The Ultimate-II+ command interface and network buffers live in the UII+
module's own ARM processor and RAM — they do **not** consume C64 address space.
The 6502 communicates with the UII+ through a small I/O register window
(typically 2-4 bytes at a configurable address). We only need ~256 bytes of
C64 RAM for a network send/receive buffer, already accounted for in the
headroom above.

---

## 3. Design Decisions

### 3.1 Map Size and Viewport

**Current:** 80x40 map, fully visible in terminal (no scrolling).

**C64 approach — scrolling viewport over a larger map:**

The C64 screen is 40x25. Reserving 1 row for the status bar and 2 rows for
the message log leaves a **40x22 visible play area**. However, the actual map
can be larger (e.g., 64x48) stored in RAM, with the viewport scrolling to
follow the player.

```
┌────────────────────────────────────────┐
│              40x22 viewport            │  <- Map area
│         (scrolls to follow @)          │
│                                        │
│                                        │
├────────────────────────────────────────┤
│ HP [████████░░░░] 24/30   Kills: 3     │  <- Status bar (row 23)
├────────────────────────────────────────┤
│ You attack the Goblin for 5 damage.    │  <- Message log
│ The Goblin is dead!                    │  <- (rows 24-25)
└────────────────────────────────────────┘
```

**Map storage at 64x48:**
- Tile data: 64 x 48 = 3072 bytes (1 byte/tile)
- Explored bits: 384 bytes
- Visible bits: 384 bytes
- Total: ~3.8 KB — well within budget.

**Alternative — smaller maps:**
We could also use 40x22 maps (no scrolling) for simplicity in a v1, matching
the viewport exactly. This would use only 880 bytes for tiles.

**Recommendation:** Start with fixed 40x22 maps (no scroll) for the initial
release, then add scrolling viewport in v1.1.

### 3.2 Map Generation

**Current:** Random room placement with corridor carving (`map.rs:222-264`).
Uses `rand::Rng`, `gen_range()`, room intersection checks, and L-shaped
tunnels.

**C64 approach:**

The same algorithm works — it's fundamentally just:
1. Pick random x, y, w, h
2. Check for overlap with existing rooms
3. Carve rectangles and corridors

The only changes needed:
- Replace `rand::Rng` with a 16-bit LFSR (linear feedback shift register) or
  a Galois LFSR for fast pseudo-random numbers on the 6510
- Room list: static array of 16 rooms max (16 x 4 bytes = 64 bytes), reduced
  from the current `max_rooms = 30`
- Room sizes: 3-7 tiles (down from 4-10) to fit the smaller map
- All coordinates are `u8` instead of `i32`

```asm
; 16-bit Galois LFSR — fast PRNG for 6502
; Input: seed in rng_lo/rng_hi
; Output: pseudo-random byte in A
prng:
    lda rng_lo
    asl
    rol rng_hi
    bcc .no_eor
    eor #$2D        ; taps: bits 0,2,3,5
.no_eor:
    sta rng_lo
    rts
```

### 3.3 Field of View

**Current:** Recursive shadowcasting (`fov.rs`). Uses `f64` slope values,
`HashSet<(i32,i32)>` for visible tiles, and recursive function calls per
octant.

This is the single hardest subsystem to port. Shadowcasting on a 1 MHz
8-bit CPU with a 256-byte stack is impractical — the recursion depth and
floating-point slopes are both showstoppers.

**C64 approach — raycasting with Bresenham lines:**

Cast rays from the player to each tile on the perimeter of the FOV circle
(radius 6, reduced from 8). Use Bresenham's line algorithm (integer-only) to
walk each ray, marking tiles as visible until hitting a wall.

```
FOV radius 6, perimeter ≈ 38 tiles
Rays cast: ~38
Avg ray length: ~4 tiles
Total tile checks: ~150 per FOV recompute
```

At 1 MHz with ~50 cycles per tile check, this takes roughly 7,500 cycles
(~7.5 ms) — well under a single frame (16.7 ms at 60 Hz NTSC).

**Visibility storage:** A 120-byte bitfield (for 40x22 = 880 tiles, rounded
up to 960) replaces the `HashSet`. Checking visibility is a single bit test:

```asm
; Check if tile (x, y) is visible
; Input: X = tile_x, Y = tile_y
; Output: Z flag set if visible
check_visible:
    ; bit_index = y * 40 + x
    ; byte_offset = bit_index >> 3
    ; bit_mask = 1 << (bit_index & 7)
    ...
```

**Trade-off:** Bresenham raycasting produces slightly different visibility
results than shadowcasting (some corner cases differ), but for a C64 roguelike
this is entirely acceptable. The original Rogue (1980) used simpler LOS.

### 3.4 Monster AI

**Current:** Per-monster `can_see()` check using shadowcasting, then greedy
chase toward player (`ai.rs`).

**C64 approach:**

Monster sight checks are expensive if each monster runs full raycasting. Two
options:

1. **Simplified LOS:** Cast a single Bresenham line from monster to player.
   If no wall blocks it and distance <= sight_radius, the monster is aware.
   Cost: ~200 cycles per monster, ~3,200 cycles for 16 monsters.

2. **Distance-only awareness:** Skip LOS entirely. If Chebyshev distance
   <= sight_radius and monster is in an explored room, it's aware. Much faster
   but less tactical (monsters "see" through walls within range).

**Recommendation:** Option 1 (single-ray LOS). It's fast enough and preserves
the tactical depth of the Rust version where walls block monster awareness.

Chase AI is trivial to port — it's just `signum(player - monster)` with
walkability checks. Three candidate moves, pick the first valid one. Already
integer-only in the Rust version (`ai.rs:70-85`).

### 3.5 Combat System

**Current:** `damage = max(0, attacker_atk - defender_def)` (`combat.rs`).

This ports directly with no changes. All values fit in `u8`. The formula is a
single subtraction and a branch:

```asm
; damage = max(0, atk - def)
    lda attacker_atk
    sec
    sbc defender_def
    bcs .positive
    lda #0              ; clamp to 0
.positive:
    sta damage
```

HP regeneration (1 HP every 3 turns) is equally trivial — a turn counter
modulo 3.

### 3.6 Entity System

**Current:** `Vec<Entity>` with 12 fields per entity, dynamically allocated
strings for names, `char` glyphs, `GameColor` enum.

**C64 approach — fixed-size entity table:**

```
Entity structure (16 bytes each):
  Byte 0:    x position (u8)
  Byte 1:    y position (u8)
  Byte 2:    glyph (PETSCII code)
  Byte 3:    color (0-15, C64 color index)
  Byte 4:    entity type (0=player, 1=goblin, 2=orc, 3=troll)
  Byte 5:    flags (bit 0: alive, bit 1: visible_to_player, bit 2-3: AI type)
  Byte 6:    hp (u8)
  Byte 7:    max_hp (u8)
  Byte 8:    attack (u8)
  Byte 9:    defense (u8)
  Byte 10:   sight_radius (u8)
  Bytes 11-15: reserved (future: status effects, inventory slot, etc.)
```

**16 entity slots = 256 bytes.** Entity names are not stored per-entity;
instead, the `entity_type` byte indexes into a ROM string table. The current
game typically spawns ~15-20 monsters on a 80x40 map; on a 40x22 map with
fewer rooms, 15 monster slots (+ 1 player) is sufficient.

### 3.7 Rendering

**Current:** Crossterm terminal rendering (`tui/render.rs`). Iterates every
tile, checks visibility/explored, writes characters with fg/bg colors.

**C64 approach:**

The C64 has two relevant display modes:

1. **Standard character mode (40x25):** Each cell is one character from a
   256-char set, with per-cell foreground color (from Color RAM at $D800)
   and a shared background color.

2. **Custom character set:** We define our own 2 KB charset with better-looking
   dungeon tiles (solid walls, dotted floors, monster glyphs, UI elements).

**Color mapping:**

| GameColor  | C64 Color         | Index |
|------------|-------------------|-------|
| Black      | Black             | 0     |
| White      | White             | 1     |
| Grey       | Light Grey        | 15    |
| DarkGrey   | Dark Grey         | 11    |
| Red        | Red               | 2     |
| DarkRed    | Brown             | 9     |
| Green      | Green             | 5     |
| DarkGreen  | Dark Green (cust) | 5*    |
| Yellow     | Yellow            | 7     |
| DarkBlue   | Blue              | 6     |
| Cyan       | Cyan              | 3     |

*DarkGreen maps to Green since the C64 has no dark green. Alternatively, use a
multicolor character mode trick to get more shades at the cost of horizontal
resolution.

**Rendering strategy:**

Instead of redrawing the entire screen every frame (expensive at 1 MHz),
use **dirty-rectangle tracking**:

1. Maintain a "previous frame" buffer (1 KB)
2. After each game step, compare new state to previous buffer
3. Only write changed cells to screen memory ($0400) and color RAM ($D800)

Typical turn: player moves 1 tile, 2-3 FOV tiles change, 1-3 monsters move.
That's ~10-20 cell updates instead of 1000. At ~20 cycles per cell write,
this takes ~400 cycles — negligible.

**Status bar:** HP bar uses PETSCII block characters (█ = $E0, ░ = $65
in the C64 charset). The `render_status_bar` logic translates directly.

### 3.8 Custom Character Set

Design a 2 KB custom charset (256 characters x 8 bytes) for better visuals:

```
Char $00: empty/black (unexplored)
Char $01: floor '.' (single dot, centered)
Char $02: wall '#' (solid block with edge detail)
Char $03: player '@' (stylized, recognizable)
Char $04: goblin 'g' (hunched figure)
Char $05: orc 'o' (broad figure)
Char $06: troll 'T' (tall figure)
Char $07: corpse '%' (X marks)
Char $08-$0F: HP bar segments (empty to full, 8 gradations)
Char $10-$1F: box-drawing characters for menus
Char $20-$5A: standard PETSCII uppercase letters for messages
Char $5B-$7F: additional UI glyphs, arrows, borders
```

This gives us crisp, purpose-built visuals while staying in fast character
mode (no bitmap rendering overhead).

### 3.9 Input Handling

**Current:** Keyboard (vi keys, arrow keys, numpad) and gamepad via crossterm
events (`terminal/input.rs`, `tui/input.rs`).

**C64 approach:**

- **Keyboard:** Scan the C64 keyboard matrix via the CIA chip ($DC00/$DC01)
  or use the Kernal `GETIN` routine ($FFE4). Map keys to game commands.
- **Joystick:** Read CIA port 2 ($DC00) for joystick input — up/down/left/
  right/fire. The fire button doubles as "wait" or "confirm." Diagonal
  movement via simultaneous directions.
- **No vi keys:** The C64 keyboard layout doesn't lend itself to hjkl. Use
  WASD, arrow keys, or joystick as primary input methods.

```
Joystick mapping:
  Up          → Move North
  Down        → Move South
  Left        → Move West
  Right       → Move East
  Up+Right    → Move NE
  Up+Left     → Move NW
  Down+Right  → Move SE
  Down+Left   → Move SW
  Fire        → Wait (or context action)
  Fire+Dir    → Autorun in direction
```

### 3.10 Save System

**Current:** JSON serialization via serde, autosave to filesystem, manual
save slots.

**C64 approach:**

Serialize game state to a compact binary format. Total save size estimate:

```
Map tiles:          960 bytes (40x24, 1 byte/tile)
Explored bitfield:  120 bytes
Entity table:       256 bytes (16 x 16)
Game state header:   16 bytes (turn count, seed, HP, flags)
Message log:        160 bytes (4 x 40)
Room list:           64 bytes (16 rooms x 4 bytes)
Checksum:             2 bytes
─────────────────────────────
Total:            ~1,578 bytes (< 7 disk sectors)
```

**Four save backends** (selected at startup based on hardware detection):

1. **1541 Floppy (stock C64):** Sequential file on disk. Writes at ~400
   bytes/sec, so saving takes ~4 seconds. One save slot. Acceptable for a
   roguelike where saves are infrequent.

2. **SD2IEC / Pi1541:** Same file format, but writes at modern SD card speed
   — effectively instant. Multiple save slots become practical.

3. **UII+ Network Save (Ultimate 64):** POST the binary save data to a
   configurable HTTP endpoint (e.g., a simple REST server on the LAN or
   internet). Save/load becomes a single TCP transaction — ~50 ms round-trip
   on a local network. This also enables **cloud saves**: play on your C64 at
   home, load the same game on VICE at work.

   ```
   PUT /api/saves/{player_id}    → upload 1,578 bytes
   GET /api/saves/{player_id}    → download save
   DELETE /api/saves/{player_id} → permadeath wipe
   ```

   The server side is trivial — a 50-line endpoint that could be added to
   the existing Rust roguelike binary (alongside the SSH and MCP servers).

4. **AT Protocol via Bridge (Ultimate 64 + atproto bridge):** Save game
   state to the player's **Personal Data Server (PDS)** on the AT Protocol
   network (Bluesky ecosystem) via a self-hosted bridge server. The C64
   sends compact binary save packets over raw TCP to a bridge on the LAN;
   the bridge translates them into atproto `putRecord`/`uploadBlob` XRPC
   calls against the player's PDS.

   This is fully designed in [docs/design/c64-atproto-bridge.md](design/c64-atproto-bridge.md).

   ```
   C64 ──binary TCP:6510──► Bridge ──HTTPS/XRPC──► PDS (bsky.social)
                             (Docker)                (user's data)
   ```

   **Why this matters:** Saves stored on a PDS are portable across all
   platforms. A game saved from a C64 can be loaded on the SSH client or a
   future web frontend — they all use the same atproto lexicons
   (`save.gameState`, `save.settings`). The PDS records produced by the
   bridge are **schema-identical** to those from any other client. Spectate
   frames published by the bridge are equally indistinguishable — a Jetstream
   subscriber cannot tell whether a `spectate.frame` was produced by an SSH
   session or a Commodore 64.

   The bridge also handles authentication (app passwords or OAuth via a web
   UI on port 8080), offline caching (saves persist locally if the PDS is
   unreachable), and PETSCII→ASCII conversion for spectate frames.

   Save slot mapping: `0x00` → `autosave` rkey, `0x01-0x05` → `slot-1`
   through `slot-5` rkeys. Up to 6 save slots via the bridge.

**Save format:** Binary, identical across all four backends. The first 9
bytes are a fixed metadata header (turn count, HP, max HP, explored%, seed)
that the atproto bridge reads to populate record fields; the rest is an
opaque blob. One slot for stock C64 floppy, multiple for SD2IEC/network/
atproto. Delete-on-death for classic roguelike mode.

### 3.11 Sound Design

**Current:** No sound (terminal-based).

The C64 has the legendary **SID chip** (MOS 6581/8580) — three voices with
multiple waveforms, filters, ADSR envelopes, and ring modulation. This is an
opportunity to **enhance** the port beyond the original.

Proposed sound effects:
- **Footsteps:** Soft noise-channel tick on each move
- **Attack hit:** Short pulse-wave stab (pitch varies by damage)
- **Attack miss:** Low thud
- **Monster death:** Descending pitch sweep
- **Player hurt:** Dissonant chord + noise burst
- **Player death:** Dramatic descending arpeggio
- **Level ambience:** Low droning pad (triangle wave + filter sweep)
- **Door/room discovery:** Rising arpeggio flourish

This is approximately 1-2 KB of sound data and a ~500-byte SID player routine.

### 3.12 Networking (Ultimate 64 / UII+)

**Current Rust version networking:**
- SSH server (`crates/ssh/`) — multiplayer spectation, remote play
- MCP server (`crates/mcp/`) — LLM integration for AI-driven gameplay

**C64 approach with UII+ Ethernet:**

The Ultimate-II+ module contains an ARM processor running its own firmware
with a full TCP/IP stack. The 6502 communicates with it through a command
interface — essentially writing command bytes and reading response bytes
through an I/O register window. The UII+ handles DNS resolution, TCP
connection management, HTTP framing, and buffering. From the 6502's
perspective, networking is just "write bytes to a port, read bytes from a
port."

This enables four network features, in order of implementation priority:

#### 3.12.1 Online Leaderboards

After a game ends (player death), submit the run stats to a leaderboard
server:

```
POST /api/leaderboard
{
  "seed": "a3f2",
  "kills": 12,
  "turns": 347,
  "explored_pct": 83,
  "player_name": "DAVID",
  "platform": "c64-u2p"
}
```

The response includes the player's rank and top 10. Display on the death
screen — "You placed #7 of 243 adventurers." The same leaderboard endpoint
could accept submissions from the Rust PC/SSH version, creating a **cross-
platform leaderboard** between C64 and modern players.

Payload size: ~150 bytes out, ~500 bytes response. At UII+ speeds this
completes in under 100 ms.

#### 3.12.2 Daily Challenge Seeds

Fetch a daily seed from the server at the title screen:

```
GET /api/daily-seed
→ {"seed": "b7f1", "date": "2026-02-20", "entries": 42}
```

All players worldwide (C64 and PC) explore the same dungeon for that day.
Combined with the leaderboard, this creates a competitive daily challenge
mode — a feature the Rust version doesn't have yet.

#### 3.12.3 Network Spectation

The existing Rust SSH server (`crates/ssh/`) already supports spectation via
the `FrameSink` trait. The C64 can participate by streaming its game state
after each turn:

- **Option A — C64 as spectated player:** After each turn, send a compact
  binary frame (~200 bytes: entity positions, visible tiles, HP, messages) to
  a relay server. The relay converts this to the format consumed by the SSH
  spectation system. People watch your C64 run from their terminals.

- **Option B — C64 as spectator:** Receive ASCII frames from the relay and
  display them on the C64 screen. Watch someone else's PC game on your C64.
  This is simpler than option A (just receive + display, no game logic).

Frame size at ~200 bytes/turn and ~1 turn/second average, the bandwidth
requirement is negligible (~1.6 kbps).

#### 3.12.4 LLM Integration via MCP

This is the most ambitious network feature. The existing MCP server
(`crates/mcp/`) provides a JSON-RPC interface for LLM-driven gameplay. The
C64 could act as an MCP client:

1. After each turn, serialize the game observation (map ASCII, entity list,
   stats) into a compact text format
2. Send it to the MCP server via HTTP POST
3. Receive the LLM's chosen action as a response
4. Execute the action and repeat

This creates a surreal scenario: **an AI playing a roguelike on a Commodore
64 in real time**, with the LLM running in the cloud and the game running
on a 1 MHz 8-bit machine, connected by Ethernet.

The observation payload would be ~500-800 bytes (the 40x22 ASCII map alone is
880 bytes, but we can compress explored/visible states). The response is
tiny (~20 bytes for an action command). Round-trip latency depends on LLM
inference time (~1-3 seconds), which is fine for a turn-based game.

The MCP server already handles all the game logic translation — the C64 just
needs to format the observation and parse the response action. Estimated
client code: ~1 KB of 6502 assembly.

#### 3.12.5 AT Protocol Integration via Bridge Server

The most fully designed network feature. A self-hosted **bridge server**
(Docker container, runs on a Raspberry Pi or any LAN host) sits between the
C64's raw TCP connection and the AT Protocol ecosystem, providing:

- **Federated saves** — Game state stored on the player's PDS, portable
  across all platforms (C64, SSH, web)
- **Federated spectation** — Spectate frames published as atproto records,
  visible via Jetstream to any subscriber
- **Bluesky identity** — The player's atproto handle (e.g.,
  `player.bsky.social`) is their identity across all platforms

The bridge uses a purpose-built binary wire protocol (little-endian,
1-byte type + 2-byte length + payload) with 10 message types. The C64 never
touches TLS, JSON, OAuth, or HTTP — the bridge handles all of that.

See [docs/design/c64-atproto-bridge.md](design/c64-atproto-bridge.md) for the
complete design including wire protocol specification, Docker deployment,
authentication flow, PETSCII→ASCII conversion, offline caching, spectate
frame publishing, and phased implementation plan.

```
C64 (6502)         Bridge (Docker)         PDS
    │                    │                   │
    │──SAVE_GAME────────►│                   │
    │                    │──uploadBlob──────►│
    │                    │──putRecord───────►│
    │◄──ACK─────────────│                   │
    │                    │                   │
    │──SPECTATE_FRAME──►│                   │
    │                    │──createRecord────►│   ──► Jetstream
    │◄──ACK─────────────│                   │       (anyone can watch)
```

The bridge is a separate project (`tools/c64-bridge/` or standalone repo),
not a workspace crate. It reuses the same atproto lexicons as the SSH and
terminal clients but has its own deployment model.

**Implementation note:** All network features must be **optional and graceful**.
At startup, the game probes for UII+ presence by reading a signature register.
If absent (stock C64, VICE emulator), all network codepaths are skipped and
the game works identically to a stock C64 build. Network menu items are hidden
when no UII+ is detected.

```
; Detect Ultimate-II+ presence
; UII+ command interface at $DF1C-$DF1F (configurable)
detect_uii:
    lda $DF1D           ; read UII+ status register
    cmp #$C9            ; UII+ identification byte
    beq .found
    lda #0
    sta has_network     ; no UII+ — disable network features
    rts
.found:
    lda #1
    sta has_network
    rts
```

### 3.13 Seed System

**Current:** 64-bit seed encoded as base-36 string (`seed_code.rs`), shared
between players.

**C64 approach:** Use a 16-bit seed (0-65535). Encode as 4-digit hex or
3-character base-36 code. Players can enter seeds on the title screen using the
keyboard. 16 bits gives 65,536 unique dungeons — more than enough for a C64
game.

The current `SeedParams` struct with width/height/preset encoding is
unnecessary since the C64 version uses fixed map dimensions.

---

## 4. Implementation Plan

### Phase 1: Core Engine (Weeks 1-4)

**Toolchain:** [cc65](https://cc65.github.io/) (C compiler + assembler for
6502). Prototype in C, then hand-optimize hot paths in assembly.

1. **Memory map and startup** — Bank out BASIC ROM, set up custom charset
   pointer, configure screen/color RAM locations, initialize IRQ handler for
   keyboard/joystick scanning.

2. **PRNG** — Implement 16-bit Galois LFSR, seeded from CIA timer or user
   input (key-timing entropy).

3. **Map generation** — Port `Map::generate()` to C/asm. Room placement with
   collision detection, L-shaped corridor carving. Target: 8-12 rooms per map.
   Room dimensions: 3x3 to 6x6.

4. **Entity system** — 16-slot fixed table, type-indexed stat lookup. Port
   `spawn_monsters()` with weighted random selection.

5. **FOV** — Bresenham raycasting, radius 6. Bitfield storage for
   visible/explored tiles.

### Phase 2: Gameplay (Weeks 5-8)

6. **Combat** — Port `melee_attack()`. Direct translation — subtract, clamp,
   check death. Generate message strings from ROM templates.

7. **AI** — Single-ray LOS awareness check. Greedy chase with 3-candidate
   movement. Port `run_monster_turns()`.

8. **Game loop** — Turn-based step: read input → execute player command →
   recompute FOV → run monster turns → render. Port the core of
   `GameState::step()`.

9. **HP regeneration** — Turn counter modulo `regen_interval` (3).

### Phase 3: UI and Polish (Weeks 9-12)

10. **Rendering** — Custom charset, dirty-rectangle renderer, color RAM
    management. Status bar with HP bar. Message log (2-line scrolling).

11. **Title screen** — Seed entry, "New Game" / "Continue" menu. PETSCII art
    title logo.

12. **Save/Load** — Binary serialization to 1541 floppy. Single save slot,
    delete-on-death for classic roguelike mode.

13. **Sound** — SID chip effects for combat, movement, death, ambience.

14. **Joystick support** — Full 8-direction + fire button mapping.

### Phase 4: Networking — UII+ Features (Weeks 13-16)

15. **UII+ driver layer** — Hardware detection, command interface init,
    TCP connect/send/receive primitives. Stub out when UII+ is absent.

16. **Cloud saves** — HTTP PUT/GET save data to a server endpoint. Add a
    minimal save-relay endpoint to the existing Rust binary.

17. **Leaderboard + daily seed** — HTTP POST scores, GET daily seed. Add
    server endpoints. Display top 10 on death screen and daily challenge
    option on title screen.

18. **Spectation relay** — Binary frame streaming to a relay server. Add a
    relay endpoint to the Rust SSH crate that bridges C64 frames into the
    existing `FrameSink` spectation system.

19. **MCP client mode** — Format observations, POST to MCP server, parse
    action responses. Add "Watch AI Play" option to title menu.

20. **AT Protocol bridge** — Implement the bridge server per the design in
    [docs/design/c64-atproto-bridge.md](design/c64-atproto-bridge.md). Phase 1
    (local cache only) can be built immediately; Phase 2 (PDS integration)
    depends on the atproto lexicons being finalized. The C64-side wire
    protocol client reuses the same UII+ TCP primitives built in step 15.

### Phase 5: Testing and Release (Weeks 17-20)

21. **Playtesting** — Test on real hardware (Ultimate 64, C64 + UII+ cart,
    stock C64, C128) and emulators (VICE). Verify disk save/load reliability,
    network features, atproto bridge end-to-end, timing on PAL vs NTSC.

22. **Performance profiling** — Ensure FOV + AI + render fits within one frame
    on worst case (many monsters in open room). Profile network round-trips.

23. **Packaging** — Create .d64 disk image, .crt cartridge image (for flash
    carts like EasyFlash), and .prg file for emulators. Ship separate
    "network-enabled" and "stock" builds if code size requires it. Publish
    the atproto bridge Docker image to GHCR.

---

## 5. What Gets Cut

Some features from the Rust version don't make sense on the C64:

| Feature              | Rust Version              | C64 Version          | Reason           |
|----------------------|---------------------------|----------------------|------------------|
| Map size             | 80x40 (configurable)      | 40x22 (fixed)        | Screen size      |
| FOV radius           | 8 tiles                   | 6 tiles              | CPU budget       |
| FOV algorithm        | Recursive shadowcasting   | Bresenham raycasting | No FP, tiny stack|
| Max rooms            | 30                        | 12                   | Map size         |
| Max monsters         | ~20-30                    | 15                   | Memory + CPU     |
| Monster sight checks | Per-monster shadowcasting  | Per-monster ray LOS  | CPU budget       |
| Save format          | JSON (serde)              | Binary (compact)     | Disk space/speed |
| Save slots           | Autosave + 3 manual       | 1 slot               | Disk simplicity  |
| Settings menu        | 12+ toggle options        | 3 options (sound, speed, joystick) | Screen space |
| Color palettes       | 4 (default, protanopia, deuteranopia, high-contrast) | 1 (fixed) | 16-color limit |
| SSH multiplayer      | Full SSH server            | UII+ spectation relay | Client-only (no SSH daemon on 6502) |
| MCP server           | LLM integration           | UII+ MCP client       | Client-only (MCP server stays on PC) |
| TOML data files      | Moddable game.toml        | Compiled-in data     | No filesystem    |
| Auto-explore         | A* pathfinding             | Simplified or cut    | Memory           |
| Look mode            | Cursor inspection          | Simplified           | Input limits     |
| Message history      | Scrollable full-screen     | Last 2 messages      | Screen space     |
| Unicode bar chars    | ████░░░░                  | PETSCII blocks       | Charset          |

### What Gets Added

| Feature              | C64 Only                                           |
|----------------------|----------------------------------------------------|
| Sound effects        | SID chip combat sounds, ambience, death jingle      |
| Custom charset       | Purpose-built dungeon tile graphics                 |
| Joystick control     | Full 8-direction joystick with fire button           |
| Title screen art     | PETSCII art splash screen                           |
| Color flash effects  | Screen border flash on hit, death screen effects     |
| Loading screen       | Animated loading indicator during disk I/O           |
| Online leaderboards  | Cross-platform scores (C64 + PC) via UII+ Ethernet  |
| Daily challenge      | Shared daily seed fetched from server                |
| Cloud saves          | Save/load game state over HTTP via UII+              |
| AT Protocol saves    | Federated saves to PDS via bridge; portable across platforms |
| Network spectation   | Stream gameplay to SSH spectation relay via UII+     |
| Federated spectation | Spectate frames published to atproto via bridge      |
| Bluesky identity     | Player identified by atproto handle across all clients |
| LLM auto-play        | MCP client mode — watch an AI play on your C64       |

---

## 6. Architecture Mapping

How the Rust crate structure maps to C64 source files:

```
Rust Crate / Module          →  C64 Source File        Size Est.
─────────────────────────────────────────────────────────────────
core/src/map.rs              →  map.s                  ~2 KB
  Map::generate()                (room placement, corridor carving)
  Map::is_walkable()             (tile lookup, bounds check)

core/src/fov.rs              →  fov.s                  ~1.5 KB
  compute_fov()                  (Bresenham raycasting, bitfield)
  can_see()                      (single-ray LOS for AI)

core/src/entity.rs           →  entity.s               ~0.5 KB
  Entity table                   (16-slot fixed array)
  Type-indexed stat tables       (ROM data)

core/src/combat.rs           →  combat.s               ~0.3 KB
  melee_attack()                 (subtract + clamp + death check)

core/src/ai.rs               →  ai.s                   ~0.8 KB
  is_aware() + chase_ai()       (LOS + greedy chase)
  run_monster_turns()            (iterate entity table)

core/src/spawn.rs            →  spawn.s                ~0.5 KB
  spawn_monsters()               (weighted random per room)

core/src/game.rs             →  game.s                 ~2 KB
  GameState::step()              (main turn logic)
  GameState::new()               (initialization)
  Autorun (simplified)           (repeat-move loop)

core/src/data.rs             →  data.s                 ~0.3 KB
  Static monster/player defs     (ROM tables, not TOML)

core/src/message_log.rs      →  msglog.s               ~0.5 KB
  MessageLog                     (circular buffer, 4 entries)
  Format strings                 (ROM templates)

tui/src/render.rs            →  render.s               ~2 KB
  render_map()                   (dirty-rect screen update)
  render_entities()              (entity glyph + color writes)
  render_status_bar()            (HP bar, kill count)
  render_message_log()           (bottom 2 rows)

tui/src/game_loop.rs         →  main.s                 ~1.5 KB
  run_game_loop()                (title→play→pause state machine)
  Input handling                 (keyboard + joystick scan)

N/A                          →  sid.s                  ~1.5 KB
  Sound effects                  (SID register writes)
  Music player (optional)

N/A                          →  charset.bin            ~2 KB
  Custom character set           (pixel art for all glyphs)

N/A                          →  disk.s                 ~0.8 KB
  Save/Load                      (binary serialization to floppy)

N/A                          →  uii_net.s              ~2 KB
  UII+ detection and init        (probe for hardware, configure)
  TCP/HTTP client                (connect, send, receive wrappers)
  Leaderboard submit/display     (POST score, parse top 10)
  Daily seed fetch               (GET seed, parse response)
  Cloud save/load                (PUT/GET save data)

N/A                          →  spectate.s             ~1 KB
  Frame serialization            (compact binary game state)
  Relay client                   (stream frames to server)

N/A                          →  mcp_client.s           ~1 KB
  Observation formatter          (ASCII map + stats to text)
  Action parser                  (parse server response to command)
  MCP client loop                (send obs, receive action, execute)

core/data/game.toml          →  (compiled into data.s)
  Balance values                 (constants in ROM)

ssh/src/ (spectate relay)    →  (server-side: add endpoint to existing Rust binary)
mcp/src/ (MCP server)        →  (server-side: already exists, no changes needed)

N/A                          →  tools/c64-bridge/      (standalone)
  AT Protocol bridge server      (Docker container, Python or Rust)
  Binary wire protocol handler   (TCP:6510 listener)
  atproto XRPC client            (save + spectate record management)
  PETSCII→ASCII conversion       (static lookup table)
  Web UI                         (config, auth, live spectate viewer)
  See: docs/design/c64-atproto-bridge.md
─────────────────────────────────────────────────────────────────
Total estimated code size:                             ~18 KB
Total with data + charset:                             ~22 KB
  (network modules add ~4 KB; still well within 46 KB budget)
```

---

## 7. Detailed Technical Notes

### 7.1 PRNG Seeding

The Rust version uses `rand::random::<u64>()` for seeding. On the C64, we
seed the 16-bit LFSR from one of:

1. **CIA Timer:** Read the free-running Timer A of CIA #1 ($DC04/$DC05) at
   the moment the player presses a key on the title screen. The low bits are
   effectively random due to human timing jitter.

2. **SID noise:** Read the SID's oscillator 3 output ($D41B) with noise
   waveform enabled. Provides hardware-generated random bytes.

3. **User-entered seed:** Parse a 4-digit hex code from keyboard input.

### 7.2 Integer-Only FOV Slopes

The Rust shadowcasting uses `f64` slopes (`let l_slope = (dx - 0.5) / (dy + 0.5)`).
Bresenham raycasting avoids floating point entirely — it uses only integer
addition and comparison, making it ideal for the 6502.

For each ray from the origin to a perimeter tile:
```
dx = abs(target_x - origin_x)
dy = abs(target_y - origin_y)
error = dx - dy

while not at target:
    if tile is wall: stop, mark remaining tiles non-visible
    mark tile visible
    e2 = 2 * error
    if e2 > -dy: error -= dy, x += step_x
    if e2 <  dx: error += dx, y += step_y
```

Total cost per ray: ~6 instructions per step, ~4 steps average = ~24
instructions = ~50-70 cycles per ray. With ~38 rays, total FOV cost is
~2,000-2,700 cycles — about 2.5 ms. Imperceptible.

### 7.3 Structural Wall Optimization

The Rust version precomputes `structural` walls (walls adjacent to floor) to
skip rendering filler walls (`map.rs:90-114`). On the C64, this optimization
is even more important because we want to minimize screen writes.

During map generation, compute a `structural` bitfield alongside the tile
data. During rendering, skip any wall tile that isn't structural — write a
black/empty character instead. This reduces the number of distinct glyphs on
screen and speeds up dirty-rect updates.

### 7.4 UII+ Network Command Interface

The Ultimate-II+ provides network access through a register-based command
interface. The 6502 writes command bytes to a small I/O window (typically at
$DF1C-$DF1F, configurable), and the UII+ module's ARM processor handles all
TCP/IP stack operations independently.

**Key operations for our use case:**

```
Command flow for an HTTP GET:
  1. Write CMD_NET_TCP_OPEN + destination IP + port 80   → UII+ opens socket
  2. Write CMD_NET_TCP_SEND + "GET /api/daily-seed\r\n"  → UII+ sends HTTP request
  3. Poll CMD_NET_TCP_STATUS until data available          → ARM handles TCP
  4. Read CMD_NET_TCP_RECEIVE into C64 buffer              → Copy response bytes
  5. Write CMD_NET_TCP_CLOSE                               → UII+ closes socket
```

The critical insight: **the ARM processor handles TCP retransmission, window
management, DNS, and buffering**. The 6502 never touches raw packets. From
the 6502's perspective, it's just writing/reading bytes through a port — not
fundamentally different from talking to a 1541 disk drive, which also has its
own 6502 processor handling low-level details.

**Polling vs. blocking:** The UII+ status register can be polled non-
blockingly (check if data is ready, return immediately if not). This lets the
game loop continue rendering "Connecting..." animations or respond to a
cancel keypress during network operations. We never hard-block on network
I/O.

**DNS resolution:** The UII+ firmware handles DNS internally. We can pass
hostnames (e.g., `roguelike.example.com`) directly instead of hardcoding IP
addresses. The server address is configured once via the title screen menu
and stored in the save file or a separate config sector on disk.

### 7.5 Turn Timing and Responsiveness

The C64 runs at 60 Hz (NTSC) or 50 Hz (PAL). The game is turn-based, so we
don't need frame-rate rendering. The loop is:

1. Wait for input (blocking)
2. Process turn (~5,000 cycles max: step + FOV + AI + render)
3. Update screen (dirty rects: ~500 cycles)
4. Return to step 1

Total turn processing: ~5,500 cycles = ~5.5 ms. The screen updates during the
next vertical blank. The game will feel instantaneous.

For autorun animation, insert a ~100 ms delay between steps (6 VBlank frames
on NTSC) to match the Rust version's `animation_speed_ms = 100` default.

---

## 8. Risk Assessment

| Risk                        | Likelihood | Impact | Mitigation                        |
|-----------------------------|------------|--------|-----------------------------------|
| FOV too slow on real HW     | Low        | High   | Already budgeted at ~2,700 cycles; profile early on VICE |
| Map gen produces dead-end dungeons on small maps | Medium | Medium | Tune room count/size; add connectivity validation |
| Save corruption on 1541     | Low        | High   | Add 16-bit checksum; verify on load |
| Custom charset looks bad    | Medium     | Low    | Iterate with CharPad editor; study existing C64 roguelikes |
| 256-byte stack overflow     | Low        | High   | Avoid deep recursion (already mitigated by Bresenham FOV) |
| cc65 C code too large/slow  | Medium     | Medium | Profile early; rewrite hot paths in asm |
| UII+ command interface underdocumented | Medium | Medium | Study UII+ firmware source (open source on GitHub); test on real hardware early |
| Network timeout blocks game loop | Medium | High | All network I/O is non-blocking with timeout; game remains playable if server is down |
| HTTP response too large for C64 buffer | Low | Medium | Server returns minimal payloads; C64 client uses streaming parse (process bytes as they arrive, don't buffer entire response) |
| MCP round-trip too slow for enjoyable spectation | Low | Low | LLM inference is 1-3s — acceptable for turn-based; add "thinking..." indicator |
| UII+ firmware version fragmentation | Medium | Medium | Target UII+ firmware 3.x+ command API; document minimum firmware version |
| Atproto bridge adds deployment complexity | Medium | Low | Bridge is optional; game works fully without it; Docker makes deployment simple; Raspberry Pi on LAN is ideal host |
| Bluesky deprecates app passwords | Low | Medium | Bridge design includes OAuth upgrade path via web UI; monitor atproto auth discussions |
| C64↔PC save format incompatibility | High | Medium | v1: separate PDS records per platform; v2: bridge converts binary↔JSON for true portability |

---

## 9. Prior Art

Roguelikes that have shipped on 8-bit platforms, demonstrating feasibility:

- **Rogue (1980)** — The original, ran on PDP-11 and early Unix terminals.
  40x24 display, simpler FOV (line-of-sight only), no shadowcasting.
- **Sword of Fargoal (1982)** — C64 native. Procedural dungeons, fog of war,
  combat. Proved the concept works beautifully on this hardware.
- **Gateway to Apshai (1983)** — C64. Real-time dungeon crawling with
  joystick control and multiple dungeon levels.
- **Hack (1985)** — Available on various 8-bit platforms. Complex inventory
  and interaction systems within tight memory constraints.
- **C64 Angband variants** — Community ports of Angband-like games to the C64,
  demonstrating that deep roguelike mechanics fit in 64 KB.

---

## 10. Deliverables

1. **Source code** — New `crates/c64/` directory (or standalone repo) containing
   cc65 C sources and 6502 assembly files.
2. **Build system** — Makefile targeting cc65, producing .prg, .d64, and .crt
   outputs for both stock and UII+ builds.
3. **Custom charset** — `charset.bin` (2 KB) designed in CharPad or similar
   tool.
4. **Disk image** — `roguelike.d64` ready for emulators and real hardware.
5. **Cartridge image** — `roguelike.crt` for EasyFlash/similar flash
   cartridges (instant load, no disk drive needed).
6. **Server endpoints** — Leaderboard, daily seed, cloud save, and spectation
   relay endpoints added to the existing Rust binary (alongside the SSH and
   MCP servers). Single deployment serves PC, SSH, MCP, and C64 clients.
7. **AT Protocol bridge** — `tools/c64-bridge/` standalone project. Docker
   image published to GHCR. Self-hostable on a Raspberry Pi. Provides PDS
   save storage, federated spectation, and Bluesky identity for C64 players.
   Designed in [docs/design/c64-atproto-bridge.md](design/c64-atproto-bridge.md).
8. **Design document** — This proposal, updated with implementation notes.

---

## 11. Open Questions

1. **Map size:** Fixed 40x22 (no scrolling) vs. 64x48 with scrolling viewport?
   Scrolling is more work but enables richer dungeons.

2. **Multiple dungeon levels?** The Rust version is single-floor. The C64
   version could add stairs and multiple levels (swap map data on level
   change). This is a natural expansion for the "what gets added" column.

3. **PAL vs. NTSC timing:** The game logic is turn-based so this doesn't
   affect gameplay, but animation timing and SID tuning differ. Support both
   with a PAL/NTSC detection routine at startup.

4. **Target hardware baseline:** Stock C64 with 1541 drive? Or also support
   REU (RAM Expansion Unit), SD2IEC, Pi1541, etc.? REU would enable much
   larger maps and instant saves. The Ultimate 64 (with built-in UII+) is the
   recommended enhanced target for network features.

5. **Cartridge vs. disk:** A 16 KB cartridge image means instant loading and
   no disk I/O for saves (use cartridge EEPROM or skip saves). A disk version
   supports saves but has slow loading. Possibly ship both.

6. **UII+ network API stability:** The Ultimate-II+ command interface has
   evolved across firmware versions. Should we target a minimum firmware
   version (e.g., 3.10+) or implement version detection with feature
   negotiation? The UII+ firmware is open source, which helps.

7. **Server hosting:** The leaderboard / daily seed / cloud save endpoints
   need a server. Options: (a) self-hosted alongside the existing SSH/MCP
   server, (b) a free-tier cloud endpoint, or (c) a peer-to-peer model
   where the C64 talks directly to another player's Rust binary on their LAN.
   Option (a) is simplest since we already have a Rust server binary.

8. **Cross-platform leaderboard fairness:** C64 maps are 40x22 with fewer
   monsters; PC maps are 80x40. Should leaderboards be per-platform, or
   should daily challenges use a C64-sized map on all platforms for parity?

9. **MCP client: who controls the game?** When the C64 is in MCP client mode,
   does the local player watch passively, or can they interrupt and take over
   mid-game (like a "co-pilot" mode)? The Rust MCP server already supports
   alternating between human and LLM turns.

10. **Network save authentication:** Cloud saves need some form of player
    identity. Options: simple player-name string (no security, honor system),
    a pre-shared key entered once, or a pairing code displayed on the server.
    For a C64 roguelike, the honor system seems appropriate. The atproto
    bridge solves this differently — credentials are stored on the bridge,
    not the C64 (see question 11).

11. **Atproto bridge auth method:** The bridge design proposes app passwords
    for v1 (simplest) with an OAuth upgrade path. Bluesky has discussed
    deprecating app passwords in favor of OAuth-only. Should we implement
    OAuth from the start via the bridge's web UI, or ship with app passwords
    and upgrade later? See
    [c64-atproto-bridge.md](design/c64-atproto-bridge.md#authentication)
    for the full analysis.

12. **Cross-platform save portability:** C64 saves are ~1.6 KB compact binary;
    PC/SSH saves are ~20-80 KB JSON. The atproto bridge stores C64 saves as
    opaque blobs with a metadata header. For true cross-platform portability
    (continue a C64 game on PC, or vice versa), the bridge would need to
    convert between binary and JSON formats. Is this a v1 requirement, or an
    aspirational goal for later? See
    [c64-atproto-bridge.md](design/c64-atproto-bridge.md#open-questions)
    question 7.

---

## 12. Conclusion

The roguelike's architecture is well-suited for a C64 port. The core game
loop is simple (turn-based, grid-based, integer combat), the Rust codebase
has clean module boundaries that map naturally to C64 source files, and the
entire game state fits comfortably in ~4 KB of RAM.

The main engineering challenges are:
1. Replacing shadowcasting FOV with Bresenham raycasting (solvable, well-understood)
2. Fitting the map into a 40x22 viewport (solvable, just smaller dungeons)
3. Disk I/O for saves (slow but acceptable for a roguelike)
4. UII+ network driver layer (new, but the command interface is documented and
   the firmware is open source)

What makes this port *exciting* is what the C64 **adds**: SID chip audio,
custom character graphics, joystick control, and the visceral satisfaction
of a complete game running on a 1 MHz 8-bit machine from 1982.

The Ultimate 64's Ethernet capability takes this further — from a standalone
retro port into a **networked node in the same ecosystem** as the PC, SSH,
and MCP versions. A C64 player's kill count appears on the same leaderboard
as a PC player's. A daily challenge seed unites players across 44 years of
hardware. An LLM plays the game through a MCP server while the C64 renders
each move on a 40-column screen. The server binary is the same Rust binary
that already serves SSH and MCP — the C64 is just another client.

The C64 roguelike wouldn't just be a downport — it would be the most
interesting client in the fleet.

Estimated total effort: **16-20 weeks** for an experienced 6502 developer
(including network features), **12-16 weeks** for the core game + sound
without networking, or **6-8 weeks** for the minimum viable game.
