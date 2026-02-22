# C64 Demo Scene Techniques for the Roguelike

**Context:** This document analyzes which demo scene techniques could improve the
C64 roguelike port. It cross-references the [port proposal](../c64-port-proposal.md),
the [gameplay implementation plan](gameplay-implementation-plan.md), the current
POC source (`crates/c64/`), and the
[demo scene research](../../c64-demo-scene-techniques.md).

**Current state:** The POC uses standard VIC-II character mode (40x25) with no
raster interrupts, no sprites, no custom charset, and no VIC-II tricks of any
kind. It does a full 880-cell redraw each turn via `write_volatile` to screen
memory ($0400) and color RAM ($D800). Total frame cost: ~20,000 cycles.

---

## Technique Assessment Summary

| Technique | Relevance | Effort | Impact | Verdict |
|-----------|-----------|--------|--------|---------|
| [Raster interrupts for color effects](#1-raster-interrupts) | High | Low | High | **Do this** |
| [Top/bottom border removal](#2-topbottom-border-removal) | High | Very Low | Medium | **Do this** |
| [FLD for screen splitting](#3-fld-for-screen-splitting) | High | Low | High | **Do this** |
| [Custom charset with demo tricks](#4-custom-charset-enhancement) | High | Medium | High | **Do this** |
| [Color RAM atmospheric lighting](#5-color-ram-atmospheric-lighting) | High | Low | High | **Do this** |
| [Sprite overlays for entities](#6-sprite-overlays-for-entities) | Medium | Medium | High | **Consider for v1.1** |
| [Tech-tech for magic effects](#7-tech-tech-for-magic-effects) | Medium | Medium | Medium | **Consider for v1.1** |
| [VSP/AGSP for scrolling viewport](#8-vspagsp-for-scrolling-viewport) | Medium | High | High | **Risky — evaluate** |
| [FLI for title/death screens](#9-fli-for-titledeath-screens) | Low | High | Medium | **Luxury — v2** |
| [Sprite multiplexing for particles](#10-sprite-multiplexing-for-particles) | Low | High | Low | **Skip** |
| [DYCP for message scrolling](#11-dycp-for-message-scrolling) | Low | High | Low | **Skip** |
| [Linecrunch for transitions](#12-linecrunch-for-level-transitions) | Medium | Medium | Medium | **Consider for v1.1** |

---

## Detailed Analysis

### 1. Raster Interrupts

**What it is:** The VIC-II fires an interrupt when the raster beam reaches a
programmable scanline. This lets you change VIC-II registers (colors, scroll
offsets, character set pointers) at different vertical positions on the screen,
creating effects impossible in a single static configuration.

**Why it matters for the roguelike:** The current POC has zero raster interrupt
usage — it relies entirely on the Kernal's default IRQ handler for keyboard
scanning. Setting up even a basic raster interrupt chain unlocks every other
technique on this list.

**Specific applications:**

1. **Split-screen color schemes.** The game area (rows 0-21), status bar
   (row 22), and message log (rows 23-24) could each have different background
   colors. The dungeon uses black, the status bar uses dark grey, the message
   log uses dark blue. Currently all three zones share `$D021` (background
   color 0), so they're all black.

2. **Per-line background color for depth.** Within the dungeon area itself,
   a subtle vertical gradient (black → dark grey → black) creates a "torchlight
   falloff" effect centered on the player's row. This requires changing `$D021`
   on each raster line — ~63 cycles per line of the gradient zone.

3. **Status bar highlight.** Flash `$D021` to red for a single frame when the
   player takes damage. The raster interrupt ensures only the status bar row
   flashes, not the entire screen.

4. **IRQ-driven SID music.** The proposal (§6.11) plans SID sound effects.
   A raster interrupt is the standard way to drive a SID music player — call
   the play routine once per frame at a fixed raster line. Without a raster
   IRQ, SID timing is tied to the game loop, which is turn-based and
   irregular.

**Implementation sketch (rust-mos):**

The `mos-hardware` crate provides `vic2::raster_interrupt()` and related
helpers, but the POC currently uses raw `poke`/`peek`. Either way, the
setup is:

```rust
// Disable Kernal IRQ, enable VIC raster IRQ
poke(0xDC0D, 0x7F);           // CIA 1: disable all CIA interrupts
peek(0xDC0D);                  // acknowledge pending
poke(0xD01A, 0x01);           // VIC: enable raster interrupt
poke(0xD012, STATUS_RASTER);  // target raster line
// Point IRQ vector to our handler
poke_word(0xFFFE, handler_addr); // requires Kernal ROM banked out
// ... or use the Kernal vector at $0314/$0315 if Kernal stays banked in
```

The handler changes `$D021` for the status bar zone, calls the SID player,
and sets up the next interrupt target. On a turn-based game, the raster IRQ
runs continuously in the background — it doesn't interfere with the game
loop's `wait_for_input()` blocking call.

**Cost:** ~200 bytes of code, ~500 cycles per frame (for a 3-zone split with
SID callback). Negligible.

**Compatibility with rust-mos:** Raster interrupt handlers need to be `extern
"C"` functions with manual register save/restore. rust-mos doesn't support
`asm!` for 6502, so the save/restore must be done via C FFI stubs or by
using `mos-hardware`'s IRQ support. The `cbm_kernal` module has examples.

**Prerequisite for:** techniques 2, 3, 5, 6, 7, 12.

---

### 2. Top/Bottom Border Removal

**What it is:** Switch from 25-row mode (RSEL=1) to 24-row mode (RSEL=0)
at the exact raster line where the VIC-II checks the vertical border
flip-flop. The VIC "misses" the border trigger and the border stays open.

**Why it matters:** The C64's visible border wastes ~51 raster lines at top
and bottom. Opening the top/bottom border provides a **free expansion zone**
for UI elements that don't consume the precious 40x25 character area.

**Specific applications:**

1. **Move the status bar into the top border.** The current layout sacrifices
   3 rows of gameplay area (rows 22-24) for the status bar and message log.
   With the top border open, status information could be rendered as **sprite
   text in the border** — 8 sprites across the top provide 192 pixels of
   width, enough for "HP████░░ 24/30 Lv3 K:7". This frees row 22 for
   gameplay, expanding the map from 40x22 to **40x23** (or 40x24 if
   messages also move to the border).

2. **Bottom border for messages.** Similarly, the 2-3 line message log
   could move to the bottom border as sprite text, freeing rows 23-24.

3. **Visual polish.** An open border looks dramatically better than the
   default C64 frame. It signals "this is a polished production" to the
   C64 community.

**Implementation cost:**

Opening top/bottom borders is the cheapest demo trick — a single register
write per frame:

```rust
// At raster line 249 (before line 251 where RSEL=1 border triggers):
fn border_open_handler() {
    let ctrl = peek(0xD011);
    poke(0xD011, ctrl & 0xF7);  // clear RSEL → 24-row mode
    // Border flip-flop never triggers because line 247 (24-row) already passed
}
// Before line 51 of next frame:
fn border_restore_handler() {
    let ctrl = peek(0xD011);
    poke(0xD011, ctrl | 0x08);  // set RSEL → 25-row mode
}
```

Total cost: **2 register writes per frame.** ~20 cycles. This is essentially
free.

**Tradeoff:** Rendering content in the border requires sprites (the VIC-II
only outputs the "idle state" pattern in the border area — no character
data). If we don't want sprites, the border is simply black — still
aesthetically better than the default blue/grey frame.

**Recommendation:** Open borders in Phase 2 alongside the custom charset.
Even without sprites, the black border is cleaner. Add sprite-based
border UI in v1.1 if the 3 extra gameplay rows are worth the sprite budget.

---

### 3. FLD for Screen Splitting

**What it is:** Flexible Line Distance suppresses bad lines by continuously
incrementing YSCROLL ahead of the raster counter. Without bad lines, the
VIC-II re-displays the last fetched character row, and the raster beam
advances without rendering new data. This creates a "blank gap" of any
height.

**Why it matters for the roguelike:** FLD provides a clean, hardware-level
separation between the scrolling game area and the static UI.

**Specific applications:**

1. **Smooth HUD/map boundary.** Instead of simply rendering the status bar
   at row 22 (which looks like part of the same screen), use FLD to insert
   a 1-2 pixel gap between row 21 (last map row) and row 22 (status bar).
   The gap renders as background color and creates a visual "divider" without
   consuming a character row.

2. **Status bar immune to scroll.** When the proposal's v1.1 adds a scrolling
   viewport (§6.1, open question #1), the game area will scroll but the
   status bar must stay fixed. FLD is the standard way to achieve this on
   the C64: let the game area scroll with hardware scroll registers, then
   use FLD to suppress bad lines in the status zone and switch to a separate
   screen pointer for the HUD.

3. **Game-over overlay animation.** The current `render_game_over()` just
   draws a box at row 8-14. With FLD, the "YOU HAVE DIED" overlay could
   *slide down from the top* by progressively reducing the FLD gap each
   frame — the overlay "pushes" the game area down. This is a classic
   demo effect (bouncing text) that would give death a dramatic feel.

4. **Dungeon level transition.** When the player descends stairs (gameplay
   plan Phase 2), FLD can wipe the screen by pushing the entire display
   off the bottom edge over 4-8 frames, then revealing the new level by
   reducing the gap. Much more dramatic than a simple clear-and-redraw.

**Implementation cost:** ~15 cycles per suppressed line. A 2-pixel gap costs
~30 cycles/frame. A full-screen wipe (200 lines) costs ~3,000 cycles but
only runs during transitions.

**Compatibility:** FLD modifies `$D011` bits 0-2 (YSCROLL), which conflicts
with hardware vertical scrolling. If the scrolling viewport is added later,
FLD and V-scroll must be carefully coordinated. For the fixed 40x21 map
(v1), there's no conflict.

---

### 4. Custom Charset Enhancement

**What it is:** The port proposal (§6.9) already plans a custom 2 KB charset.
Demo scene techniques can make this charset significantly more powerful.

**Demo-enhanced charset strategies:**

1. **Per-line charset switching (Tech-Tech lite).** The VIC-II's charset
   pointer (`$D018` bits 1-3) can be changed on every raster line via a
   raster interrupt. This means different rows of the screen can use
   **different character sets**. Application: the game area uses a dungeon
   tileset, the status bar uses a clean UI font, and the message log uses
   a compact text font — all without sacrificing character codes.

   With 7 possible charset positions in a 16 KB VIC bank, you could have
   up to 7 different charsets active simultaneously (one per screen zone).
   Practically, 2-3 are useful: dungeon tiles + UI font + text font.

   **Memory cost:** Each additional charset is 2 KB. Two extra charsets =
   4 KB additional. The memory budget (§4.2) has ~21 KB headroom, so this
   is comfortable.

2. **Animated tiles via charset cycling.** Water, lava, torches, and
   magical effects can animate by rotating character definitions in the
   charset RAM. Change 8 bytes (one character's pixel data) per frame to
   cycle through 4 animation frames. A bubbling water tile or flickering
   torch costs **8 bytes of charset write per frame** (~40 cycles).

   This is dramatically cheaper than redrawing screen RAM — you change
   the font definition and *every instance of that character on screen
   updates simultaneously*. A dungeon with 50 water tiles animates with
   the same 8-byte write as a dungeon with 1 water tile.

3. **2x2 tile blocks.** Use 4 adjacent character codes to form larger
   16x16 "metatiles" for key features: the player character, boss
   monsters, treasure chests, stairs, altars. This requires reserving
   blocks of character codes (e.g., codes $00-$03 = player's four
   quadrants) and rendering them as 2x2 groups. The chirp8-c64 project
   (§3.2 of the proposal) validates this approach.

   **Tradeoff:** 2x2 tiles halve the effective map resolution. Best used
   selectively for important entities, not for all floor/wall tiles.

4. **Dynamic charset for FOV effects.** Instead of using Color RAM alone
   for fog of war, dynamically modify character definitions for "explored
   but not visible" tiles. A wall character in the visible FOV shows full
   brick detail; the same character code outside the FOV shows a dimmed,
   simplified version. This requires maintaining two copies of each dungeon
   character (lit and dim) and patching the charset based on FOV state.

   **Cost:** ~16 bytes per dual-definition character (8 lit + 8 dim).
   With 8 dungeon tile types, that's 128 bytes of charset data, swapped
   on FOV changes. This is only worth it if the visual improvement over
   Color RAM dimming (blue tint on explored tiles) is significant.

**Implementation note:** Per-line charset switching requires a raster
interrupt (technique #1). Without rasters, you're limited to a single
charset — still useful, but less powerful.

---

### 5. Color RAM Atmospheric Lighting

**What it is:** Change `$D020` (border) and `$D021` (background) on each
raster line via a raster interrupt to create horizontal color bands.
Combined with Color RAM manipulation, this simulates lighting effects.

**Why it matters:** The current POC uses static colors — visible tiles are
light grey/dark grey, explored tiles are blue, unexplored are black. This
is functional but flat. Raster-timed color changes add depth and atmosphere
with minimal CPU cost.

**Specific applications:**

1. **Torchlight gradient.** Center a vertical color gradient on the player's
   Y position. Rows near the player use a warm `$D021` (dark grey or brown),
   rows further away fade to black. This simulates torchlight falloff
   *independently of the FOV system* — the FOV determines what's visible,
   the gradient determines the ambient mood.

   Implementation: In the raster interrupt chain, change `$D021` based on
   distance from the player's screen row. With the player at row 10:

   ```
   Rows  0-5:   $D021 = black (0)
   Rows  6-7:   $D021 = dark grey (11)
   Rows  8-12:  $D021 = brown (9) or grey (12) — warm zone
   Rows 13-14:  $D021 = dark grey (11)
   Rows 15-21:  $D021 = black (0)
   ```

   The gradient table is pre-computed and shifted each time the player
   moves vertically. Cost: one `STA $D021` per raster line in the map area
   = 22 register writes = ~110 cycles/frame.

2. **Damage flash.** When the player takes damage, set `$D020` (border) to
   red for 2-3 frames. The raster interrupt handles this independently of
   the game loop — the flash happens instantly, not on the next turn's
   render. This is how commercial C64 games handle hit feedback.

3. **Low-HP warning.** When HP drops below 30%, pulse the border color
   between black and dark red on alternating frames. The raster interrupt
   checks player HP and modulates `$D020` accordingly.

4. **Dungeon depth atmosphere.** Deeper floors could shift the color palette
   warmer (fire dungeon = red tints) or cooler (ice dungeon = blue tints)
   by modifying the background color gradient. This ties into the gameplay
   plan's depth system (Phase 2) and mood system (Phase 5).

**Cost:** The raster interrupt chain for a simple gradient adds ~150
cycles/frame (~0.7% of the frame budget). The gradient table is ~25 bytes.
This is one of the highest impact-to-cost ratios of any technique.

**Interaction with dirty-rect rendering:** The proposal (§6.8) plans
dirty-rectangle rendering to avoid redrawing unchanged cells. Raster
color effects are orthogonal to this — they modify VIC-II registers, not
screen RAM. They work correctly even with dirty-rect rendering.

---

### 6. Sprite Overlays for Entities

**What it is:** Use hardware sprites to render the player and key monsters,
overlaid on top of the character-mode dungeon map.

**Why it matters:** Characters in a fixed charset are 8x8 monochrome (or
4x8 multicolor). Sprites are 24x21 with independent colors, expandable to
48x42, and can be animated smoothly between tiles. This creates a dramatic
visual distinction between entities and the environment.

**Specific applications:**

1. **Animated player sprite.** Instead of a static `@` character, the player
   is a 24x21 sprite with idle animation (breathing, torch flicker). When
   the player moves, the sprite glides smoothly between tiles over 2-4
   frames instead of snapping instantly. The underlying character cell shows
   the floor tile while the sprite transitions.

   This is how *Sword of Fargoal* (1982) handled player movement — sprite
   over character-mode map — and it's dramatically more polished than
   character-mode-only rendering.

2. **Monster sprites for bosses.** Reserve 2-3 sprites for "important"
   monsters (bosses, quest targets). Regular goblins and orcs remain as
   charset characters, but trolls or unique enemies get full sprite
   treatment with animation.

3. **Cursor/selection sprite.** A highlight sprite (24x21, semi-transparent
   via multicolor mode) follows the player's movement target or hovers over
   the selected item during inventory management.

4. **Smooth FOV reveal.** When a new room is first revealed, sprites could
   briefly highlight the room entrance with a "light burst" effect (expand
   a white sprite from 1x1 to 24x21 over 4 frames, then remove).

**Implementation:**

Sprites are configured via VIC-II registers $D000-$D01E:

```rust
// Enable sprite 0 (player)
poke(0xD015, 0x01);           // sprite 0 enabled
poke(0xD000, player_pixel_x); // sprite 0 X position
poke(0xD001, player_pixel_y); // sprite 0 Y position
poke(0xD027, COLOR_YELLOW);   // sprite 0 color
poke(0x07F8, sprite_data_ptr / 64); // sprite 0 data pointer
```

Pixel coordinates are `tile_x * 8 + 24` (24 is the VIC-II's left border
offset) for X and `tile_y * 8 + 50` for Y.

**Cost:**
- Sprite DMA steals 2 cycles per sprite per raster line (within the
  sprite's vertical extent). With 3 sprites active: 6 cycles/line × 21
  lines = ~126 cycles/frame. Negligible.
- Sprite data: 63 bytes per frame × 4 animation frames × 4 entities =
  1,008 bytes. Within budget.
- Code: ~300 bytes for sprite setup, animation, and smooth movement.

**Tradeoff:** Using sprites for entities means fewer sprites available for
border UI (technique #2) or particle effects (technique #10). With 8
hardware sprites: reserve 1-2 for the player, 2-3 for visible monsters,
and 3-4 remain for UI or effects.

**Recommendation:** Add player sprite in Phase 2 alongside the custom
charset. It's a natural pairing — the charset handles the dungeon, sprites
handle entities. Monster sprites can follow in v1.1.

---

### 7. Tech-Tech for Magic Effects

**What it is:** Per-line horizontal displacement using the X-scroll register
(`$D016` bits 0-2) and charset pointer switching (`$D018`), creating a
wavy distortion of the screen.

**Why it matters:** Roguelikes have status effects — confusion, poison,
intoxication, magical sight. Tech-tech provides a visceral, hardware-level
way to communicate these states.

**Specific applications:**

1. **Confusion effect.** When the player is confused (if such a status
   is added), apply a gentle sine-wave tech-tech to the game area for
   the duration. Each raster line is displaced 0-3 pixels horizontally,
   following a slowly oscillating sine table. The dungeon appears to
   "breathe" or waver. The status bar and message log are unaffected
   (the raster interrupt stops the effect before row 22).

2. **Magic vision.** Drinking a "Scroll of Mapping" (items plan, Phase 3)
   could trigger a brief (1-2 second) tech-tech pulse across the entire
   screen as all tiles are revealed, simulating a magical shockwave.

3. **Damage taken distortion.** A single-frame tech-tech jolt (2-3 pixel
   displacement for 1 frame) on player damage. More visceral than a color
   flash alone, especially combined with a border flash (technique #5).

**Implementation:**

Tech-tech requires a stable raster interrupt chain covering the game area
(22 lines × 8 pixels = 176 raster lines). On each line:

```rust
// Timing-critical: must be exactly 63 cycles per iteration (PAL)
fn tech_tech_line(sine_idx: u8) {
    let displacement = SINE_TABLE[sine_idx];
    let coarse = displacement >> 3; // charset offset (0-7)
    let fine = displacement & 0x07; // pixel offset (0-7)
    poke(0xD018, charset_base_table[coarse]);
    poke(0xD016, 0x08 | fine); // CSEL=1, MCM=0, XSCROLL=fine
}
```

**Cost:**
- Requires **pre-shifted charsets** — one for each coarse displacement.
  With 3-4 pixel range: 3 extra charsets × 2 KB = 6 KB. The memory budget
  can absorb this if they're loaded on-demand (only when a tech-tech effect
  is active).
- CPU: ~10 cycles per raster line × 176 lines = ~1,760 cycles/frame while
  the effect is active. ~8.4% of the frame budget — significant but
  acceptable for a temporary effect.

**Tradeoff:** Tech-tech conflicts with per-line charset switching
(technique #4). If both the dungeon-tileset-per-zone feature and tech-tech
are desired, they need careful raster scheduling. In practice, tech-tech
would temporarily override the dungeon charset during the effect, then
restore normal display.

**Recommendation:** Implement as a post-v1 polish feature. It's high-impact
for atmosphere but requires pre-shifted charset data and careful timing
integration with whatever raster chain exists at that point.

---

### 8. VSP/AGSP for Scrolling Viewport

**What it is:** VSP (Variable Screen Positioning) shifts where the VIC-II
reads screen data horizontally without copying memory. Combined with
Linecrunch (vertical repositioning), this becomes AGSP — full 2D
hardware-assisted scrolling.

**Why it matters:** The port proposal (§6.1, open question #1) flags
scrolling as a v1.1 feature: "Add scrolling viewport over 64x48 maps."
AGSP would make this dramatically cheaper than software scrolling.

**The problem:** The VSP bug corrupts DRAM on many C64 units. The port
proposal targets the Ultimate 64 as the recommended platform, which uses
an FPGA recreation of the VIC-II. The FPGA VIC-II may or may not reproduce
the VSP bug — it depends on how faithfully the DRAM refresh timing is
emulated.

**Software scrolling alternative:**

For a turn-based roguelike, software scrolling (copying screen RAM) is
actually quite viable:

```
Full screen copy: 40 × 21 = 840 bytes
At ~10 cycles per byte (LDA/STA pair): ~8,400 cycles
Frequency: only on player movement (not every frame)
```

8,400 cycles per player move is ~40% of a PAL frame but happens at most
once per turn. For a turn-based game where the player waits for input,
this is imperceptible — the scroll completes in <10ms.

**Hardware scroll (safe subset):**

The hardware X-scroll register (`$D016` bits 0-2) provides 0-7 pixels of
smooth horizontal offset for free. The hardware Y-scroll (`$D011` bits
0-2) provides 0-7 pixels vertically. This means the coarse scroll (full
character shifts) can use software copy while the fine scroll (sub-
character) uses hardware — the best of both worlds.

For a turn-based game with discrete tile movement, even the fine scroll
may not be necessary — the player snaps from tile to tile. But if smooth
sprite movement (technique #6) is implemented, the camera could smoothly
pan between tiles using hardware scroll + software copy, creating a
polished scrolling experience without any VSP risk.

**Recommendation:** Use **software coarse scroll + hardware fine scroll**
for the v1.1 scrolling viewport. Skip VSP entirely — the reliability risk
isn't worth it for a turn-based game that only scrolls on player input.
If sprite-based player movement is implemented, use the hardware scroll
registers to animate the camera pan over 4-6 frames per tile move.

---

### 9. FLI for Title/Death Screens

**What it is:** Flexible Line Interpretation forces a bad line on every
raster line, providing per-line color resolution for multicolor bitmap
images.

**Why it matters:** The port proposal (§6.9) plans a PETSCII art title
screen. A FLI or NUFLI image would be dramatically more impressive — a
hand-painted dungeon scene or character portrait at near-photographic
quality.

**Tradeoff:** FLI consumes nearly all CPU time (~4 cycles/line on bad
lines with sprites). It cannot coexist with game logic. On the title
screen this is fine — the game is waiting for a keypress. On the death
screen, FLI would need to replace the game display entirely.

**Cost:**
- FLI image data: ~17 KB (8 screen RAMs + bitmap + color RAM)
- Code: ~500 bytes for the FLI display loop
- Combined: ~17.5 KB for one image

The memory budget has ~21 KB of headroom, so one FLI image fits. But it
consumes most of the headroom, leaving little room for multiple images.

**Alternative: PETSCII art + raster effects.** A well-designed PETSCII
title screen with raster color bars, animated charset characters, and
border removal looks impressive at a fraction of the cost. Many acclaimed
C64 games used PETSCII title screens (see: Sword of Fargoal, Boulder Dash).

**Recommendation:** Start with PETSCII art title screen enhanced with
raster color bars (technique #5) and animated charset characters (technique
#4). FLI is a luxury for v2, after the gameplay is polished — and even
then, the PETSCII approach may be more charming for a roguelike's
aesthetic.

---

### 10. Sprite Multiplexing for Particles

**What it is:** Reuse the 8 hardware sprites multiple times per frame to
display 16-96+ sprites.

**Why it matters for the roguelike:** Not much. Multiplexing shines in
shoot-'em-ups with dozens of simultaneous bullets. A roguelike has at most
16 entities, only ~5-8 visible at once, and they move one tile per turn.

If sprites are used for entities (technique #6), the 8 hardware sprites
are already sufficient: 1 player + up to 7 visible monsters. Multiplexing
would only help if we wanted *more* than 8 simultaneous sprite-based
entities, which the 40x21 viewport makes unlikely.

**One niche use case:** Particle effects (damage numbers, XP gain popups,
sparkle effects) could use a simple 2-zone multiplexer: game entities in
the top zone, particle effects in the bottom zone. But this adds complexity
for purely cosmetic benefit.

**Recommendation:** Skip. The 8 hardware sprites are sufficient. If more
are needed, it means the game design has grown beyond what sprite-based
entities are intended for — fall back to charset-mode entities for overflow.

---

### 11. DYCP for Message Scrolling

**What it is:** Different Y Character Position — characters bob vertically
in a sine wave while scrolling horizontally. A classic demo scroller.

**Why it matters for the roguelike:** It doesn't, really. The message log
is functional UI text, not a demo scroller. DYCP would be charming on the
title screen ("PRESS ANY KEY TO BEGIN" scrolling across with a sine bounce)
but the implementation cost is high for a purely decorative effect.

**Recommendation:** Skip. The message log should prioritize readability.

---

### 12. Linecrunch for Level Transitions

**What it is:** Selectively suppress bad lines to "crunch" screen rows,
creating vertical scroll/wipe effects.

**Why it matters:** Dungeon level transitions (stairs, gameplay plan
Phase 2) need a visual indicator that the world is changing. A linecrunch
wipe — the current floor "crunches" upward as if being consumed, then
the new floor is revealed from the bottom — is the C64 equivalent of a
modern screen transition.

**Specific application:**

```
Frame 1: Full dungeon display (21 rows)
Frame 2: Top 2 rows crunched away (19 rows visible, shifted down)
Frame 3: Top 5 rows crunched (16 rows visible)
Frame 4: Top 10 rows crunched (11 rows visible)
Frame 5: All rows crunched (blank screen)
Frame 6: New dungeon generated, reveal from top
Frame 7-10: Progressive reveal via reverse linecrunch
```

Total transition: ~10 frames = 200ms (PAL). Fast, dramatic, hardware-
accelerated.

**Alternative:** FLD achieves a similar effect by pushing the display
down. The visual difference: FLD pushes the whole display in one block
(like a curtain), while linecrunch can selectively remove rows (like a
dissolve). FLD is simpler; linecrunch is more flexible.

**Cost:** ~15 cycles per crunched line per frame. A full wipe (21 lines)
costs ~315 cycles per transition frame × 10 frames = ~3,150 cycles total.
The transition is a one-time event (not per-turn), so the cost is
irrelevant.

**Recommendation:** Implement when dungeon depth is added (gameplay plan
Phase 2). The FLD variant is simpler and achieves 80% of the visual
impact with less code.

---

## Recommended Implementation Order

Aligned with the port proposal's phasing:

### Phase 2 Additions (Custom Charset + Polish)

**Step 1: Basic raster interrupt chain**
- Set up VIC raster IRQ (replacing Kernal IRQ)
- 3-zone color split: game area / status bar / messages
- SID player callback on each frame
- **Unlocks all subsequent techniques**

**Step 2: Top/bottom border removal**
- Single register write per frame
- Black border with no content initially
- Immediate visual improvement

**Step 3: Color RAM atmospheric gradient**
- Torchlight vertical gradient centered on player row
- Damage flash (border → red for 2 frames)
- Low-HP border pulse

**Step 4: Per-zone charset switching**
- Dungeon tileset for game area (rows 0-21)
- UI font for status bar (row 22)
- Text font for messages (rows 23-24)

**Step 5: Animated charset tiles**
- Water/lava animation (8 bytes/frame)
- Torch flicker in rooms
- Stairs glow/pulse

### Phase 2 Additions (Entity Enhancement)

**Step 6: Player sprite**
- Hardware sprite 0 for the player
- Idle animation (2-4 frames)
- Smooth inter-tile movement (4-frame glide)
- Sprite-over-character mode (dungeon shows through)

### v1.1 Additions (Scrolling + Effects)

**Step 7: Software scroll + hardware fine scroll**
- 64x48 map with 40x21 viewport
- Software copy for coarse scroll
- `$D011`/`$D016` for sub-tile smoothing
- FLD-based HUD pinning (status bar doesn't scroll)

**Step 8: FLD/Linecrunch level transitions**
- Stair descent wipe animation
- 10-frame transition (~200ms)

**Step 9: Tech-tech for status effects (optional)**
- Confusion sine wave
- Magic vision pulse
- Damage jolt

### v2 Luxury

**Step 10: FLI title screen (optional)**
- Hand-painted dungeon scene
- ~17 KB image data

---

## Cycle Budget Impact

Current POC frame budget (no raster effects):

```
Available per frame (PAL):              ~19,656 cycles
FOV computation:                        ~7,500 cycles
Full redraw (880 cells × ~20 cycles):   ~17,600 cycles
AI + combat:                            ~2,000 cycles
Total:                                  ~27,100 cycles (exceeds one frame!)
```

The POC actually spans ~1.4 frames per turn — acceptable for turn-based
because there's no animation between turns. The player doesn't notice
because input is blocking.

With recommended techniques, the **continuous background cost** (running
every frame regardless of player action) is:

```
Raster interrupt chain:                 ~200 cycles (3-zone split)
  - Background color changes:           ~110 cycles (22 lines × 5 cyc)
  - Border removal:                     ~20 cycles (2 register writes)
  - SID player callback:               ~300 cycles (typical music driver)
  - Charset zone switching:            ~30 cycles (2 switches)
Total continuous overhead:              ~660 cycles/frame (~3.4%)
```

With dirty-rect rendering (proposal §6.8), the per-turn render drops from
~17,600 to ~500 cycles. Combined with raster effects:

```
Available per frame:                    ~19,656 cycles
Continuous raster overhead:                ~660 cycles
Remaining for game logic:               ~18,996 cycles/frame
Per-turn costs (on player action):
  FOV:                                  ~7,500 cycles
  Dirty-rect render:                      ~500 cycles
  AI + combat:                          ~2,000 cycles
  Total per-turn:                       ~10,000 cycles (0.5 frames)
```

Plenty of headroom for sprite animation, smooth scrolling, and gameplay
features.

---

## What NOT to Do

Some demo techniques are counterproductive for a roguelike:

1. **Side border removal.** Costs significant CPU per line and requires
   cycle-exact timing on every raster line. The visual benefit (showing
   content in the narrow side borders) is minimal for a character-mode
   game. Skip.

2. **Sprite crunch.** Exploits a hardware glitch for variable-height
   sprites. Fragile, hard to debug, and unnecessary when the 8 sprites
   are sufficient without multiplexing.

3. **VSP.** The DRAM corruption risk is unacceptable for a game that
   maintains persistent state. Software scrolling is fast enough for
   turn-based movement.

4. **IFLI/interlacing.** Flicker is unpleasant in a game where the player
   stares at the screen for extended periods. If high-quality images are
   desired, use NUFLI (flicker-free) or skip FLI entirely.

5. **ECM invalid mode.** Blanks the screen to black. No use case.

---

## Key Insight: Demo Tricks Serve Different Goals in Games

Demo coders optimize for **visual spectacle at any CPU cost** — an effect
that consumes 90% of the frame budget is fine if it looks incredible for
30 seconds. Game developers optimize for **sustained atmosphere within a
CPU budget** — effects must run continuously, coexist with game logic, and
never interfere with gameplay responsiveness.

The techniques recommended above are specifically chosen for their
**cost-to-atmosphere ratio** in a turn-based context:

| Technique | CPU cost | Atmosphere gain | Runs continuously? |
|-----------|----------|-----------------|-------------------|
| Raster color gradient | ~110 cyc/frame | High (torchlight) | Yes |
| Border removal | ~20 cyc/frame | Medium (polish) | Yes |
| Charset animation | ~40 cyc/frame | Medium (life) | Yes |
| Player sprite | ~126 cyc/frame | High (presence) | Yes |
| Damage flash | ~10 cyc (2 frames) | High (feedback) | On hit only |
| Level transition | ~3,150 cyc total | High (drama) | On stairs only |
| Tech-tech | ~1,760 cyc/frame | Medium (magic) | During effect |
| **Total continuous** | **~296 cyc/frame** | | |

The continuous background cost of the top 4 techniques is under 300
cycles/frame — **1.5% of the frame budget** — for a dramatic improvement
in atmosphere and visual quality.
