use crate::command::Direction;
use crate::game::{GameState, LookOptions, TileInfo};
use crate::platform::Renderer;
use crate::rules::{damage, health};
use crate::types::{Coord, GameColor};

/// Result of handling a look-mode input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookAction {
    /// The look cursor should remain open.
    Continue,
    /// The look cursor should close.
    Close,
}

/// A command within look mode (separate from GameCommand — no autorun/attack).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookCommand {
    /// Move the cursor in a direction.
    Move(Direction),
    /// Close look mode.
    Close,
}

/// Cursor for look mode. Starts at the player's position and can be moved
/// around the map to examine tiles.
pub struct LookCursor {
    pub cursor_x: Coord,
    pub cursor_y: Coord,
}

impl LookCursor {
    /// Create a new look cursor at the player's position.
    pub fn new(player_x: Coord, player_y: Coord) -> Self {
        Self {
            cursor_x: player_x,
            cursor_y: player_y,
        }
    }

    /// Handle a look-mode command. Returns whether to continue or close.
    pub fn handle_input(&mut self, cmd: LookCommand, state: &GameState) -> LookAction {
        match cmd {
            LookCommand::Move(dir) => {
                let (dx, dy) = dir.to_offset();
                let nx = self.cursor_x + dx;
                let ny = self.cursor_y + dy;
                // Clamp to map bounds.
                if state.map.in_bounds(nx, ny) {
                    self.cursor_x = nx;
                    self.cursor_y = ny;
                }
                LookAction::Continue
            }
            LookCommand::Close => LookAction::Close,
        }
    }

    /// Get tile info at the current cursor position.
    pub fn current_info(&self, state: &GameState) -> TileInfo {
        state.look_at(self.cursor_x, self.cursor_y)
    }

    /// Get tile info with configurable reveal options (for dev-tools overlays).
    pub fn current_info_with(&self, state: &GameState, opts: &LookOptions) -> TileInfo {
        state.look_at_with(self.cursor_x, self.cursor_y, opts)
    }

    /// Draw the look-mode overlay: cursor glyph and description in status area.
    ///
    /// `viewport_offset` shifts world→screen coords (pass `(0, 0)` when there
    /// is no viewport scrolling).
    pub fn draw_overlay(
        &self,
        renderer: &mut dyn Renderer,
        info: &TileInfo,
        screen_height: Coord,
        message_log_lines: Coord,
        viewport_offset: (Coord, Coord),
    ) {
        // Draw cursor at the cursor position, offset by viewport.
        let (vx, vy) = viewport_offset;
        renderer.draw_char(
            self.cursor_x - vx,
            self.cursor_y - vy,
            'X',
            GameColor::Yellow,
            GameColor::DarkBlue,
        );

        // Draw description in the status bar row (just above the message log).
        let status_row = screen_height - 1 - message_log_lines;
        let desc = format_look_description(info);
        let label = format!("[Look] {}", desc);
        let (screen_w, _) = renderer.screen_size();
        // Pad to fill the row, truncate if too long.
        let display: String = label.chars().take(screen_w as usize).collect();
        renderer.draw_str(
            0,
            status_row,
            &display,
            GameColor::Cyan,
            GameColor::DarkBlue,
        );
    }
}

/// Format a human-readable description of a tile for look mode.
pub fn format_look_description(info: &TileInfo) -> String {
    let coords = format!("({},{})", info.x, info.y);

    if !info.explored {
        return format!("{} Unexplored", coords);
    }

    if !info.visible {
        return format!("{} {} (remembered)", coords, info.terrain);
    }

    let base = match &info.entity {
        Some(ent) if ent.alive => {
            let tier = health::health_tier(damage::narrow(ent.hp), damage::narrow(ent.max_hp));
            let desc = health::health_description(tier);
            format!(
                "{} {} - {} ({}) ({})",
                coords, info.terrain, ent.name, ent.glyph, desc
            )
        }
        Some(ent) => {
            format!("{} {} - {} corpse", coords, info.terrain, ent.name)
        }
        None => {
            format!("{} {}", coords, info.terrain)
        }
    };

    if info.items.is_empty() {
        base
    } else {
        let names: Vec<&str> = info.items.iter().map(|i| i.name.as_str()).collect();
        format!("{} [{}]", base, names.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::entity::Entity;
    use crate::fov;
    use crate::game::GameState;
    use crate::map::{Map, Tile};
    use crate::message_log::MessageLog;

    fn test_game() -> GameState {
        let mut m = Map::new(20, 20);
        for y in 1..=10 {
            for x in 1..=10 {
                let idx = m.idx(x, y);
                m.tiles[idx] = Tile::Floor;
            }
        }

        let player = Entity::player(5, 5);
        let visible = fov::compute_fov(&m, 5, 5, 8);
        let explored = visible.clone();

        GameState {
            map: m,
            entities: vec![player],
            fov_radius: 8,
            visible,
            explored,
            log: MessageLog::new(),
            game_over: false,
            turn_count: 0,
            seed: 0,
            preset: None,
            dirty: false,
            regen_interval: data::config().regen_interval,
            max_autorun_steps: data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
            ground_items: Vec::new(),
            item_catalog: data::compiled_defaults().items,
            equipment: Default::default(),
            inventory: Default::default(),
            auto_pickup: false,
            depth: 1,
            target_depth: 5,
            game_won: false,
            depth_scaling: Default::default(),
            max_rooms: 30,
            room_size_min: 4,
            room_size_max: 10,
            max_monsters_per_room: 2,
            max_items_per_room: 1,
        }
    }

    #[test]
    fn cursor_starts_at_player() {
        let cursor = LookCursor::new(5, 5);
        assert_eq!(cursor.cursor_x, 5);
        assert_eq!(cursor.cursor_y, 5);
    }

    #[test]
    fn cursor_moves() {
        let gs = test_game();
        let mut cursor = LookCursor::new(5, 5);
        let action = cursor.handle_input(LookCommand::Move(Direction::East), &gs);
        assert_eq!(action, LookAction::Continue);
        assert_eq!(cursor.cursor_x, 6);
        assert_eq!(cursor.cursor_y, 5);
    }

    #[test]
    fn cursor_clamps_to_map_bounds() {
        let gs = test_game();
        let mut cursor = LookCursor::new(0, 0);
        let action = cursor.handle_input(LookCommand::Move(Direction::West), &gs);
        assert_eq!(action, LookAction::Continue);
        // Should not move out of bounds.
        assert_eq!(cursor.cursor_x, 0);
        assert_eq!(cursor.cursor_y, 0);
    }

    #[test]
    fn cursor_close_returns_close_action() {
        let gs = test_game();
        let mut cursor = LookCursor::new(5, 5);
        let action = cursor.handle_input(LookCommand::Close, &gs);
        assert_eq!(action, LookAction::Close);
    }

    #[test]
    fn current_info_delegates_to_look_at() {
        let mut gs = test_game();
        gs.update_fov();
        let cursor = LookCursor::new(5, 5);
        let info = cursor.current_info(&gs);
        assert_eq!(info.terrain, "Floor");
        assert!(info.entity.is_some());
    }

    #[test]
    fn format_description_visible_monster() {
        let info = TileInfo {
            x: 6,
            y: 5,
            terrain: "Floor".into(),
            entity: Some(crate::game::EntityInfo {
                name: "Goblin".into(),
                glyph: 'g',
                x: 6,
                y: 5,
                hp: 6,
                max_hp: 6,
                alive: true,
            }),
            items: Vec::new(),
            visible: true,
            explored: true,
            glyph: 'g',
        };
        assert_eq!(
            format_look_description(&info),
            "(6,5) Floor - Goblin (g) (healthy)"
        );
    }

    #[test]
    fn format_description_corpse() {
        let info = TileInfo {
            x: 7,
            y: 5,
            terrain: "Floor".into(),
            entity: Some(crate::game::EntityInfo {
                name: "Orc".into(),
                glyph: '%',
                x: 7,
                y: 5,
                hp: 0,
                max_hp: 12,
                alive: false,
            }),
            items: Vec::new(),
            visible: true,
            explored: true,
            glyph: '%',
        };
        assert_eq!(format_look_description(&info), "(7,5) Floor - Orc corpse");
    }

    #[test]
    fn format_description_empty_floor() {
        let info = TileInfo {
            x: 3,
            y: 3,
            terrain: "Floor".into(),
            entity: None,
            items: Vec::new(),
            visible: true,
            explored: true,
            glyph: '.',
        };
        assert_eq!(format_look_description(&info), "(3,3) Floor");
    }

    #[test]
    fn format_description_floor_with_item() {
        let info = TileInfo {
            x: 4,
            y: 4,
            terrain: "Floor".into(),
            entity: None,
            items: vec![crate::game::ItemInfo {
                name: "Health Potion".into(),
                glyph: '!',
                x: 4,
                y: 4,
            }],
            visible: true,
            explored: true,
            glyph: '!',
        };
        assert_eq!(
            format_look_description(&info),
            "(4,4) Floor [Health Potion]"
        );
    }

    #[test]
    fn format_description_monster_with_item() {
        let info = TileInfo {
            x: 6,
            y: 5,
            terrain: "Floor".into(),
            entity: Some(crate::game::EntityInfo {
                name: "Goblin".into(),
                glyph: 'g',
                x: 6,
                y: 5,
                hp: 6,
                max_hp: 6,
                alive: true,
            }),
            items: vec![crate::game::ItemInfo {
                name: "Short Sword".into(),
                glyph: '/',
                x: 6,
                y: 5,
            }],
            visible: true,
            explored: true,
            glyph: 'g',
        };
        assert_eq!(
            format_look_description(&info),
            "(6,5) Floor - Goblin (g) (healthy) [Short Sword]"
        );
    }

    #[test]
    fn format_description_explored_not_visible() {
        let info = TileInfo {
            x: 10,
            y: 10,
            terrain: "Floor".into(),
            entity: None,
            items: Vec::new(),
            visible: false,
            explored: true,
            glyph: '.',
        };
        assert_eq!(format_look_description(&info), "(10,10) Floor (remembered)");
    }

    #[test]
    fn format_description_unexplored() {
        let info = TileInfo {
            x: 15,
            y: 15,
            terrain: "Unknown".into(),
            entity: None,
            items: Vec::new(),
            visible: false,
            explored: false,
            glyph: ' ',
        };
        assert_eq!(format_look_description(&info), "(15,15) Unexplored");
    }

    #[test]
    fn draw_overlay_renders_cursor_and_description() {
        let mut gs = test_game();
        gs.update_fov();
        let cursor = LookCursor::new(5, 5);
        let info = cursor.current_info(&gs);

        let mut renderer = MockRenderer::new(80, 24);
        cursor.draw_overlay(&mut renderer, &info, 24, 4, (0, 0));

        // Cursor should be drawn at (5, 5) with 'X'.
        assert!(renderer.chars.iter().any(|(x, y, ch, fg, bg)| {
            *x == 5
                && *y == 5
                && *ch == 'X'
                && *fg == GameColor::Yellow
                && *bg == GameColor::DarkBlue
        }));

        // Status row should contain "[Look]" description.
        assert!(
            renderer
                .strings
                .iter()
                .any(|(_, _, text, _, _)| text.contains("[Look]"))
        );
    }

    /// A mock renderer for testing draw calls.
    struct MockRenderer {
        chars: Vec<(Coord, Coord, char, GameColor, GameColor)>,
        strings: Vec<(Coord, Coord, String, GameColor, GameColor)>,
        width: Coord,
        height: Coord,
    }

    impl MockRenderer {
        fn new(width: Coord, height: Coord) -> Self {
            Self {
                chars: Vec::new(),
                strings: Vec::new(),
                width,
                height,
            }
        }
    }

    impl Renderer for MockRenderer {
        fn clear(&mut self) {
            self.chars.clear();
            self.strings.clear();
        }

        fn draw_char(&mut self, x: Coord, y: Coord, ch: char, fg: GameColor, bg: GameColor) {
            self.chars.push((x, y, ch, fg, bg));
        }

        fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor, bg: GameColor) {
            self.strings.push((x, y, text.to_string(), fg, bg));
        }

        fn flush(&mut self) {}

        fn screen_size(&self) -> (Coord, Coord) {
            (self.width, self.height)
        }
    }
}
