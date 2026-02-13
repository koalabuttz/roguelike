# Roguelike

A terminal-based roguelike dungeon crawler written in Rust.

Explore randomly generated dungeons, fight monsters, and try to survive. Renders directly in the terminal using Unicode box-drawing characters and ANSI colors — no external game engine required.

## Building and Running

Requires [Rust](https://www.rust-lang.org/tools/install) (edition 2024).

```sh
cargo run
```

The game adapts to your terminal size automatically.

## Controls

| Action | Keys |
|--------|------|
| Move cardinal | Arrow keys, `hjkl` (vi), numpad `2468` |
| Move diagonal | `yubn` (vi), numpad `7913` |
| Wait a turn | `.` or numpad `5` |
| Quit | `q`, `Esc`, or `Ctrl+C` |

Moving into a monster attacks it.

## Gameplay

- `@` is you. Monsters appear as colored letters (`g`oblin, `o`rc, `T`roll).
- Monsters wake and chase you when they enter your field of view.
- `%` marks a corpse. Dead monsters stay on the map.
- The HP bar and message log are at the bottom of the screen.

## Monsters

| Monster | Glyph | HP | ATK | DEF | Spawn Weight |
|---------|-------|----|-----|-----|--------------|
| Goblin  | `g`   | 6  | 3   | 0   | 60%          |
| Orc     | `o`   | 12 | 4   | 1   | 30%          |
| Troll   | `T`   | 20 | 6   | 3   | 10%          |

## Adding a New Monster

All content lives in `src/data.rs`. To add a monster:

1. Define a template constant:

```rust
pub const DRAGON: MonsterTemplate = MonsterTemplate {
    name: "Dragon",
    glyph: 'D',
    color: Color::Red,
    hp: 40,
    attack: 10,
    defense: 5,
    ai: AiBehavior::Chase,
};
```

2. Add it to the spawn table:

```rust
SpawnEntry { template: &DRAGON, weight: 5 },
```

If it needs new AI, add a variant to `AiBehavior` in `src/entity.rs` and implement it in `src/ai.rs`.

## Project Structure

```
src/
  main.rs          Event loop and input handling
  game.rs          GameState struct and core game logic
  data.rs          Monster templates, spawn table, config constants
  entity.rs        Entity struct, EntityKind, AiBehavior enum
  ai.rs            Monster AI system (dispatches on AiBehavior)
  combat.rs        Melee attack resolution
  spawn.rs         Monster spawning using weighted tables
  map.rs           Map struct and dungeon generation
  fov.rs           Field of view (recursive shadowcasting)
  render.rs        Terminal rendering
  message_log.rs   Message log
```

## Configuration

Game-wide tuning knobs are in `src/data.rs` under `GameConfig`:

| Setting | Default | Description |
|---------|---------|-------------|
| `fov_radius` | 8 | Player's field of view radius |
| `max_rooms` | 30 | Maximum rooms per dungeon |
| `room_size_min` | 4 | Minimum room dimension |
| `room_size_max` | 10 | Maximum room dimension |
| `max_monsters_per_room` | 2 | Monster cap per room |
| `ui_bottom_rows` | 5 | Rows reserved for status bar and log |

## Dependencies

- [crossterm](https://crates.io/crates/crossterm) 0.28 — cross-platform terminal manipulation
- [rand](https://crates.io/crates/rand) 0.8 — random number generation

### Optional Dependencies (planned)
- [rodio](https://crates.io/crates/rodio) — Audio playback for sound effects and music (background threads, MP3/WAV/OGG)

## Roadmap

### Foundation (enables networking, platforms & advanced features)
- [ ] **Input abstraction** — `GameCommand` enum, decouple game logic from terminal input
- [ ] **Platform abstraction** — Traits for input, rendering, and audio; game logic never imports platform-specific crates
- [ ] **Type aliases** — `Coord`, `Stat` type aliases for coordinates and stats, enabling platform-specific sizing
- [ ] **Seeded RNG** — Separate RNG streams per system (map, combat, spawn, loot) for deterministic replay
- [ ] **Save/load** — Serialize/deserialize game state via serde

### Core Gameplay
- [ ] **Items** — Potions, scrolls, equipment, inventory system
- [ ] **Hunger** — Food clock mechanic to encourage exploration
- [ ] **Stairs** — Multi-level dungeons with procedural depth
- [ ] **Experience/leveling** — Player progression system
- [ ] **Magic/abilities** — Spells, special attacks, or class-specific powers
- [ ] **A\* pathfinding** — Smarter monster AI that navigates around obstacles
- [ ] **Meta-progression** — Persistent unlocks between runs (classes, upgrades, achievements)

### UI/UX
- [ ] **Controller support** — Gamepad input on all desktop platforms; d-pad/stick for movement, context-sensitive menus for actions
- [ ] **Steam Deck** — Verified controller layout, Steam Input API integration
- [ ] **Menus** — Inventory screen, character sheet, help screen (designed for both keyboard and controller)
- [ ] **Look mode** — Cursor to examine tiles and entities
- [ ] **Targeting** — Ranged attacks, spell targeting
- [ ] **Options/settings** — Keybind customization, display preferences, difficulty modes

### Accessibility
- [ ] **Colorblind modes** — Alternative color palettes (current green/dark-green/dark-red is problematic for ~8% of men)
- [ ] **High-contrast mode** — Maximum readability option
- [ ] **Screen reader support** — Structured output for NVDA/JAWS/VoiceOver

### Networking
- [ ] **Replay system** — Record and playback games deterministically
- [ ] **Shared leaderboard** — REST API for score submission
- [ ] **Daily challenges** — Everyone plays the same seed, compare scores
- [ ] **Seed sharing** — Share a seed as a code/URL, others play the same dungeon
- [ ] **Live spectating** — Watch other players in real-time via WebSocket
- [ ] **Bones files** — Dead players leave traces in others' dungeons

### Platform Support
- [x] **Windows / macOS / Linux** — Native terminal via crossterm (current)
- [ ] **CI matrix** — Test on all three desktop OS (Intel + ARM) in GitHub Actions
- [ ] **Web (WASM)** — Browser-based play via wasm-pack + xterm.js; enables browser spectating and leaderboards
- [ ] **Game Boy Advance** — Native ARM via `thumbv4t-none-eabi` target + `gba` crate; no_std, fixed-size containers
- [ ] **SSH server** — Server-side play via russh, NetHack-server style (players connect via SSH)
- [ ] **Commodore 64** — Native 6502 via rust-mos; no_std, fixed-size containers, 8-bit types, C64 screen/keyboard I/O

### Modding
- [ ] **Data-driven content** — Move templates to external files (RON/TOML); add monsters/items without recompiling
- [ ] **Hot reload** — Reload data files during development without restarting
- [ ] **Scripting** — Embedded Lua or Rhai for custom AI, quests, and event handlers (future)

### Developer Tools
- [ ] **Debug overlay** — Visualize AI state, FOV boundaries, pathfinding routes
- [ ] **Balance telemetry** — Track average run length, death causes, monster kill rates
- [ ] **Map editor** — Visual tool for designing and testing dungeon layouts

### Polish
- [ ] **Animation effects** — Attack swooshes, spell particles (terminal-rendered)
- [ ] **Sound effects** — Audio via rodio (MP3/WAV/OGG), background threads, SoundEvent enum
- [ ] **Music** — Atmospheric dungeon tracks, boss themes, adaptive based on depth/danger
- [ ] **Tileset support** — Alternative to ASCII (graphical tiles)
- [ ] **Localization (i18n)** — Externalized strings, multi-language support
- [ ] **Tutorial/guided run** — Introduce mechanics gradually for new players

## License

GPL-3.0-or-later
