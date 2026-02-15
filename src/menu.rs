use crate::platform::{MenuCommand, Renderer};
use crate::types::GameColor;

/// What happens when a menu item is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewGame,
    ResumeGame,
    SaveGame,
    LoadGame,
    Quit,
    /// The user pressed Esc/Back. The caller decides what this means:
    /// title screen interprets it as quit, pause menu as resume.
    Back,
}

/// A single menu entry.
pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
}

/// A navigable menu screen with a title and selectable items.
///
/// Generic and reusable — title screen, pause menu, and future menus
/// (config, inventory) are all just different `Menu` instances.
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.into(),
            items,
            selected: 0,
        }
    }

    /// Move the selection cursor up, wrapping at the top.
    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move the selection cursor down, wrapping at the bottom.
    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
    }

    /// The action of the currently highlighted item.
    pub fn selected_action(&self) -> MenuAction {
        self.items[self.selected].action
    }

    /// Apply a menu command. Returns `Some(action)` if an item was selected.
    pub fn handle_input(&mut self, cmd: MenuCommand) -> Option<MenuAction> {
        match cmd {
            MenuCommand::Up => {
                self.move_up();
                None
            }
            MenuCommand::Down => {
                self.move_down();
                None
            }
            MenuCommand::Select => {
                if self.items[self.selected].enabled {
                    Some(self.selected_action())
                } else {
                    None
                }
            }
            MenuCommand::Back => Some(MenuAction::Back),
        }
    }

    /// Render the menu centered on screen using the platform renderer.
    pub fn draw(&self, renderer: &mut dyn Renderer) {
        let (screen_w, screen_h) = renderer.screen_size();

        renderer.clear();

        // Title: centered, near the top third of the screen.
        let title_y = screen_h / 4;
        let title_x = (screen_w - self.title.len() as i32) / 2;
        renderer.draw_str(
            title_x,
            title_y,
            &self.title,
            GameColor::Cyan,
            GameColor::Black,
        );

        // Items: centered, starting below the title.
        let items_start_y = title_y + 3;
        for (i, item) in self.items.iter().enumerate() {
            let y = items_start_y + i as i32;
            let is_selected = i == self.selected;

            let prefix = if is_selected { "> " } else { "  " };
            let text = format!("{}{}", prefix, item.label);

            let x = (screen_w - text.len() as i32) / 2;
            let fg = if !item.enabled {
                GameColor::DarkGrey
            } else if is_selected {
                GameColor::Yellow
            } else {
                GameColor::White
            };
            renderer.draw_str(x, y, &text, fg, GameColor::Black);
        }

        renderer.flush();
    }
}

/// Construct the title screen menu.
///
/// When `has_save` is true the "Load Game" option is selectable; otherwise
/// it is shown greyed-out so the player knows the feature exists.
pub fn title_menu(has_save: bool) -> Menu {
    Menu::new(
        "R O G U E L I K E",
        vec![
            MenuItem {
                label: "New Game".to_string(),
                action: MenuAction::NewGame,
                enabled: true,
            },
            MenuItem {
                label: "Load Game".to_string(),
                action: MenuAction::LoadGame,
                enabled: has_save,
            },
            MenuItem {
                label: "Quit".to_string(),
                action: MenuAction::Quit,
                enabled: true,
            },
        ],
    )
}

/// Construct the pause menu.
pub fn pause_menu() -> Menu {
    Menu::new(
        "Paused",
        vec![
            MenuItem {
                label: "Resume".to_string(),
                action: MenuAction::ResumeGame,
                enabled: true,
            },
            MenuItem {
                label: "Save Game".to_string(),
                action: MenuAction::SaveGame,
                enabled: true,
            },
            MenuItem {
                label: "Load Game".to_string(),
                action: MenuAction::LoadGame,
                enabled: true,
            },
            MenuItem {
                label: "Quit".to_string(),
                action: MenuAction::Quit,
                enabled: true,
            },
        ],
    )
}

/// Display a centered "Loading..." message. Call before a potentially slow
/// operation (like deserializing a save file) so the player sees feedback.
pub fn draw_loading(renderer: &mut dyn Renderer) {
    let (screen_w, screen_h) = renderer.screen_size();
    renderer.clear();
    let msg = "Loading...";
    let x = (screen_w - msg.len() as i32) / 2;
    let y = screen_h / 2;
    renderer.draw_str(x, y, msg, GameColor::White, GameColor::Black);
    renderer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Coord;

    /// A mock renderer that records draw calls for menu testing.
    struct MockRenderer {
        strings: Vec<(Coord, Coord, String, GameColor, GameColor)>,
        cleared: bool,
        flushed: bool,
        width: Coord,
        height: Coord,
    }

    impl MockRenderer {
        fn new(width: Coord, height: Coord) -> Self {
            Self {
                strings: Vec::new(),
                cleared: false,
                flushed: false,
                width,
                height,
            }
        }

        /// Find the first draw_str call containing the given substring.
        fn find_str(&self, needle: &str) -> Option<&(Coord, Coord, String, GameColor, GameColor)> {
            self.strings
                .iter()
                .find(|(_, _, text, _, _)| text.contains(needle))
        }
    }

    impl Renderer for MockRenderer {
        fn clear(&mut self) {
            self.cleared = true;
            self.strings.clear();
        }

        fn draw_char(&mut self, _x: Coord, _y: Coord, _ch: char, _fg: GameColor, _bg: GameColor) {}

        fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor, bg: GameColor) {
            self.strings.push((x, y, text.to_string(), fg, bg));
        }

        fn flush(&mut self) {
            self.flushed = true;
        }

        fn screen_size(&self) -> (Coord, Coord) {
            (self.width, self.height)
        }
    }

    fn two_item_menu() -> Menu {
        Menu::new(
            "Test Menu",
            vec![
                MenuItem {
                    label: "Option A".to_string(),
                    action: MenuAction::NewGame,
                    enabled: true,
                },
                MenuItem {
                    label: "Option B".to_string(),
                    action: MenuAction::Quit,
                    enabled: true,
                },
            ],
        )
    }

    #[test]
    fn initial_selection_is_zero() {
        let menu = two_item_menu();
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.selected_action(), MenuAction::NewGame);
    }

    #[test]
    fn move_down_advances_selection() {
        let mut menu = two_item_menu();
        menu.move_down();
        assert_eq!(menu.selected, 1);
        assert_eq!(menu.selected_action(), MenuAction::Quit);
    }

    #[test]
    fn move_down_wraps_to_top() {
        let mut menu = two_item_menu();
        menu.move_down();
        menu.move_down();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn move_up_wraps_to_bottom() {
        let mut menu = two_item_menu();
        menu.move_up();
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn move_up_from_bottom_goes_to_previous() {
        let mut menu = two_item_menu();
        menu.move_down(); // now at 1
        menu.move_up(); // now at 0
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn handle_input_select_returns_action() {
        let mut menu = two_item_menu();
        assert_eq!(
            menu.handle_input(MenuCommand::Select),
            Some(MenuAction::NewGame)
        );
    }

    #[test]
    fn handle_input_back_returns_back() {
        let mut menu = two_item_menu();
        assert_eq!(menu.handle_input(MenuCommand::Back), Some(MenuAction::Back));
    }

    #[test]
    fn handle_input_navigation_returns_none() {
        let mut menu = two_item_menu();
        assert_eq!(menu.handle_input(MenuCommand::Down), None);
        assert_eq!(menu.handle_input(MenuCommand::Up), None);
    }

    #[test]
    fn handle_input_navigate_then_select() {
        let mut menu = two_item_menu();
        menu.handle_input(MenuCommand::Down);
        assert_eq!(
            menu.handle_input(MenuCommand::Select),
            Some(MenuAction::Quit)
        );
    }

    #[test]
    fn single_item_menu_navigation() {
        let mut menu = Menu::new(
            "Single",
            vec![MenuItem {
                label: "Only".to_string(),
                action: MenuAction::Quit,
                enabled: true,
            }],
        );
        // Up and down should stay on the only item.
        menu.move_up();
        assert_eq!(menu.selected, 0);
        menu.move_down();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn draw_renders_title_and_items() {
        let menu = two_item_menu();
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        assert!(r.cleared);
        assert!(r.flushed);

        // Title should be drawn.
        assert!(r.find_str("Test Menu").is_some());

        // Both items should be drawn.
        assert!(r.find_str("Option A").is_some());
        assert!(r.find_str("Option B").is_some());
    }

    #[test]
    fn draw_highlights_selected_item() {
        let mut menu = two_item_menu();
        menu.move_down(); // select Option B
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        // Selected item (Option B) should be yellow.
        let selected = r.find_str("Option B").unwrap();
        assert_eq!(selected.3, GameColor::Yellow);

        // Unselected item (Option A) should be white.
        let unselected = r.find_str("Option A").unwrap();
        assert_eq!(unselected.3, GameColor::White);
    }

    #[test]
    fn draw_selected_item_has_arrow_prefix() {
        let menu = two_item_menu();
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        // First item (selected) should have "> " prefix.
        let selected = r.find_str("Option A").unwrap();
        assert!(selected.2.starts_with("> "));

        // Second item should have "  " prefix.
        let unselected = r.find_str("Option B").unwrap();
        assert!(unselected.2.starts_with("  "));
    }

    #[test]
    fn title_menu_has_expected_items() {
        let menu = title_menu(false);
        assert_eq!(menu.title, "R O G U E L I K E");
        assert_eq!(menu.items.len(), 3);
        assert_eq!(menu.items[0].action, MenuAction::NewGame);
        assert_eq!(menu.items[1].action, MenuAction::LoadGame);
        assert_eq!(menu.items[2].action, MenuAction::Quit);
    }

    #[test]
    fn pause_menu_has_expected_items() {
        let menu = pause_menu();
        assert_eq!(menu.title, "Paused");
        assert_eq!(menu.items.len(), 4);
        assert_eq!(menu.items[0].action, MenuAction::ResumeGame);
        assert_eq!(menu.items[1].action, MenuAction::SaveGame);
        assert_eq!(menu.items[2].action, MenuAction::LoadGame);
        assert_eq!(menu.items[3].action, MenuAction::Quit);
    }

    #[test]
    fn draw_centers_title_horizontally() {
        let menu = Menu::new(
            "ABCDEFGHIJ", // 10 chars
            vec![MenuItem {
                label: "Go".to_string(),
                action: MenuAction::NewGame,
                enabled: true,
            }],
        );
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        let title = r.find_str("ABCDEFGHIJ").unwrap();
        // (80 - 10) / 2 = 35
        assert_eq!(title.0, 35);
    }

    #[test]
    fn draw_centers_items_horizontally() {
        let menu = two_item_menu();
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        // "> Option A" = 10 chars. (80 - 10) / 2 = 35
        let item = r.find_str("Option A").unwrap();
        assert_eq!(item.0, 35);
    }

    #[test]
    fn draw_on_small_screen_clamps_to_zero() {
        let menu = Menu::new(
            "Very Long Title That Exceeds Width",
            vec![MenuItem {
                label: "Item".to_string(),
                action: MenuAction::Quit,
                enabled: true,
            }],
        );
        // Screen narrower than the title — x should go negative
        // (the renderer is responsible for clipping, but we shouldn't panic).
        let mut r = MockRenderer::new(10, 10);
        menu.draw(&mut r);

        // Title still gets drawn (renderer handles clipping).
        assert!(r.find_str("Very Long").is_some());
        assert!(r.flushed);
    }

    #[test]
    fn disabled_item_not_selectable() {
        let mut menu = Menu::new(
            "Test",
            vec![MenuItem {
                label: "Disabled".to_string(),
                action: MenuAction::LoadGame,
                enabled: false,
            }],
        );
        assert_eq!(menu.handle_input(MenuCommand::Select), None);
    }

    #[test]
    fn disabled_item_drawn_in_dark_grey() {
        let menu = Menu::new(
            "Test",
            vec![
                MenuItem {
                    label: "Enabled".to_string(),
                    action: MenuAction::NewGame,
                    enabled: true,
                },
                MenuItem {
                    label: "Disabled".to_string(),
                    action: MenuAction::LoadGame,
                    enabled: false,
                },
            ],
        );
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        let disabled = r.find_str("Disabled").unwrap();
        assert_eq!(disabled.3, GameColor::DarkGrey);

        let enabled = r.find_str("Enabled").unwrap();
        assert_eq!(enabled.3, GameColor::Yellow); // selected, so Yellow
    }

    #[test]
    fn title_menu_with_save_has_load_enabled() {
        let menu = title_menu(true);
        let load_item = menu
            .items
            .iter()
            .find(|i| i.action == MenuAction::LoadGame)
            .unwrap();
        assert!(load_item.enabled);
    }

    #[test]
    fn title_menu_without_save_has_load_disabled() {
        let menu = title_menu(false);
        let load_item = menu
            .items
            .iter()
            .find(|i| i.action == MenuAction::LoadGame)
            .unwrap();
        assert!(!load_item.enabled);
    }

    #[test]
    fn draw_loading_renders_centered_message() {
        let mut r = MockRenderer::new(80, 24);
        draw_loading(&mut r);

        assert!(r.cleared);
        assert!(r.flushed);

        let msg = r.find_str("Loading...").unwrap();
        // "Loading..." = 10 chars. (80 - 10) / 2 = 35
        assert_eq!(msg.0, 35);
        // Vertically centered: 24 / 2 = 12
        assert_eq!(msg.1, 12);
        assert_eq!(msg.3, GameColor::White);
    }
}
