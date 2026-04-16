# Android Port

> **Status:** Planning. No code yet.

How to bring the roguelike to Android as a native APK using Rust, keeping all game logic in `roguelike-core` and following the same frontend-crate pattern as GBA, NDS, Vita, and C64.

## Stack

| Layer | Choice | Version | Why |
|-------|--------|---------|-----|
| Activity glue | `android-activity` (NativeActivity) | 0.6 | No Java/Kotlin needed. Games don't need on-screen keyboard. |
| Windowing | `winit` with `android-native-activity` feature | 0.30 | Cross-platform event loop, touch events, lifecycle (Resumed/Suspended). |
| Rendering | `softbuffer` | 0.4 | Raw `&mut [u32]` pixel buffer. Same pattern as Vita/NDS software renderers. |
| Font | `roguelike-renderer3d::font` | in-tree | 8x8 bitmap font already exists. Blit glyphs into pixel buffer. |
| Build tool | `cargo-apk2` | 1.3 | Maintained fork of cargo-apk. Handles NDK linker, APK signing, DEX packaging. |
| Target | `aarch64-linux-android` | NDK r27 | 95%+ of 2026 devices. Add `armv7-linux-androideabi` later if needed. |

### Why this stack

- **softbuffer over wgpu**: The game is a colored character grid. GPU acceleration adds complexity for zero visual benefit. softbuffer's API (`&mut [u32]` → `present()`) matches the GBA/NDS/Vita framebuffer pattern exactly.
- **winit over SDL2**: Pure Rust, no C cross-compilation. SDL2 has better Android track record but the build system overhead (NDK + SDL2 .so + Gradle) is significant.
- **winit over macroquad**: macroquad owns the event loop. winit lets us reuse our `GameView` → pixel buffer rendering pattern directly.
- **NativeActivity over GameActivity**: No text input needed (seed entry uses a roller, not keyboard). GameActivity adds Java dependency for on-screen keyboard support we don't use.
- **Standalone workspace**: Same as GBA/NDS/Vita — Android's NDK target constraints don't pollute the parent workspace.

### Desktop development path

winit + softbuffer runs identically on Linux/macOS/Windows. Develop and test the renderer on desktop using mouse clicks as touch stand-ins, then deploy to device for real touch testing.

## Crate Structure

```
crates/android/
├── Cargo.toml              standalone workspace
├── src/
│   ├── lib.rs              android_main, winit event loop, lifecycle
│   ├── render.rs           GameView → softbuffer pixel buffer (glyph blitting)
│   ├── input.rs            Touch → GameCommand translation
│   ├── palette.rs          GameColor → 0xRRGGBB pixel mapping
│   └── saves.rs            SaveBackend impl (Android internal storage)
```

### Dependency Graph

```
roguelike-core ←── roguelike-saves ←── roguelike-android
                                            │
                                            ├── winit (windowing + events)
                                            ├── softbuffer (pixel buffer)
                                            └── roguelike-renderer3d::font (glyph data)
```

## Phases

### Phase 1 — Proof of Life
Colored rectangles on screen, tap-to-move, single game session. No menus, no saves. Equivalent to Vita Phase 1 but with input.

Deliverables:
- `crates/android/` scaffold with standalone workspace
- softbuffer pixel buffer rendering via `GameView` (colored rects per tile)
- Touch input: tap adjacent tile = move, tap player = wait
- Runs on desktop (winit window) and Android (APK via cargo-apk2)
- Standard tier (`GameState` with full std)

### Phase 2 — Glyph Rendering + Full Touch
Replace colored rectangles with bitmap font glyphs from `roguelike-renderer3d::font`. Design the full touch control scheme.

Deliverables:
- 8x8 glyph blitting into pixel buffer (scaled to cell size)
- Tap distant tile = pathfind-to
- Tap monster = attack/look depending on adjacency
- Swipe = move in swipe direction
- Virtual button bar (Inventory, AutoExplore, Wait, Look, Stairs)
- FOV dimming for explored-but-not-visible tiles

### Phase 3 — Menus + Saves
Title screen, pause menu, settings, save/load.

Deliverables:
- Title screen with seed entry (tap-roller like GBA, not keyboard)
- Pause menu (Resume, Save, Settings, Quit)
- SaveBackend impl using Android internal storage (`internal_data_path()`)
- Settings persistence (auto-pickup, color palette)
- Casual mode support

### Phase 4 — Polish
Adaptive layout, gamepad, accessibility.

Deliverables:
- Responsive layout: phone (portrait 40x24) vs tablet (landscape 80x24)
- Physical gamepad support via winit (Bluetooth controllers)
- Color accessibility palettes (Protanopia, Deuteranopia, HighContrast)
- Haptic feedback on combat events (if Android API accessible via NDK)

### Phase 5 — Distribution
Play Store packaging.

Deliverables:
- App icon, splash screen
- APK signing for Play Store
- Min SDK 28 (Android 9 Pie)
- Optional: F-Droid metadata for open-source distribution

## Touch Input Design

The primary design challenge. Reference: `crates/nds/src/touch.rs` for tile-coordinate mapping.

### Coordinate mapping

```
screen_pixel → tile_col = pixel_x / cell_width
             → tile_row = pixel_y / cell_height
tile_col + viewport_x → world_x
tile_row + viewport_y → world_y
```

### Gesture → GameCommand mapping

| Gesture | Condition | Command |
|---------|-----------|---------|
| Tap tile | Adjacent to player | `Move(direction_toward_tap)` |
| Tap tile | Player's tile | `Wait` |
| Tap tile | Distant, walkable | `PathfindTo(x, y)` via auto-explore |
| Tap tile | Has stairs, adjacent | `Descend` |
| Tap entity | Adjacent | `Move(toward)` (attacks) |
| Tap entity | Distant | triggers Look mode on that tile |
| Swipe | Any direction | `Move(swipe_direction)` |
| Swipe + hold | Sustained contact | `Autorun(direction)` |
| Button bar tap | — | Mapped command (Inventory, AutoExplore, etc.) |

### Button bar

Fixed row at screen bottom, outside the map viewport:

```
[ INV ]  [ LOOK ]  [ WAIT ]  [ EXPL ]  [ STRS ]  [ MSG ]
```

Same pattern as NDS touch buttons (`crates/nds/src/touch.rs:screen_to_button`).

## Color Mapping

GameColor → `0xRRGGBB` (softbuffer u32 format, no alpha). Tuned for LCD panels (most Android devices), not OLED like Vita.

```rust
fn game_color_to_pixel(c: GameColor) -> u32 {
    match c {
        GameColor::Black     => 0x000000,
        GameColor::White     => 0xFFFFFF,
        GameColor::Grey      => 0xAAAAAA,
        GameColor::DarkGrey  => 0x555555,
        GameColor::Red       => 0xFF4444,
        GameColor::DarkRed   => 0xAA0000,
        GameColor::Green     => 0x44FF44,
        GameColor::DarkGreen => 0x00AA00,
        GameColor::Yellow    => 0xFFDD00,
        GameColor::DarkBlue  => 0x0000AA,
        GameColor::Cyan      => 0x44FFFF,
        GameColor::Rgb(r, g, b) => (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}
```
