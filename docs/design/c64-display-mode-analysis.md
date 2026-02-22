# C64 Display Mode Analysis

**Context:** The POC uses standard VIC-II character mode (40×25) with no
sprites, no custom charset, and no alternative display modes. This document
evaluates whether an alternative VIC-II mode would benefit the roguelike,
and explains why standard character mode is the correct choice.

**Related:**
[Port proposal](../c64-port-proposal.md) §6.8-6.9,
[Demo techniques for roguelike](c64-demo-techniques-for-roguelike.md) §4,
[Demo scene research](../../c64-demo-scene-techniques.md) §6.

---

## VIC-II Display Modes

| Mode | Resolution | Colors | Cell size | Screen RAM | Bitmap RAM | CPU avail/bad line |
|------|-----------|--------|-----------|------------|------------|-------------------|
| **Standard text** | 40×25 chars (320×200 px) | 2 per cell | 8×8 | 1 KB | — | ~23 cycles |
| **Multicolor text** | 40×25 chars (160×200 px) | 4 per cell (half-width px) | 4×8 | 1 KB | — | ~23 cycles |
| **Hires bitmap** | 320×200 px | 2 per 8×8 cell | 8×8 | 1 KB | 8 KB | ~23 cycles |
| **Multicolor bitmap** | 160×200 px | 4 per 4×8 cell | 4×8 | 1 KB | 8 KB | ~23 cycles |
| **ECM (Extended Color)** | 40×25 chars | 2 per cell, from 4 backgrounds | 8×8 | 1 KB | — | ~23 cycles |
| **FLI** (forced bad lines) | 160×200 px | 4 per 4×1 cell | 4×1 | 8×1 KB | 8 KB | **~4 cycles** |

Standard text mode is controlled by `$D011` (BMM=0, ECM=0) and `$D016`
(MCM=0). The VIC-II reads a character code from screen RAM, uses it as an
index into the charset (256 entries × 8 bytes each), and renders the 8×8
pattern using the cell's Color RAM foreground color against the shared
background color (`$D021`).

---

## Why Each Alternative Mode Is Wrong for a Roguelike

### Bitmap Modes (Hires & Multicolor)

In bitmap mode, every pixel is individually addressable — all 320×200 (or
160×200) dots are under direct control. This sounds powerful, but it inverts
the cost structure for tile-based games.

**Update cost per tile move:**

| Operation | Character mode | Bitmap mode |
|-----------|---------------|-------------|
| Erase old position | 1 byte (screen code) | 8 bytes (restore floor bitmap) |
| Draw new position | 1 byte (screen code) | 8 bytes (monster bitmap) |
| Color at old position | 1 byte (color RAM) | 1 byte (color attribute) |
| Color at new position | 1 byte (color RAM) | 1 byte (color attribute) |
| **Total per entity move** | **2 bytes, ~16 cycles** | **18 bytes, ~144 cycles** |

With 15 monsters moving per turn, bitmap mode costs ~2,160 cycles for
entity rendering vs ~240 cycles in character mode — a **9× penalty** for
zero gameplay benefit.

The bitmap itself consumes **8 KB** — nearly half of the current game's
total memory footprint (POC: 13 KB code + ~3 KB data). The memory budget
(port proposal §4.2) allocates ~25 KB total with ~21 KB headroom. An 8 KB
bitmap consumes 38% of that headroom for a feature that makes rendering
*slower*.

The color resolution is identical: 2 colors per 8×8 cell (hires bitmap)
or 4 colors per 4×8 cell (multicolor bitmap). Character mode achieves the
same color resolution with a well-designed charset — the VIC-II applies
the same color attribute logic regardless of whether the pixel pattern
comes from a charset lookup or direct bitmap data.

**When bitmap modes make sense:** Drawing arbitrary graphics — paint programs,
photo viewers, filled polygon renderers, plot graphs. A roguelike draws the
same ~20 tile types hundreds of times per frame. That's literally what
character mode's 256-entry lookup table is designed for.

### Multicolor Text Mode

Multicolor text gives 4 colors per character cell instead of 2, but at the
cost of **halving horizontal resolution** — each "pixel" is 2 dots wide, so
characters are effectively 4×8 instead of 8×8.

**The tradeoff for a roguelike:**

| Aspect | Standard text | Multicolor text |
|--------|--------------|-----------------|
| Horizontal pixel resolution | 8 px per cell | 4 px per cell |
| Colors per cell | 2 (fg + bg) | 4 (fg + bg + mc1 + mc2) |
| Glyph readability | High | Reduced (blocky) |
| Text rendering | Clean | Pixelated |

The roguelike is text-heavy: `@`, `G`, `O`, `T` entity glyphs, the HP bar
numbers, the status bar labels ("HP", "K:", "T:"), and the 2-3 line message
log. All of these become blocky and harder to read at half horizontal
resolution.

The 4-color benefit doesn't compensate. Dungeon tiles (`.` floor, solid
block wall, space) need high contrast and readability, not color variety.
The current POC's color scheme — light grey walls, dark grey floors, blue
explored tiles, colored entity glyphs — uses Color RAM's per-cell foreground
color effectively within 2-color mode.

**When multicolor text makes sense:** Top-down RPGs with graphical tilesets
(Ultima-style) where each 4×8 cell contains a small sprite-like icon with
shading. The extra colors add depth to richly drawn tiles. For a roguelike's
abstract single-character glyphs, readability matters more than color depth.

### ECM (Extended Color Mode)

ECM lets each character cell select one of 4 background colors (stored in
`$D021`-`$D024`) instead of sharing the single `$D021` background. The
top 2 bits of the character code select the background; the remaining 6
bits address the charset.

**The application that seems ideal:** FOV visualization. Visible tiles use
background color 0 (black), explored-but-dark tiles use background color 1
(dark blue), unexplored tiles use background color 2 (black), and the status
bar uses background color 3 (dark grey). Four zones, four backgrounds —
exactly what ECM provides.

**The fatal constraint:** ECM reduces the usable charset from **256 to 64
characters**. The top 2 bits of the character code are repurposed for
background selection, leaving only 6 bits for the charset index.

The current POC uses character codes across a wide range:

```
$00     @  (player)
$07     G  (goblin)
$0F     O  (orc)
$14     T  (troll)
$20     space (empty/unexplored)
$25     %  (corpse)
$2E     .  (floor)
$65     light shade (HP bar empty)
$A0     reverse space (solid wall)
$C0     horizontal line (game over box)
$DD     vertical line (game over box)
```

That's already 11 distinct codes, and the custom charset plan (port proposal
§6.9) adds HP bar segments ($08-$0F), item glyphs ($10-$17), box-drawing
characters ($18-$1F), and a full uppercase alphabet ($20-$5A). With 64
characters total, there isn't room for a dungeon tileset AND a text font AND
UI elements AND item icons.

**The alternative that achieves the same goal:** Raster interrupts changing
`$D021` per screen zone. This gives zone-specific background colors (the
demo techniques report recommends exactly this) without sacrificing charset
range. The torchlight gradient effect (changing `$D021` per raster line)
goes further than ECM's 4-color limit.

ECM also cannot be combined with bitmap mode or multicolor mode — setting
both ECM and BMM in `$D011` creates an "invalid mode" that forces all
output to black.

### FLI (Flexible Line Interpretation)

FLI forces a bad line on every raster line, switching the screen RAM pointer
each line to provide per-line color resolution. Combined with multicolor
bitmap, this gives 4 colors per 4×1 cell — dramatically better than the
standard 4×8 color cell.

**Why it's wrong for gameplay:** FLI consumes nearly all CPU time. With
sprites enabled, only ~4 cycles remain per raster line on a bad line. The
FLI loop must be unrolled for every line — no room for game logic, AI, FOV
computation, or input polling. The game would freeze during FLI display.

**Where FLI works:** Static screens. The title screen and death screen are
the only places FLI could run, since the game is waiting for a keypress.
Even there, FLI consumes ~17 KB of memory (8 screen RAMs + bitmap), eating
most of the memory headroom.

FLI is evaluated in the
[demo techniques report](c64-demo-techniques-for-roguelike.md) §9 as a
"v2 luxury" — achievable but not cost-effective relative to a PETSCII title
screen enhanced with raster color bars.

---

## Why Standard Character Mode Is Ideal

The POC already demonstrates the core advantages. This section makes them
explicit.

### 1. Minimal Update Cost

Changing a tile requires **2 writes**: one byte to screen RAM (character
code), one byte to Color RAM (foreground color).

```rust
// Current POC (c64.rs:94-99)
pub fn draw_char(x: u8, y: u8, sc: u8, color: u8) {
    let offset = (y as usize) * 40 + (x as usize);
    unsafe {
        write_volatile(SCREEN.add(offset), sc);
        write_volatile(COLOR_RAM.add(offset), color);
    }
}
```

Two `STA` instructions = ~8 cycles per cell. The full 880-cell redraw costs
~20,000 cycles (including loop overhead). With dirty-rectangle rendering
(port proposal §6.8), only changed cells are updated — typically ~20 cells
per turn = ~160 cycles.

No other display mode achieves this efficiency for tile-based content.

### 2. Hardware Deduplication via Charset

The VIC-II's character mode is essentially a **hardware tile engine** with a
256-entry tile palette. The charset is a lookup table: character code → 8×8
pixel pattern. Every floor tile on screen references the same 8-byte
definition. Every wall tile references the same definition.

A dungeon with 200 visible floor tiles:
- **Character mode:** 200 bytes of screen RAM, all pointing to the same
  8-byte charset entry. Total unique pixel data: 8 bytes.
- **Bitmap mode:** 200 × 8 = 1,600 bytes of identical pixel data in the
  bitmap buffer.

The charset's deduplication is free — the VIC-II performs the lookup in
hardware during the dot clock, consuming zero CPU cycles.

### 3. Charset Animation Is Global

Modifying the 8 bytes that define a character changes every instance of that
character on screen simultaneously. This enables:

- **Water animation:** Rewrite the "water" character's 8-byte definition
  each frame. All water tiles animate in sync. Cost: 8 bytes/frame = ~40
  cycles. In bitmap mode, animating 30 water tiles would cost 30 × 8 = 240
  bytes/frame = ~1,920 cycles.

- **Torch flicker:** Alternate between two 8-byte definitions for the
  "torch" character on odd/even frames.

- **Pulsing stairs glyph:** Cycle through 4 definitions (bright → dim →
  bright) over 16 frames.

This property is unique to character mode and fundamentally unavailable in
bitmap mode.

### 4. Text Rendering Is Native

The status bar, message log, number displays, and menu text are all rendered
by writing character codes to screen RAM. No font rendering code, no
bitmap blitting, no glyph lookup tables — just `screen_ram[offset] = char`.

The current POC renders "HP 24/30 K:5 T:0123" with a sequence of `draw_text`
and `draw_number` calls that total ~200 bytes of code. In bitmap mode, the
same text would require a full software font renderer (~500+ bytes of code)
plus a font bitmap (~2 KB), and each character would require 8 bitmap writes
instead of 1 screen RAM write.

### 5. Per-Cell Color via Color RAM

Each of the 1,000 screen cells has an independent 4-bit foreground color
stored in Color RAM ($D800-$DBE7). The POC uses this effectively:

| Element | Color | Index |
|---------|-------|-------|
| Visible floor | Dark Grey | 11 |
| Visible structural wall | Light Grey | 15 |
| Explored floor (not visible) | Blue | 6 |
| Explored wall (not visible) | Blue | 6 |
| Unexplored | Black | 0 |
| Player (@) | Yellow | 7 |
| Goblin (G) | Green | 5 |
| Orc (O) | Brown | 9 |
| Troll (T) | Red | 2 |

This provides 16 distinct foreground colors across the screen with no CPU
cost beyond the initial write — the VIC-II reads Color RAM on the same
cycle as screen RAM via its private D8-D11 data lines.

### 6. Maximum CPU for Game Logic

Bad lines steal ~40 cycles per character row (every 8th raster line within
the display window). With 25 character rows, that's 25 bad lines × ~40
cycles = ~1,000 cycles/frame of VIC-II overhead.

This overhead is identical across all text modes (standard, multicolor,
ECM) and *increases* in bitmap mode (the VIC-II fetches more data per line
for bitmap patterns). FLI forces a bad line on every line, pushing
overhead to 200 × ~40 = ~8,000 cycles/frame.

Standard character mode minimizes VIC-II overhead, leaving the most CPU
cycles for game logic:

| Mode | Bad lines/frame | VIC overhead | CPU remaining (PAL) |
|------|----------------|-------------|-------------------|
| Standard text | 25 | ~1,000 cyc | ~18,656 cyc |
| Multicolor text | 25 | ~1,000 cyc | ~18,656 cyc |
| Hires bitmap | 25 | ~1,000 cyc | ~18,656 cyc |
| Multicolor bitmap | 25 | ~1,000 cyc | ~18,656 cyc |
| ECM | 25 | ~1,000 cyc | ~18,656 cyc |
| FLI | 200 | ~8,000 cyc | ~11,656 cyc |

(Bad line overhead is the same for text and bitmap modes because the VIC-II
always fetches 40 bytes of screen data per bad line. Bitmap modes add g-
accesses on every line, but these use the phi1 half-cycle that the CPU
can't use anyway.)

---

## The Enhancement That Matters

The visual quality improvement comes not from switching display modes but
from **enhancing character mode with demo scene techniques**:

1. **Custom charset** (port proposal §6.9) — Purpose-built dungeon tiles,
   HP bar segments, item icons, and UI elements. Same mode, better graphics.

2. **Per-zone charset switching** (demo techniques report §4) — Raster
   interrupts swap `$D018` at zone boundaries. The dungeon area uses a
   graphical tileset; the status bar uses a clean UI font; the message log
   uses a readable text font. Up to 7 charsets in a single VIC bank.

3. **Raster color effects** (demo techniques report §5) — Per-line `$D021`
   changes for torchlight gradient, damage flash, zone-specific backgrounds.
   All within character mode.

4. **Sprite overlays** (demo techniques report §6) — Hardware sprites for
   the player and key entities, rendered on top of the character-mode map.
   The dungeon environment stays in character mode; sprites add animation
   and visual distinction to moving entities.

5. **Charset animation** (demo techniques report §4.2) — Animated water,
   torches, stairs by cycling character definitions. Unique to character
   mode.

These techniques stay entirely within standard character mode but achieve
visual quality that rivals — or exceeds — what bitmap modes can offer for
a tile-based game.

---

## Decision

**Standard VIC-II character mode (40×25) is the correct display mode for
the C64 roguelike.** No mode change is recommended.

The visual improvement path is:
1. Custom charset (Phase 2, port proposal)
2. Raster interrupt chain for color effects and charset switching (demo
   techniques report, steps 1-5)
3. Sprite overlays for entities (demo techniques report, step 6)

All within standard character mode. All leveraging the mode's inherent
strengths — minimal update cost, hardware deduplication, global charset
animation, native text rendering, and maximum CPU availability.
