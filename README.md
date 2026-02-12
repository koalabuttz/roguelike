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

## License

GPL-3.0-or-later
