# Plan: Platform Abstraction + Menu System

Next session plan. Platform abstraction first (enables menus and all future frontends), then a modular menu system with title screen and pause menu.

## Context

- Save/load is done. The critical path says: `Input abstraction -> Save/load -> Items + Menus`.
- Save/load is complete, so **menus** are next on the critical path.
- But menus need a rendering/input abstraction to avoid building them directly on crossterm (which would be thrown away for every future platform).
- The `GameColor` change (done in the save/load PR) already removed crossterm from `entity.rs` and `data.rs`. Only 3 files still import crossterm: `main.rs`, `input.rs`, `render.rs`.
- The [cross-platform architecture doc](cross-platform-architecture.md) outlines the workspace split. We're doing **Phase 1** (traits in the current crate) — the workspace split is Phase 2 for when a second frontend actually exists.

## Part 1: Platform Abstraction Traits

### Goal

Define `Renderer` and `InputSource` traits in core so game logic (and menus) can be written against them. Terminal implementations live in the existing rendering/input files.

### New file: `src/platform.rs`

```rust
use crate::types::{Coord, GameColor};

/// A command from the player. Platform-agnostic.
/// (GameCommand already exists in input.rs — re-export or move here.)

/// Abstraction over rendering output.
pub trait Renderer {
    /// Clear the entire screen.
    fn clear(&mut self);

    /// Draw a character at (x, y) with foreground and optional background color.
    fn draw_char(&mut self, x: Coord, y: Coord, ch: char, fg: GameColor);

    /// Draw a string starting at (x, y).
    fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor);

    /// Flush all pending draws to the screen.
    fn flush(&mut self);

    /// Screen dimensions (width, height) in character cells.
    fn screen_size(&self) -> (Coord, Coord);
}

/// Abstraction over input sources.
pub trait InputSource {
    /// Block until the next game command is available.
    fn next_command(&mut self) -> Option<GameCommand>;
}
```

### Changes to existing files

- `src/render.rs` — Implement `Renderer` for a `CrosstermRenderer` struct (wraps `Stdout`). Existing `render()` function refactored to use the trait methods, or kept as-is and `CrosstermRenderer` is used by menus only at first (incremental migration).
- `src/input.rs` — Implement `InputSource` for a `CrosstermInput` struct. The `translate_key_event` function becomes the implementation body.
- `src/main.rs` — Construct `CrosstermRenderer` + `CrosstermInput`, pass them to the game loop.
- `src/lib.rs` — Add `pub mod platform;`.

### Design decisions

- **Traits, not generics everywhere.** Use `&mut dyn Renderer` (trait objects) to keep the API simple. Performance is irrelevant for a turn-based game rendering at <30 FPS.
- **Colors in the trait.** `draw_char` takes `GameColor`, not crossterm `Color`. Each `Renderer` implementation maps to its platform colors internally.
- **Incremental migration.** The full game rendering doesn't need to move to the trait on day one. Menus use the trait, game rendering can migrate later. This keeps the PR focused.

## Part 2: Menu System

### Goal

A modular, reusable menu framework. Easy to add/remove items. Title screen and pause menu as the first two consumers.

### New file: `src/menu.rs`

```rust
/// A single menu entry.
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
}

/// What happens when a menu item is selected.
pub enum MenuAction {
    NewGame,
    ResumeGame,
    SaveGame,
    LoadGame,
    Quit,
    // Extend later: Config, Help, etc.
}

/// A menu screen — list of items with a selected index.
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self { ... }
    pub fn move_up(&mut self) { ... }
    pub fn move_down(&mut self) { ... }
    pub fn selected_action(&self) -> &MenuAction { ... }

    /// Render using the platform renderer trait.
    pub fn draw(&self, renderer: &mut dyn Renderer) { ... }
}
```

### Specific menus

**Title screen** (`MenuAction` items):
- New Game
- Quit

**Pause menu** (opened with `Esc` during gameplay):
- Resume
- Save Game
- Load Game
- Quit

Both are just `Menu::new(title, items)` — adding a config screen later is one function call.

### Game loop state machine

```
         ┌─────────┐
         │  Title   │──── New Game ───→ Playing
         │  Screen  │                      │
         └────┬─────┘                  Esc │
              │                            ▼
            Quit                     ┌──────────┐
              │                      │  Pause   │
              ▼                      │  Menu    │
            Exit                     └──┬───┬───┘
                                        │   │
                                   Resume  Save/Load/Quit
                                        │
                                        ▼
                                     Playing
```

New file or section in `src/game.rs` (or `src/app.rs`):

```rust
pub enum AppState {
    Title(Menu),
    Playing,
    Paused(Menu),
}
```

`main.rs` runs a top-level loop that dispatches on `AppState`:
- `Title` — draw menu, handle up/down/enter
- `Playing` — existing game loop
- `Paused` — draw menu over game screen, handle selection

### Menu input

Menus need simpler input than gameplay — just up/down/enter/escape. Options:

1. **Separate `MenuCommand` enum** — `Up, Down, Select, Back`. The `InputSource` could have a `next_menu_command()` method, or we reuse `GameCommand` with a menu-specific mapping.
2. **Reuse `GameCommand`** — `Move { dy: -1 }` = up, `Move { dy: 1 }` = down, `Wait` = select, `Quit` = back. Hacky but zero new types.

**Decision: Option 1.** A `MenuCommand` enum is cleaner and keeps menu logic independent from game logic. The `InputSource` trait gets a second method.

## File Summary

| File | Action |
|------|--------|
| `src/platform.rs` | **New** — `Renderer`, `InputSource` traits, `MenuCommand` enum |
| `src/menu.rs` | **New** — `Menu`, `MenuItem`, `MenuAction`, rendering/input logic |
| `src/render.rs` | **Modify** — add `CrosstermRenderer` implementing `Renderer` |
| `src/input.rs` | **Modify** — add `CrosstermInput` implementing `InputSource` |
| `src/main.rs` | **Modify** — `AppState` enum, top-level loop dispatching title/playing/paused |
| `src/game.rs` | **Modify** — `step()` returns info needed by `main` to decide transitions (e.g., game over triggers) |
| `src/lib.rs` | **Modify** — add `pub mod platform; pub mod menu;` |

## What This Unblocks

- **Config menu** — add `MenuAction::Config` + a config screen later
- **Help screen** — add `MenuAction::Help`
- **Workspace split** — `platform.rs` traits become the `roguelike-core` API boundary
- **WASM port** — implement `Renderer` + `InputSource` for web canvas
- **Controller support** — implement `InputSource` for gamepad
- **Items/inventory** — inventory screen is just another menu

## Out of Scope (for this session)

- Workspace split (Phase 2 of cross-platform doc — wait for second frontend)
- Full migration of game rendering to `Renderer` trait (incremental — menus first)
- Config/help/inventory screens (modular design makes these trivial to add later)
- Terminal keybindings for save/load outside menus (pause menu covers this)

## Verification

1. `cargo test` — all existing tests pass, new menu tests pass
2. `cargo clippy -- -D warnings` — no warnings
3. `cargo fmt --check` — formatted
4. Manual test: launch game, see title screen, start game, press Esc, see pause menu, save/load/resume/quit all work
5. Only `render.rs`, `input.rs`, and `main.rs` import crossterm (same as before — no new platform deps in core)
