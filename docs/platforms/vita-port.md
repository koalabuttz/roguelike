# PS Vita Port

> **Status:** Phase 1 complete (scaffold — vita2d renderer, compact-tier game loop, static render). No input yet. See issues #318–#322 for Phase 2–6 roadmap.

How to bring the roguelike to the PlayStation Vita while keeping all game logic in `roguelike-core`, exploiting Vita hardware for features that genuinely benefit the game, and fitting cleanly into the workspace alongside existing and planned ports.

## Hardware Summary

| Resource | Spec | Relevance |
|----------|------|-----------|
| CPU | ARM Cortex-A9 quad-core @ 444 MHz | Vastly exceeds game requirements. One core runs the game; others available for audio, networking, background tasks. |
| RAM | 512 MB total (typically ~256 MB available to homebrew) | Enormous. No memory pressure. `std` collections, full `GameState`, generous caches. |
| Display | 5" 960x544 OLED, 16.7M colors | The Vita's crown jewel. Deep blacks, vivid colors, perfect for a dungeon crawler with dynamic lighting. |
| GPU | PowerVR SGX543MP4+ | vita2d library provides simple 2D rendering. Hardware-accelerated sprite and texture drawing. |
| Input | D-pad, 2 analog sticks, 4 face buttons, L/R triggers, Start/Select | Richest input of any target platform. 8-direction movement without modifiers. |
| Front touchscreen | 5" capacitive multi-touch | Tap-to-move, tap-to-examine, drag-to-scroll message history. |
| Rear touchpad | Capacitive multi-touch | Secondary input for less common actions (zoom, inventory shortcuts). |
| Accelerometer / Gyroscope | 3-axis each | Novelty input — ambient torchlight flicker driven by physical tilt. |
| Storage | Memory card (proprietary) or internal (Vita Slim) | vita-sdk save data API. JSON serialization is fine. |
| Audio | Stereo speakers + headphone jack, hardware audio decoder | Full PCM/WAV playback via vita-sdk audio API. Far beyond PSG — can do procedural audio, multi-channel mixing, spatial effects. |
| WiFi | 802.11b/g/n | Enables network features: AT Protocol spectating, seed sharing, leaderboards. |
| Battery | 2210 mAh Li-ion | Suspend/resume support critical for portable play. |

## Relationship to Existing Docs

This proposal covers the **Vita frontend crate** — rendering, input, save, audio, and Vita-specific features. It does not redesign game logic. Like the GBA port, it consumes systems defined in other docs:

| Doc | What Vita uses from it |
|-----|----------------------|
| [cross-platform.md](../architecture/cross-platform.md) | Crate structure, `Renderer`/`InputSource` traits, `GameColor` mapping |
| [simulation.md](../architecture/simulation.md) | `SimBudget` caps (generous on Vita: 512 entities, 1000+ CA tiles/turn) |
| [acoustic-propagation.md](../design/acoustic-propagation.md) | `SoundEvent` → real audio rendering. Vita is the first Rust-based platform with hardware capable of doing this justice. |
| [gameplay-implementation-plan.md](../design/gameplay-implementation-plan.md) | Items, stairs, enchantment, mood — Vita renders these, doesn't change their logic |
| [gba-port.md](gba-port.md) | Establishes patterns for constrained Rust ports. Vita deliberately diverges where its hardware allows. |
| [c64-port-proposal.md](c64-port-proposal.md) | Defines server-side endpoints (leaderboards, daily seeds, cloud saves, spectation relay) that the Vita consumes over WiFi. Also defines the cross-platform save portability problem (C64's 1.6 KB binary vs Vita/desktop JSON) that atproto must solve. |
| [c64-atproto-bridge.md](c64-atproto-bridge.md) | Atproto lexicon design for saves and spectation. The Vita uses the same lexicons directly (no bridge needed — it has native HTTPS/OAuth). |

Where those docs specify Vita-relevant details, this doc references rather than duplicates. Where Vita needs something unique (OLED-optimized rendering, touch input, analog stick movement, spatial audio), this doc defines it.

## Architecture

### Crate Structure

```
crates/vita/
├── Cargo.toml          depends on roguelike-core + roguelike-saves
├── src/
│   ├── main.rs         Entry point: vita-sdk initialization, main loop
│   ├── render.rs       VitaRenderer: Renderer trait impl using vita2d
│   ├── input.rs        VitaInput: InputSource trait impl (controls + touch)
│   ├── touch.rs        Touch-to-GameCommand translation (front + rear)
│   ├── audio.rs        SoundEvent → PCM rendering, spatial mixing
│   ├── saves.rs        SaveBackend impl using vita-sdk save data API
│   ├── suspend.rs      Suspend/resume lifecycle handling
│   └── assets/
│       ├── font.rs     Embedded bitmap font(s) for dungeon glyphs
│       └── palette.rs  OLED-optimized color palettes
```

### What Does NOT Change in Core

This is the key advantage of the Vita as a port target: **core requires zero modifications.**

Unlike the GBA port, which runs at tier compact with `no_std`, `i16` coords, and fixed-size containers, the Vita:

- Has a full operating system with `std` support via vitasdk's newlib
- Uses tier standard's `i32` types (`Coord`, `Stat`, `Pos`) natively. For micro-tier cross-platform seeds, the Vita uses `core::tier_micro` to generate identical dungeons to the C64 and GBA
- Has enough RAM for `Vec`, `String`, `HashSet` — all standard collections
- Can use `serde_json` for saves — the `SaveBackend` trait works as-is
- Implements the `GameStep` trait, which lets `FrameSink` and the game loop work with any tier's game state (`FrameSink::write_frame` takes `&dyn GameStep` instead of `&GameState`)

The Vita port is a **pure frontend addition**: implement `Renderer`, `InputSource`, and `SaveBackend`, write the game loop glue, and the entire game runs unchanged.

### Dependency Graph

```
roguelike-core ←── roguelike-saves ←── roguelike-vita
                                            │
                                            ├── vitasdk (C FFI bindings)
                                            ├── vita2d-sys (2D rendering)
                                            └── (optional) rodio or custom PCM for audio
```

The Vita crate depends on `roguelike-core` and `roguelike-saves`. It does **not** depend on `roguelike-tui` — the TUI crate is crossterm-specific, and the Vita has its own rendering path. This matches the pattern: TUI is shared by terminal-based frontends; non-terminal frontends (GBA, Vita, Web) each have their own renderer.

### Cross-Compilation

The Vita homebrew toolchain targets `armv7-vita-eabihf` (a custom Rust tier-3 target). The vitasdk provides a GCC-based C toolchain that Rust links against via `cc` and `bindgen`.

Build command:
```bash
cargo build --target armv7-vita-eabihf -p roguelike-vita
```

The resulting ELF is converted to a `.vpk` package (Vita's installable format) using `vita-mksfoex` and `vita-pack-vpk` from the vitasdk toolchain.

## Rendering

### The Renderer Trait on Vita

The core `Renderer` trait operates in character-cell units (`draw_char`, `draw_str`). On a terminal, one character = one cell. On the Vita's 960x544 pixel display, each character cell is rendered as a bitmap glyph at a configurable pixel size.

```rust
pub struct VitaRenderer {
    /// Pixel dimensions of each character cell.
    cell_width: u32,   // Default: 24px → 40 columns
    cell_height: u32,  // Default: 24px → 22 rows
    /// Active color palette (normal, high-contrast, colorblind variants).
    palette: ColorPalette,
    /// Pre-rendered glyph atlas loaded into GPU texture.
    glyph_atlas: vita2d_texture,
    /// Back buffer for dirty-rect optimization.
    prev_frame: Vec<CellState>,
}
```

#### Cell Sizing and Display Layout

At 24x24 pixel cells: 40 columns x 22 rows — close to the terminal's default 80x24 but scaled for readability on the Vita's 5" screen. The map viewport, status bar, and message area use the same character-cell layout as the terminal version.

Alternative sizes for user preference:
| Cell size | Grid | Feel |
|-----------|------|------|
| 16x16 | 60x34 | Dense, more map visible, small text |
| 20x20 | 48x27 | Balanced |
| 24x24 | 40x22 | Default — readable at arm's length |
| 32x32 | 30x17 | Large, comfortable, less map visible |

The cell size is a runtime setting stored in `Settings`, not a compile-time constant. The glyph atlas is regenerated when the setting changes.

#### Glyph Atlas

Rather than drawing each character with individual draw calls, pre-render all printable ASCII characters (32–126) plus game-specific glyphs into a single GPU texture atlas. Each `draw_char` call becomes a single textured quad blit from the atlas — fully GPU-accelerated via vita2d.

The atlas is generated at startup (and regenerated on cell-size change) from an embedded bitmap font. A high-quality bitmap font like Terminus or a hand-crafted roguelike font provides crisp rendering at the cell sizes above.

#### `GameColor` Mapping

```rust
fn to_vita_rgba(color: GameColor) -> u32 {
    match color {
        GameColor::Black    => 0x000000FF,
        GameColor::White    => 0xFFFFFFFF,
        GameColor::Grey     => 0xAAAAAAFF,
        GameColor::DarkGrey => 0x555555FF,
        GameColor::Red      => 0xFF4444FF,
        GameColor::DarkRed  => 0xAA0000FF,
        GameColor::Green    => 0x44FF44FF,
        GameColor::DarkGreen=> 0x00AA00FF,
        GameColor::Yellow   => 0xFFDD00FF,
        GameColor::DarkBlue => 0x0000AAFF,
        GameColor::Cyan     => 0x44FFFFFF,
        GameColor::Rgb(r,g,b) => ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | 0xFF,
    }
}
```

These values are OLED-tuned: the Vita's OLED panel has true black (#000000 emits zero light) and exceptional color saturation. The palette can push more vivid colors than a typical LCD terminal without looking garish.

#### Dirty-Rect Rendering

The `Renderer` trait calls `clear()` then redraws the entire screen each frame. On the Vita, this is wasteful — most cells don't change between turns. The `VitaRenderer` maintains a `prev_frame` buffer and only redraws cells that changed since the last `flush()`. This is an internal optimization invisible to core.

Turn-based gameplay means the screen typically changes once per player action. Between turns, the renderer sleeps, saving battery.

### OLED-Specific Visual Design

The Vita's OLED display has unique characteristics that the renderer should exploit:

1. **True black background.** Unexplored tiles and the screen border are literal black — the OLED pixels are off. This creates a natural vignette effect where the lit dungeon floats in absolute darkness. No LCD backlight bleed.

2. **High contrast FOV gradient.** The distance-based lighting described in the GBA port (palette-based brightness zones) translates directly but with far more color depth. Instead of 4 palette-indexed brightness levels, the Vita can smoothly interpolate brightness per-tile using full RGB values:

   ```rust
   fn apply_fov_dimming(base_color: u32, distance: u32, fov_radius: u32) -> u32 {
       let brightness = 1.0 - (distance as f32 / fov_radius as f32).powi(2);
       scale_rgb(base_color, brightness.max(0.15))
   }
   ```

   The quadratic falloff (`.powi(2)`) creates a visible bright center that falls off naturally toward the FOV edge, more convincing than linear stepping.

3. **Explored tile dimming.** Tiles the player has seen but can't currently see are rendered at ~15% brightness. On OLED, this is genuinely dim — not the "dark grey on LCD backlight" that terminal emulators produce.

4. **Color bleeding for mood.** When creature mood / escalation systems are implemented, the background tint can shift subtly (e.g., a faint red wash in high-tension areas). OLED color accuracy makes this perceptible at very low intensities where LCD panels would clip to black.

## Input

The Vita has the richest input hardware of any target platform. Every input method maps cleanly to `GameCommand` and `MenuCommand` without hacks or modifiers.

### Physical Controls

| Input | GameCommand | Notes |
|-------|-------------|-------|
| D-pad | `Move(Direction)` | 4 cardinal directions |
| D-pad diagonals | `Move(Direction)` | Hardware supports simultaneous Up+Right etc. — true 8-direction unlike GBA |
| Left analog stick | `Move(Direction)` | 8-direction with dead zone. Octagonal sector mapping. |
| Right analog stick | Look mode cursor | Free cursor movement without entering/exiting look mode — always available |
| Cross (X) | `Wait` | Confirm in menus (`MenuCommand::Select`) |
| Circle (O) | `Back` / cancel | `MenuCommand::Back` |
| Square | Auto-explore | |
| Triangle | Auto-fight | |
| L trigger | Autorun (last direction) | |
| R trigger | Message history | |
| L + R | Toggle map overlay | Full explored map (get_explored_map equivalent) |
| Start | Pause menu | |
| Select | Help screen | |

#### Left Analog Stick Mapping

The analog stick provides (x, y) in [-128, 127]. Map to 8-direction movement using angular sectors:

```rust
fn analog_to_direction(x: i8, y: i8) -> Option<Direction> {
    let magnitude = ((x as f32).powi(2) + (y as f32).powi(2)).sqrt();
    if magnitude < DEAD_ZONE { return None; }

    let angle = (y as f32).atan2(x as f32);
    // Divide into 8 sectors of PI/4 each
    let sector = ((angle + PI) / (PI / 4.0)) as usize % 8;
    Some(DIRECTION_TABLE[sector])  // [East, SouthEast, South, SouthWest, ...]
}
```

Unlike the GBA (which needs an L-button modifier for diagonals), the Vita's analog stick provides natural 8-direction input. This is the most comfortable input method for dungeon movement across all platforms.

A repeat timer fires after an initial delay (300ms) then at a steady rate (120ms) while the stick is held, enabling smooth corridor traversal without autorun.

### Touch Input

The front touchscreen provides tap-to-move gameplay. This is not a gimmick — it's a genuinely useful input mode that no other port can offer.

#### Front Touchscreen

| Gesture | Action |
|---------|--------|
| Tap on visible tile | `pathfind_to(x, y)` — walk to that tile |
| Tap on monster | `pathfind_to(monster.x, monster.y)` — walk to and attack |
| Tap on player | `Wait` |
| Long press on tile | Look mode — examine tile contents |
| Swipe up on message area | Scroll message history |
| Tap on status bar | Toggle detailed stats |

Touch coordinates are translated from pixel space to character-cell space using the cell size:

```rust
fn touch_to_cell(touch_x: u16, touch_y: u16) -> (Coord, Coord) {
    ((touch_x as Coord) / cell_width as Coord,
     (touch_y as Coord) / cell_height as Coord)
}
```

Then cell coordinates are translated to map coordinates using the viewport offset. The `pathfind_to` logic already exists in core — the touch handler simply produces the same `GameCommand::PathfindTo` that the MCP server uses.

#### Rear Touchpad

The rear touchpad is divided into quadrants for less-common actions:

| Quadrant | Action |
|----------|--------|
| Top-left | Toggle cell size (cycle through presets) |
| Top-right | Quick save |
| Bottom-left | Inventory (when items are implemented) |
| Bottom-right | Toggle message history overlay |

Rear touch is optional and can be disabled in settings. Some Vita owners find accidental rear touches annoying, so this must be explicitly opt-in.

## Save System

The Vita runs a full OS with a filesystem. The `SaveBackend` trait from `crates/saves` works directly:

```rust
impl SaveBackend for VitaSaves {
    fn save(&self, slot: usize, state: &GameState) -> Result<(), SaveError> {
        let json = serde_json::to_string(state)?;
        let path = format!("ux0:data/roguelike/saves/slot_{}.json", slot);
        std::fs::write(&path, json)?;
        Ok(())
    }

    fn load(&self, slot: usize) -> Result<GameState, SaveError> {
        let path = format!("ux0:data/roguelike/saves/slot_{}.json", slot);
        let json = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&json)?)
    }

    // ... list_slots, delete, metadata ...
}
```

Alternatively, use vita-sdk's `sceAppUtilSaveDataMount` / `SceAppUtilSaveDataSlotParam` API for proper Vita save data integration (save icons, descriptions visible in the system save manager). This is a polish decision — filesystem writes work first, save data API integration is a later enhancement.

JSON serialization is not a concern on Vita. A full `GameState` serializes to ~10–50 KB of JSON, negligible against the memory card's capacity. The same 5-slot casual / 1-slot classic save discipline applies.

### Suspend/Resume

Vita suspends to RAM when the user presses the power button or switches apps. The game must handle this gracefully:

1. **On suspend signal** (`SCE_KERNEL_POWER_TICK`): Auto-save current state to a dedicated "suspend" slot. This is transparent — not one of the user's save slots.
2. **On resume:** Detect the suspend save, restore state, delete the suspend save.
3. **On cold boot with a suspend save present:** Offer to resume or discard (handles the case where the Vita ran out of battery while suspended).

This is critical for portable play. The user should never lose progress because they put the Vita to sleep.

## Audio

The Vita is the first target platform with audio hardware capable of doing justice to the [acoustic propagation system](../design/acoustic-propagation.md). While the terminal renders sound events as message text and the GBA uses 4-channel PSG, the Vita can produce rich, spatialized procedural audio.

### Audio Architecture

```rust
pub struct VitaAudio {
    /// Active sound channels (mixed into stereo output).
    channels: [AudioChannel; 8],
    /// Ambient loop state.
    ambient: AmbientState,
    /// Master volume (0.0–1.0).
    master_volume: f32,
}

struct AudioChannel {
    /// PCM sample buffer.
    sample: Option<&'static [i16]>,
    /// Playback position within the sample.
    position: usize,
    /// Volume (0.0–1.0), derived from SoundEvent intensity.
    volume: f32,
    /// Stereo pan (-1.0 left, 0.0 center, 1.0 right).
    pan: f32,
}
```

### SoundEvent Rendering

Each `SoundEvent` from core maps to a short PCM sample with spatial positioning:

| SoundKind | Sample | Duration | Notes |
|-----------|--------|----------|-------|
| `Combat` | Metallic clash | ~100ms | Pitched slightly by intensity |
| `Footstep` | Soft tap | ~50ms | Alternating L/R samples for walking rhythm |
| `Run` | Heavier footstep | ~60ms | Faster cadence |
| `Death` | Low thud | ~200ms | |
| `MonsterAlert` | Growl/hiss | ~150ms | Varies by monster type |
| `DoorOpen` (future) | Creak | ~200ms | |
| `ItemPickup` (future) | Chime | ~100ms | |

Samples are short procedurally-generated waveforms (not recorded audio), keeping the binary small and matching the game's abstract aesthetic. This parallels the [C64's SID approach](c64-port-proposal.md#611-sound-design) — both platforms use procedural audio driven by `SoundKind`, but with different synthesis methods (PCM samples on Vita, SID register writes on C64).

### Spatial Audio

The Vita has stereo speakers and headphone output. Sound events carry a source position. The audio system calculates pan and volume from the relative position of the sound source to the player:

```rust
fn spatialize(player: Pos, source: Pos, intensity: u8) -> (f32, f32) {
    let dx = (source.0 - player.0) as f32;
    let dy = (source.1 - player.1) as f32;
    let distance = (dx * dx + dy * dy).sqrt();

    // Pan: -1.0 (left) to 1.0 (right), based on horizontal offset
    let pan = (dx / distance.max(1.0)).clamp(-1.0, 1.0);

    // Volume: attenuate by distance, scaled by intensity
    let volume = (intensity as f32 / 255.0) * (1.0 - distance / FOV_RADIUS as f32).max(0.0);

    (pan, volume)
}
```

With headphones, the player can hear a monster approaching from the left before it enters the FOV. This is a mechanical advantage — sound becomes a genuine second sense, exactly as the acoustic propagation doc envisions.

### Ambient Audio

A low ambient drone plays continuously, modulated by game state:

- **Tension level** (from the escalation system): Drone pitch and volume rise as tension increases.
- **Room size:** Larger rooms get subtle reverb (longer decay). Corridors are drier.
- **Depth** (when stairs are implemented): Deeper floors get lower-pitched ambience.

The ambient system uses a simple wavetable oscillator mixed into the output at low volume. It adds atmosphere without consuming significant CPU.

## Memory Budget

The Vita's 256 MB available RAM makes memory budgeting a formality rather than a constraint. For documentation completeness:

| Data | Size | Notes |
|------|------|-------|
| `GameState` (full) | ~50–200 KB | Standard `Vec`/`HashSet` collections, JSON-serializable |
| Glyph atlas texture | ~256 KB | 96 glyphs × 4 cell sizes × RGBA, GPU memory |
| Audio samples | ~100 KB | Short PCM samples for all sound events |
| prev_frame buffer | ~10 KB | 40×22 cells × 12 bytes/cell |
| Audio mixing buffers | ~32 KB | 8 channels × 4 KB each |
| **Total** | **~650 KB** | **<0.3% of available RAM** |

Memory is a non-issue. The Vita port does not need fixed-size containers, heap-size caps, or any memory-conscious compromises. This is a feature, not a deficiency — it means the Vita port can use idiomatic Rust with zero `#[cfg]` workarounds.

## Implementation Phases

Each phase produces a testable artifact. Phases 1–3 produce a fully playable Vita build.

### Phase 1: Scaffold and Static Rendering (Effort: M)

**Goal:** Vita binary boots, renders the dungeon, player is visible.

- Set up `crates/vita/` with vitasdk cross-compilation.
- Implement `VitaRenderer` using vita2d: glyph atlas generation, `draw_char`/`draw_str` as texture blits, `GameColor` mapping.
- Render the full map at 24x24 cell size.
- Display HP bar and message area.
- **Test:** Build with `cargo build --target armv7-vita-eabihf -p roguelike-vita`. Install `.vpk` on Vita (or Vita3K emulator). Verify the dungeon renders.

**Depends on:** Nothing. Cross-compilation toolchain setup is the prerequisite.

### Phase 2: Input and Game Loop (Effort: S-M)

**Goal:** Playable turn-based game — move, attack, die.

- Implement `VitaInput`: D-pad, analog stick, face buttons → `GameCommand`.
- Wire up the game loop: poll input → `GameState::step()` → re-render.
- Handle game over (death screen, restart).
- Implement basic save/load via filesystem writes.
- **Test:** Full gameplay loop on hardware/emulator. Navigate dungeon, fight monsters.

**Depends on:** Phase 1.

### Phase 3: Touch Input + Save System (Effort: M)

**Goal:** Touch-to-move works. Saves persist. Suspend/resume is safe.

- Implement front touchscreen: tap-to-pathfind, long-press-to-examine.
- Implement rear touchpad quadrant actions (optional, off by default).
- Upgrade saves to use vita-sdk save data API (icons, system save manager integration).
- Implement suspend/resume auto-save.
- **Test:** Tap to move, suspend mid-game, resume. Verify state integrity.

**Depends on:** Phase 2.

### Phase 4: FOV Lighting + Visual Polish (Effort: S-M)

**Goal:** The dungeon looks alive. OLED exploited.

- Implement quadratic FOV brightness falloff per-tile.
- Implement explored-tile dimming at 15% brightness.
- Add optional torchlight flicker (subtle random brightness variation at FOV boundary).
- Implement cell-size selection in settings menu.
- Implement dirty-rect rendering optimization.
- **Test:** Visual inspection. FOV gradient is smooth, unexplored tiles are true black, cell size changes are smooth.

**Depends on:** Phase 1 (rendering must work).

### Phase 5: Audio (Effort: M-L)

**Goal:** Sound events render as spatial audio. Ambient drone plays.

- Implement PCM audio output via vita-sdk audio API.
- Generate procedural sound samples for core `SoundKind` variants.
- Implement spatial panning and distance attenuation.
- Implement ambient drone with tension-based modulation.
- Volume control in settings.
- **Test:** Combat sounds come from the direction of the monster. Ambient drone shifts with tension. Audio plays through speakers and headphones.

**Depends on:** Phase 2 (needs a playable game generating sound events). Independent of Phases 3–4.

### Phase 6: Network Features (Effort: M)

**Goal:** WiFi-enabled features for connected play.

The [C64 port proposal](c64-port-proposal.md#613-networking-ultimate-64--uii) defines server-side endpoints for leaderboards, daily seeds, cloud saves, and spectation relays — designed to serve both C64 (via UII+ Ethernet) and other clients. The Vita consumes these same endpoints over WiFi, with the advantage of native HTTPS support (no bridge needed, unlike the C64's binary TCP protocol through the UII+).

- Implement seed code sharing (copy to clipboard / display as QR code on screen).
- Leaderboard submission on death (same endpoint as C64: `POST /api/leaderboard`). The Vita submits with `"platform": "vita"`. Cross-platform leaderboards show C64, terminal, SSH, and Vita players together.
- Daily challenge seed fetch (`GET /api/daily-seed`). Same shared seed across all platforms — a C64 player and a Vita player explore the same dungeon on the same day. The [C64 doc's fairness question](c64-port-proposal.md#12-decisions-and-remaining-questions) (Q8) about map size parity applies: Vita uses desktop-sized maps (80x40) while C64 uses 40x22. Daily challenges should specify map dimensions in the seed code to ensure parity.
- (Future, depends on atproto) AT Protocol integration for spectating and PDS saves. The Vita can use the atproto lexicons directly via native HTTPS/XRPC — the same lexicons the C64 accesses through its bridge. A game saved from a Vita appears identically in a player's PDS to one saved from the SSH client.
- **Test:** Seed codes are shareable. Leaderboard submissions work. Network features degrade gracefully when WiFi is off (same principle as the C64's [graceful UII+ absence detection](c64-port-proposal.md#613-networking-ultimate-64--uii)).

**Depends on:** Phase 2. Server endpoints depend on the leaderboard/daily-seed service being deployed (shared with C64). AT Protocol integration depends on the `atproto` crate being implemented.

## Features That Would Be Genuinely Impressive on Vita

The Vita has a reputation as an indie gaming powerhouse that was commercially underserved. A well-executed roguelike that leans into Vita-specific hardware would stand out in the homebrew scene. These features go beyond "it runs on Vita" into "this is better *because* it's on Vita."

### 1. OLED Darkness as a Mechanic

No other display technology renders darkness like OLED. When the Vita's pixels are off, they are *off* — absolute zero-luminance black. This transforms the FOV system from "grey tiles around a lit area" (which is what every LCD and terminal produces) into a physical experience where the dungeon is a pool of light surrounded by genuine void.

The quadratic brightness falloff described in the rendering section means tiles at the FOV edge barely glow, fading smoothly into the abyss. Explored-but-not-visible tiles appear as ghostly 15% brightness memories. The effect is that the player instinctively understands their vulnerability — the darkness isn't an abstraction, it's visually oppressive.

This is not achievable on any other platform in the project. Terminal emulators have backlight bleed. The GBA's LCD cannot produce true black. Even modern desktop monitors rarely match OLED contrast. The Vita version of the dungeon would look and *feel* fundamentally different.

### 2. Spatial Audio Through Headphones — Sound as a Real Second Sense

The acoustic propagation system is designed as a game mechanic, not decoration. On the terminal, it manifests as message text ("You hear footsteps to the east"). On the GBA, it's PSG beeps. On the Vita with headphones, it becomes **actual spatial audio** — the player physically hears a monster approaching from the left channel before it enters the FOV.

This is the acoustic propagation doc's vision fully realized. The player develops a genuine dual-sense awareness: eyes on the map, ears tracking threats through walls. A growl from the right headphone channel teaches the player to check east. Combat sounds from two rooms away create urgency without any on-screen indicator. The message log becomes a fallback, not the primary sound channel.

Combined with the OLED darkness, this creates a uniquely immersive experience: a dark screen punctuated by a small pool of light, with sounds arriving from the void. This would be remarkable for a roguelike on any platform, and the Vita's portable form factor (headphones in a dark room) is the ideal context.

### 3. Right-Stick Free-Look While Playing

Most roguelikes have a look mode that you enter, move a cursor, then exit. The Vita's right analog stick enables **simultaneous look-while-playing**: the right stick controls a look cursor at all times, showing tile descriptions in a floating tooltip, while the left stick/D-pad controls movement. The player never pauses to examine — information flows continuously.

This is a UI interaction that no keyboard-based, D-pad-only, or touchscreen-only platform can replicate. It requires two independent analog inputs. The Vita is the only target platform in the project that has them.

### 4. Touch-to-Pathfind as Primary Navigation

Tapping a visible tile to walk there (using core's existing A* pathfinding) turns the Vita into a point-and-tap dungeon explorer. Long-press to examine. Tap a monster to pathfind-and-attack. This leverages the front touchscreen as a first-class input mode, not a cursor emulator.

The interaction is immediate and intuitive in a way that keyboard movement and even analog stick input aren't. Casual players who find traditional roguelike controls intimidating can play entirely by touch. Combined with auto-explore (Square button), the game becomes approachable without losing depth for players who prefer physical controls.

### 5. Portable Suspend-Anywhere

The Vita is a portable console. Players pick it up for 5 minutes on a bus, then put it away. The transparent suspend/resume system (auto-save on power button press, auto-restore on wake) means the game is always exactly where the player left it. No save points, no save menus, no lost progress.

This seems simple, but it's the difference between a portable game and a desktop game running on a portable device. The save discipline system (classic/casual modes) still applies to manual saves — suspend is orthogonal and always available.

### 6. Tension-Reactive Ambient Audio

The ambient drone system, modulated by the escalation system's tension value, creates an audio environment that responds to how the player is doing. Low tension: quiet, atmospheric hum. High tension (many monsters alerted, low HP): the drone rises in pitch and volume, the audio mix shifts. The player feels danger before they see it.

This is possible because the Vita has real audio hardware with sufficient channels and mixing capability. The GBA's 4-channel PSG can approximate it. The terminal has no audio at all. The Vita version is the definitive audio experience.

## Open Questions

1. **vitasdk Rust support maturity.** The `vitasdk` toolchain is primarily C-based. Rust cross-compilation to `armv7-vita-eabihf` requires a custom target spec and linking against vitasdk's newlib. The [vita-rust](https://github.com/vita-rust) project provides tooling, but it's community-maintained. Evaluate current state before committing.

2. **vita2d vs raw GPU.** `vita2d` is vitasdk's simple 2D rendering library. It's convenient but may not expose all GPU features needed for effects like per-tile alpha blending. For Phase 1–4, vita2d is sufficient. Phase 5+ effects (ambient color shifting, smooth alpha transitions) may need lower-level `sceGxm` calls.

3. **Emulator testing in CI.** Vita3K is an open-source Vita emulator with growing compatibility. Evaluate whether it can run the game headlessly for CI (similar to mGBA for the GBA port). If not, CI is limited to cross-compilation success; gameplay testing requires hardware.

4. **Font selection.** Bitmap fonts need to look good at multiple cell sizes on a 960x544 screen. At 16x16, each glyph has 256 pixels — enough for clean rendering. At 32x32, glyphs have 1024 pixels but the grid only shows 30x17 tiles. Finding or creating a font that works well across this range is a design task.

5. **Right-stick look mode and turn consumption.** The right-stick free-look described in the impressive features section raises a question: does moving the look cursor consume a turn? It should not (it's purely informational), but core's `InputSource::next_command()` is blocking. The Vita input layer needs to handle right-stick input internally (updating the tooltip) without forwarding it to `next_command()`. This is a frontend concern — core doesn't know about the right stick.

6. **Cross-platform leaderboard fairness.** The [C64 proposal raises this question](c64-port-proposal.md#12-decisions-and-remaining-questions) (Q8): C64 uses 40x22 maps, desktop/Vita use 80x40. Should daily challenge leaderboards be per-platform or require a shared map size? The Vita can generate maps at any size, so it could use C64-sized maps for daily challenges and full-sized maps for freeplay. This is a design decision that affects all networked ports.

7. **Cross-platform save portability.** The C64's binary save format (~1.6 KB) and the Vita's JSON format (~10-50 KB) represent the same game state in incompatible serializations. The [C64 proposal](c64-port-proposal.md#12-decisions-and-remaining-questions) (Q12) and the [C64 bridge doc](c64-atproto-bridge.md) discuss this at length. If atproto PDS saves are schema-compatible at the lexicon level, a player could in theory continue a C64 game on a Vita (or vice versa) — but this requires a conversion layer. Is this a goal for v1, or aspirational?

## Testing Strategy

### Hardware Testing

Primary development and testing uses a Vita with HENkaku/Enso (homebrew-enabled firmware). Transfer `.vpk` via USB, FTP, or vitashell.

### Emulator Testing

Vita3K provides desktop testing without hardware. Check compatibility for:
- vita2d rendering
- Touch input emulation (mouse → touch)
- Audio output
- Save data API

### Cross-Compilation CI

```yaml
- name: Build Vita
  run: |
    # Install vitasdk toolchain (cached)
    cargo build --target armv7-vita-eabihf -p roguelike-vita

- name: Test core (standard features, Vita-relevant)
  run: cargo test -p roguelike-core
```

Since Vita uses the default feature set (no `gba` flag, no `no_std`), all existing core tests apply unmodified. The Vita crate itself gets unit tests for input mapping, color conversion, and cell coordinate math.

## Relationship to Other Ports

| Concern | Terminal | SSH | MCP | GBA | **Vita** | C64 | Web |
|---------|----------|-----|-----|-----|----------|-----|-----|
| `std` available | Yes | Yes | Yes | No | **Yes** | No (`no_std` Rust) | Yes |
| `SaveBackend` | Yes | Yes | N/A | No (SRAM) | **Yes** | No (binary) | Yes |
| `no_std` needed | No | No | No | Yes | **No** | Yes (standalone `no_std`) | No |
| Type sizing / tier | tier standard (i32) | tier standard (i32) | tier standard (i32) | tier compact (i16) | **tier standard (i32)** | tier micro (u8) | tier standard (i32) |
| Renderer type | crossterm | crossterm | JSON | tiles+OAM | **vita2d** | PETSCII+custom charset | canvas |
| Audio capable | No | No | No | PSG (4ch) | **Full PCM** | SID (3ch+filter) | Web Audio |
| Touch input | No | No | No | No | **Yes (front+rear)** | No | Yes |
| Analog sticks | Gamepad* | No | No | No | **Native (dual)** | Joystick (1-axis) | Gamepad* |
| Network capable | N/A | SSH | MCP | No | **WiFi (native)** | UII+ Ethernet | Yes |
| Leaderboard/daily | No | No | No | No | **Yes (HTTP)** | Yes (UII+ HTTP) | Yes |
| Atproto method | Direct | Direct | N/A | No | **Direct (HTTPS)** | Via bridge | Direct |
| Save format | JSON | JSON | In-memory | postcard/SRAM | **JSON** | Binary (~1.6 KB) | JSON |

*via optional gilrs/browser gamepad API

The Vita port sits in a sweet spot: it has the hardware richness to be the most feature-complete port, while requiring zero core modifications. It doesn't block or depend on any other port's work. The GBA's tier compact constraints, the C64's tier micro constraints, the Web's async constraints — none of these affect the Vita.

### Shared Server Infrastructure

The [C64 port proposal](c64-port-proposal.md) defines server-side endpoints that serve multiple clients:

| Endpoint | C64 access | Vita access |
|----------|-----------|-------------|
| `POST /api/leaderboard` | UII+ HTTP, binary TCP | Native HTTPS |
| `GET /api/daily-seed` | UII+ HTTP | Native HTTPS |
| `PUT/GET /api/saves/{id}` | UII+ HTTP or atproto bridge | `SaveBackend` or native atproto |
| Spectation relay | UII+ TCP binary frames | Native TCP/atproto |
| MCP server | UII+ HTTP (client mode) | N/A (Vita runs the game locally) |

The Vita's advantage: it speaks native HTTPS and can use atproto directly (OAuth + XRPC), while the C64 needs a bridge server for anything beyond raw HTTP. The server endpoints are the same — the C64's UII+ and the Vita's WiFi are just different transports to shared infrastructure.

### The C64 Contrast

The C64 port is architecturally unique among all ports: it targets the MOS 6502 via [rust-mos](https://github.com/mrk-its/rust-mos), with extreme constraints (64 KB RAM, 1 MHz CPU). The C64 runs at tier micro (`u8` coordinates, `u8` stats, fixed-size arrays, LFSR-16 RNG, iterative shadowcasting FOV) via `core::tier_micro`, depending on `roguelike-core` directly as a thin frontend just like every other port. The Vita port is at the opposite extreme of hardware capability — it runs at tier standard, reusing `roguelike-core` entirely unchanged, with all effort going into the frontend.

This distinction matters for save compatibility. The C64's binary save format (~1.6 KB, see [C64 doc section 3.10](c64-port-proposal.md#612-save-system)) is structurally different from the JSON `GameState` serialization used by the Vita and all other Rust-based ports. Cross-platform save portability between C64 and Vita/desktop requires format conversion — either in the atproto bridge (as the [C64 bridge doc](c64-atproto-bridge.md) proposes) or as a server-side translation layer. The Vita doesn't need to solve this problem directly, but its atproto integration should use the same lexicons (`save.gameState`, `save.settings`) so that saves are at least schema-compatible at the PDS level.

**"Don't close doors" notes:**

- The touch-to-pathfind input mode should emit standard `GameCommand::PathfindTo` commands (or the moral equivalent). If this command doesn't exist yet, add it to the `GameCommand` enum in core — it benefits the Web port (click-to-move) and any future touchscreen platform equally.
- The spatial audio system's `spatialize()` function takes abstract `Pos` coordinates and returns pan/volume. If this logic proves useful across platforms (Web Audio, desktop speakers, and potentially as a reference for the C64's SID distance attenuation), extract it to a shared utility in core or a new `audio` crate. But don't extract prematurely — let the Vita implementation prove the interface first.
- The cell-size selection UI generalizes to any pixel-based renderer (Web canvas, GBA tile sizing, future tileset support on desktop). The setting should live in core's `Settings` struct if it doesn't already have a display-scale concept.
- The suspend/resume pattern (auto-save to a hidden slot on lifecycle events) applies to mobile/web ports equally. Document it as a pattern, don't hard-code it as Vita-specific.
- The leaderboard submission and daily seed fetch are intentionally thin HTTP calls (not platform-specific logic). Any platform with network access — Vita (WiFi), C64 (UII+), Web (fetch API), desktop (reqwest) — can consume them. The server endpoints defined in the [C64 proposal](c64-port-proposal.md#613-networking-ultimate-64--uii) should be documented as a shared protocol, not a C64-specific feature.

## Effort Estimate

| Phase | Effort | Cumulative |
|-------|--------|------------|
| Phase 1: Scaffold + rendering | M | M |
| Phase 2: Input + game loop | S-M | M-L |
| Phase 3: Touch + saves + suspend | M | L |
| Phase 4: FOV lighting + polish | S-M | L |
| Phase 5: Audio | M-L | L-XL |
| Phase 6: Network | M | XL |

**Total to playable (Phases 1–3): L (week+).** The bulk is cross-compilation toolchain setup and vita2d integration. Once rendering and input work, the game logic runs unchanged.

**Total to impressive (Phases 1–5): L-XL.** Audio is the largest optional phase but delivers the highest "wow factor" on Vita hardware.

The Vita port is significantly less effort than the GBA port (no tier compact constraints, no `no_std`, no fixed-size containers) while delivering a richer end product. This is the most favorable effort-to-impressiveness ratio of any planned port.
