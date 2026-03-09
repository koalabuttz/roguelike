use crate::platform::{MenuCommand, Renderer};
use crate::saves::SlotMetadata;
use crate::settings::{Platform, Setting, Settings};
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
    /// The user confirmed a yes/no dialog (the "Yes" choice).
    Confirm,
    /// Navigate to the settings submenu.
    Settings,
    /// Toggle casual/classic mode in the settings menu.
    ToggleCasualMode,
    /// Toggle explored-percentage display in the settings menu.
    ToggleShowExploredPct,
    /// Return to the title screen from the pause menu.
    TitleScreen,
    /// Toggle coordinates display in the settings menu.
    ToggleShowCoordinates,
    /// Toggle keybind hints in the settings menu.
    ToggleShowKeybindHints,
    /// Toggle corpse rendering in the settings menu.
    ToggleShowCorpses,
    /// Toggle kill counter in the status bar.
    ToggleShowKills,
    /// Toggle turn counter in the status bar.
    ToggleShowTurnCount,
    /// Toggle vi-key movement in the settings menu.
    ToggleViKeys,
    /// Toggle numpad movement in the settings menu.
    ToggleNumpad,
    /// Cycle animation speed in the settings menu.
    CycleAnimationSpeed,
    /// Cycle autosave frequency in the settings menu.
    CycleAutosaveFrequency,
    /// Cycle message log lines in the settings menu.
    CycleMessageLogLines,
    /// A save slot was selected (0-indexed slot number).
    SelectSlot(u8),
    /// Enter a seed code to start a specific dungeon.
    EnterSeed,
    /// Cycle the color palette (for colorblind accessibility).
    CycleColorPalette,
    /// Cycle left-hand keyboard layout.
    CycleLeftHandLayout,
    /// Edit the player's name (text input dialog).
    EditPlayerName,
    /// Cycle player pronouns.
    CyclePronouns,
    /// Return to the server lobby (SSH only).
    Lobby,
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

        // Items: left-justified as a block, block centered on screen.
        let items_start_y = title_y + 3;
        let max_item_width = self
            .items
            .iter()
            .map(|item| item.label.len() as i32 + 2) // +2 for "> " or "  " prefix
            .max()
            .unwrap_or(0);
        let items_x = (screen_w - max_item_width) / 2;

        for (i, item) in self.items.iter().enumerate() {
            let y = items_start_y + i as i32;
            let is_selected = i == self.selected;

            let prefix = if is_selected { "> " } else { "  " };
            let text = format!("{}{}", prefix, item.label);

            let fg = if !item.enabled {
                GameColor::DarkGrey
            } else if is_selected {
                GameColor::Yellow
            } else {
                GameColor::White
            };
            renderer.draw_str(items_x, y, &text, fg, GameColor::Black);
        }

        renderer.flush();
    }
}

/// Construct the title screen menu.
///
/// In **classic** mode the layout is: Continue (enabled if save) → New Game →
/// Settings → Quit. In **casual** mode: New Game → Load Game (enabled if
/// save) → Settings → Quit. On SSH, "Quit" is replaced with "Log Out".
pub fn title_menu(has_save: bool, casual_mode: bool, platform: Platform) -> Menu {
    let exit_item = if platform == Platform::Ssh {
        MenuItem {
            label: "Lobby".to_string(),
            action: MenuAction::Lobby,
            enabled: true,
        }
    } else {
        MenuItem {
            label: "Quit".to_string(),
            action: MenuAction::Quit,
            enabled: true,
        }
    };

    let mut items = if casual_mode {
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
                label: "Seed".to_string(),
                action: MenuAction::EnterSeed,
                enabled: true,
            },
            MenuItem {
                label: "Settings".to_string(),
                action: MenuAction::Settings,
                enabled: true,
            },
        ]
    } else {
        vec![
            MenuItem {
                label: "Continue".to_string(),
                action: MenuAction::LoadGame,
                enabled: has_save,
            },
            MenuItem {
                label: "New Game".to_string(),
                action: MenuAction::NewGame,
                enabled: true,
            },
            MenuItem {
                label: "Seed".to_string(),
                action: MenuAction::EnterSeed,
                enabled: true,
            },
            MenuItem {
                label: "Settings".to_string(),
                action: MenuAction::Settings,
                enabled: true,
            },
        ]
    };
    items.push(exit_item);
    Menu::new("R O G U E L I K E", items)
}

/// Construct the pause menu.
///
/// In **classic** mode: Resume → Title Screen → Quit (no save/load — autosave
/// handles it). In **casual** mode: Resume → Save Game → Load Game → Title
/// Screen → Quit. On SSH, "Quit" is replaced with "Log Out".
pub fn pause_menu(casual_mode: bool, platform: Platform) -> Menu {
    let exit_item = if platform == Platform::Ssh {
        MenuItem {
            label: "Lobby".to_string(),
            action: MenuAction::Lobby,
            enabled: true,
        }
    } else {
        MenuItem {
            label: "Quit".to_string(),
            action: MenuAction::Quit,
            enabled: true,
        }
    };

    let mut items = if casual_mode {
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
                label: "Title Screen".to_string(),
                action: MenuAction::TitleScreen,
                enabled: true,
            },
        ]
    } else {
        vec![
            MenuItem {
                label: "Resume".to_string(),
                action: MenuAction::ResumeGame,
                enabled: true,
            },
            MenuItem {
                label: "Title Screen".to_string(),
                action: MenuAction::TitleScreen,
                enabled: true,
            },
        ]
    };
    items.push(exit_item);
    Menu::new("Paused", items)
}

/// Construct the settings submenu.
///
/// Shows only settings available on the current platform, with current values.
pub fn settings_menu(settings: &Settings, platform: Platform) -> Menu {
    let mut items = Vec::new();

    if Setting::CasualMode.is_available(platform) {
        let label = if settings.casual_mode {
            "Mode: Casual"
        } else {
            "Mode: Classic"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleCasualMode,
            enabled: true,
        });
    }

    if Setting::ShowExploredPct.is_available(platform) {
        let label = if settings.show_explored_pct {
            "Explored %: On"
        } else {
            "Explored %: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowExploredPct,
            enabled: true,
        });
    }

    if Setting::ShowCoordinates.is_available(platform) {
        let label = if settings.show_coordinates {
            "Coordinates: On"
        } else {
            "Coordinates: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowCoordinates,
            enabled: true,
        });
    }

    if Setting::ShowKeybindHints.is_available(platform) {
        let label = if settings.show_keybind_hints {
            "Keybind Hints: On"
        } else {
            "Keybind Hints: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowKeybindHints,
            enabled: true,
        });
    }

    if Setting::ShowCorpses.is_available(platform) {
        let label = if settings.show_corpses {
            "Show Corpses: On"
        } else {
            "Show Corpses: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowCorpses,
            enabled: true,
        });
    }

    if Setting::ShowKills.is_available(platform) {
        let label = if settings.show_kills {
            "Kill Count: On"
        } else {
            "Kill Count: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowKills,
            enabled: true,
        });
    }

    if Setting::ShowTurnCount.is_available(platform) {
        let label = if settings.show_turn_count {
            "Turn Count: On"
        } else {
            "Turn Count: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleShowTurnCount,
            enabled: true,
        });
    }

    if Setting::ColorPalette.is_available(platform) {
        items.push(MenuItem {
            label: format!("Palette: {}", settings.color_palette.display_name()),
            action: MenuAction::CycleColorPalette,
            enabled: true,
        });
    }

    if Setting::ViKeys.is_available(platform) {
        let label = if settings.vi_keys {
            "Vi Keys: On"
        } else {
            "Vi Keys: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleViKeys,
            enabled: true,
        });
    }

    if Setting::Numpad.is_available(platform) {
        let label = if settings.numpad {
            "Numpad: On"
        } else {
            "Numpad: Off"
        };
        items.push(MenuItem {
            label: label.to_string(),
            action: MenuAction::ToggleNumpad,
            enabled: true,
        });
    }

    if Setting::LeftHandLayout.is_available(platform) {
        items.push(MenuItem {
            label: format!(
                "Left-Hand Keys: {}",
                settings.left_hand_layout.display_name()
            ),
            action: MenuAction::CycleLeftHandLayout,
            enabled: true,
        });
    }

    if Setting::PlayerName.is_available(platform) {
        let name_display = if settings.player_name.is_empty() {
            "Name: (none)".to_string()
        } else {
            format!("Name: {}", settings.player_name)
        };
        items.push(MenuItem {
            label: name_display,
            action: MenuAction::EditPlayerName,
            enabled: true,
        });
    }

    if Setting::Pronouns.is_available(platform) {
        items.push(MenuItem {
            label: format!("Pronouns: {}", settings.pronouns.display_name()),
            action: MenuAction::CyclePronouns,
            enabled: true,
        });
    }

    if Setting::AnimationSpeed.is_available(platform) {
        items.push(MenuItem {
            label: format!("Animation Speed: {}ms", settings.animation_speed_ms),
            action: MenuAction::CycleAnimationSpeed,
            enabled: true,
        });
    }

    if Setting::AutosaveFrequency.is_available(platform) {
        let label = if settings.autosave_frequency == 1 {
            "Autosave: Every Turn".to_string()
        } else {
            format!("Autosave: Every {} Turns", settings.autosave_frequency)
        };
        items.push(MenuItem {
            label,
            action: MenuAction::CycleAutosaveFrequency,
            enabled: true,
        });
    }

    if Setting::MessageLogLines.is_available(platform) {
        items.push(MenuItem {
            label: format!("Message Log: {} Lines", settings.message_log_lines),
            action: MenuAction::CycleMessageLogLines,
            enabled: true,
        });
    }

    items.push(MenuItem {
        label: "Back".to_string(),
        action: MenuAction::Back,
        enabled: true,
    });

    Menu::new("Settings", items)
}

/// Construct a yes/no confirmation dialog.
///
/// The `message` is shown as the menu title (e.g. "Unsaved progress will be
/// lost."). "Yes" maps to `MenuAction::Confirm`; "No" maps to
/// `MenuAction::Back`. Default selection is "No" (index 1) so that an
/// accidental Enter press doesn't confirm.
pub fn confirm_menu(message: &str) -> Menu {
    let mut menu = Menu::new(
        message,
        vec![
            MenuItem {
                label: "Yes".to_string(),
                action: MenuAction::Confirm,
                enabled: true,
            },
            MenuItem {
                label: "No".to_string(),
                action: MenuAction::Back,
                enabled: true,
            },
        ],
    );
    menu.selected = 1;
    menu
}

/// Format a slot label from its metadata.
///
/// Occupied (show_pct=true):  `"Slot 1 — Turn 42, 20/30 HP, 35%"`
/// Occupied (show_pct=false): `"Slot 1 — Turn 42, 20/30 HP"`
/// Empty:                     `"Slot 1 — Empty"`
fn slot_label(index: u8, meta: &Option<SlotMetadata>, show_pct: bool) -> String {
    let num = index + 1;
    match meta {
        Some(m) => {
            let name_part = match &m.player_name {
                Some(name) if !name.is_empty() => format!(" ({})", name),
                _ => String::new(),
            };
            let base = format!(
                "Slot {}{} \u{2014} Turn {}, {}/{} HP",
                num, name_part, m.turn_count, m.player_hp, m.player_max_hp
            );
            if show_pct {
                format!("{}, {}%", base, m.explored_pct)
            } else {
                base
            }
        }
        None => format!("Slot {} \u{2014} Empty", num),
    }
}

/// Construct the save-slot picker (casual mode, from pause menu).
///
/// Shows 5 slots (always enabled — saving to any slot is valid) plus "Back".
pub fn save_slot_menu(slots: &[Option<SlotMetadata>; 5], show_pct: bool) -> Menu {
    let mut items: Vec<MenuItem> = (0..5u8)
        .map(|i| MenuItem {
            label: slot_label(i, &slots[i as usize], show_pct),
            action: MenuAction::SelectSlot(i),
            enabled: true,
        })
        .collect();
    items.push(MenuItem {
        label: "Back".to_string(),
        action: MenuAction::Back,
        enabled: true,
    });
    Menu::new("Save Game", items)
}

/// Construct the load-slot picker (casual mode, from pause or title).
///
/// Shows Autosave (enabled if present) + 5 slots (enabled if occupied) + "Back".
pub fn load_slot_menu(
    has_autosave: bool,
    autosave_meta: &Option<SlotMetadata>,
    slots: &[Option<SlotMetadata>; 5],
    show_pct: bool,
) -> Menu {
    let autosave_label = match autosave_meta {
        Some(m) => {
            let base = format!(
                "Autosave \u{2014} Turn {}, {}/{} HP",
                m.turn_count, m.player_hp, m.player_max_hp
            );
            if show_pct {
                format!("{}, {}%", base, m.explored_pct)
            } else {
                base
            }
        }
        None => "Autosave \u{2014} Empty".to_string(),
    };
    let mut items = vec![MenuItem {
        label: autosave_label,
        action: MenuAction::LoadGame,
        enabled: has_autosave,
    }];
    for i in 0..5u8 {
        items.push(MenuItem {
            label: slot_label(i, &slots[i as usize], show_pct),
            action: MenuAction::SelectSlot(i),
            enabled: slots[i as usize].is_some(),
        });
    }
    items.push(MenuItem {
        label: "Back".to_string(),
        action: MenuAction::Back,
        enabled: true,
    });
    Menu::new("Load Game", items)
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
    fn title_menu_classic_has_expected_items() {
        let menu = title_menu(false, false, Platform::Terminal);
        assert_eq!(menu.title, "R O G U E L I K E");
        assert_eq!(menu.items.len(), 5);
        assert_eq!(menu.items[0].action, MenuAction::LoadGame); // "Continue"
        assert_eq!(menu.items[0].label, "Continue");
        assert_eq!(menu.items[1].action, MenuAction::NewGame);
        assert_eq!(menu.items[2].action, MenuAction::EnterSeed);
        assert_eq!(menu.items[2].label, "Seed");
        assert_eq!(menu.items[3].action, MenuAction::Settings);
        assert_eq!(menu.items[4].action, MenuAction::Quit);
    }

    #[test]
    fn title_menu_casual_has_expected_items() {
        let menu = title_menu(false, true, Platform::Terminal);
        assert_eq!(menu.title, "R O G U E L I K E");
        assert_eq!(menu.items.len(), 5);
        assert_eq!(menu.items[0].action, MenuAction::NewGame);
        assert_eq!(menu.items[1].action, MenuAction::LoadGame);
        assert_eq!(menu.items[1].label, "Load Game");
        assert_eq!(menu.items[2].action, MenuAction::EnterSeed);
        assert_eq!(menu.items[3].action, MenuAction::Settings);
        assert_eq!(menu.items[4].action, MenuAction::Quit);
    }

    #[test]
    fn title_menu_classic_continue_enabled_when_save_exists() {
        let menu = title_menu(true, false, Platform::Terminal);
        assert!(menu.items[0].enabled); // "Continue" enabled
        assert_eq!(menu.items[0].label, "Continue");
    }

    #[test]
    fn pause_menu_classic_has_expected_items() {
        let menu = pause_menu(false, Platform::Terminal);
        assert_eq!(menu.title, "Paused");
        assert_eq!(menu.items.len(), 3);
        assert_eq!(menu.items[0].action, MenuAction::ResumeGame);
        assert_eq!(menu.items[1].action, MenuAction::TitleScreen);
        assert_eq!(menu.items[2].action, MenuAction::Quit);
    }

    #[test]
    fn pause_menu_casual_has_expected_items() {
        let menu = pause_menu(true, Platform::Terminal);
        assert_eq!(menu.title, "Paused");
        assert_eq!(menu.items.len(), 5);
        assert_eq!(menu.items[0].action, MenuAction::ResumeGame);
        assert_eq!(menu.items[1].action, MenuAction::SaveGame);
        assert_eq!(menu.items[2].action, MenuAction::LoadGame);
        assert_eq!(menu.items[3].action, MenuAction::TitleScreen);
        assert_eq!(menu.items[4].action, MenuAction::Quit);
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
    fn draw_left_justifies_items_as_centered_block() {
        let menu = two_item_menu();
        let mut r = MockRenderer::new(80, 24);
        menu.draw(&mut r);

        // Both items are 10 chars ("  Option A" / "  Option B"), so the
        // block x = (80 - 10) / 2 = 35. All items share this x.
        let item_a = r.find_str("Option A").unwrap();
        let item_b = r.find_str("Option B").unwrap();
        assert_eq!(item_a.0, 35);
        assert_eq!(item_b.0, 35);
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
    fn title_menu_casual_with_save_has_load_enabled() {
        let menu = title_menu(true, true, Platform::Terminal);
        let load_item = menu
            .items
            .iter()
            .find(|i| i.action == MenuAction::LoadGame)
            .unwrap();
        assert!(load_item.enabled);
    }

    #[test]
    fn title_menu_casual_without_save_has_load_disabled() {
        let menu = title_menu(false, true, Platform::Terminal);
        let load_item = menu
            .items
            .iter()
            .find(|i| i.action == MenuAction::LoadGame)
            .unwrap();
        assert!(!load_item.enabled);
    }

    #[test]
    fn title_menu_classic_without_save_has_continue_disabled() {
        let menu = title_menu(false, false, Platform::Terminal);
        let continue_item = menu.items.iter().find(|i| i.label == "Continue").unwrap();
        assert!(!continue_item.enabled);
    }

    #[test]
    fn settings_menu_classic_shows_mode() {
        let s = Settings::default();
        let menu = settings_menu(&s, Platform::Terminal);
        assert_eq!(menu.items[0].label, "Mode: Classic");
        assert_eq!(menu.items[0].action, MenuAction::ToggleCasualMode);
        assert_eq!(menu.items[1].label, "Explored %: Off");
        assert_eq!(menu.items[1].action, MenuAction::ToggleShowExploredPct);
        // Last item should be Back.
        assert_eq!(menu.items.last().unwrap().action, MenuAction::Back);
    }

    #[test]
    fn settings_menu_casual_shows_mode() {
        let s = Settings {
            casual_mode: true,
            ..Settings::default()
        };
        let menu = settings_menu(&s, Platform::Terminal);
        assert_eq!(menu.items[0].label, "Mode: Casual");
        assert_eq!(menu.items[0].action, MenuAction::ToggleCasualMode);
    }

    #[test]
    fn settings_menu_explored_pct_on() {
        let s = Settings {
            show_explored_pct: true,
            ..Settings::default()
        };
        let menu = settings_menu(&s, Platform::Terminal);
        assert_eq!(menu.items[1].label, "Explored %: On");
        assert_eq!(menu.items[1].action, MenuAction::ToggleShowExploredPct);
    }

    #[test]
    fn settings_menu_mcp_hides_unavailable() {
        let s = Settings::defaults_for(Platform::Mcp);
        let menu = settings_menu(&s, Platform::Mcp);
        // MCP should not have AnimationSpeed, ViKeys, Numpad, ShowKeybindHints, or ColorPalette.
        let actions: Vec<_> = menu.items.iter().map(|i| i.action).collect();
        assert!(!actions.contains(&MenuAction::CycleAnimationSpeed));
        assert!(!actions.contains(&MenuAction::ToggleViKeys));
        assert!(!actions.contains(&MenuAction::ToggleNumpad));
        assert!(!actions.contains(&MenuAction::ToggleShowKeybindHints));
        assert!(!actions.contains(&MenuAction::CycleColorPalette));
        assert!(!actions.contains(&MenuAction::CycleLeftHandLayout));
        assert!(!actions.contains(&MenuAction::EditPlayerName));
        assert!(!actions.contains(&MenuAction::CyclePronouns));
    }

    #[test]
    fn settings_menu_terminal_shows_all() {
        let s = Settings::default();
        let menu = settings_menu(&s, Platform::Terminal);
        let actions: Vec<_> = menu.items.iter().map(|i| i.action).collect();
        assert!(actions.contains(&MenuAction::ToggleCasualMode));
        assert!(actions.contains(&MenuAction::ToggleShowExploredPct));
        assert!(actions.contains(&MenuAction::ToggleShowCoordinates));
        assert!(actions.contains(&MenuAction::ToggleShowKeybindHints));
        assert!(actions.contains(&MenuAction::ToggleShowCorpses));
        assert!(actions.contains(&MenuAction::CycleColorPalette));
        assert!(actions.contains(&MenuAction::ToggleViKeys));
        assert!(actions.contains(&MenuAction::ToggleNumpad));
        assert!(actions.contains(&MenuAction::CycleLeftHandLayout));
        assert!(actions.contains(&MenuAction::EditPlayerName));
        assert!(actions.contains(&MenuAction::CyclePronouns));
        assert!(actions.contains(&MenuAction::CycleAnimationSpeed));
        assert!(actions.contains(&MenuAction::CycleAutosaveFrequency));
        assert!(actions.contains(&MenuAction::CycleMessageLogLines));
        assert!(actions.contains(&MenuAction::Back));
    }

    #[test]
    fn confirm_menu_defaults_to_no() {
        let menu = confirm_menu("Are you sure?");
        assert_eq!(menu.selected, 1);
        assert_eq!(menu.selected_action(), MenuAction::Back);
    }

    #[test]
    fn confirm_menu_yes_returns_confirm() {
        let mut menu = confirm_menu("Are you sure?");
        menu.selected = 0;
        assert_eq!(
            menu.handle_input(MenuCommand::Select),
            Some(MenuAction::Confirm)
        );
    }

    #[test]
    fn confirm_menu_no_returns_back() {
        let mut menu = confirm_menu("Are you sure?");
        assert_eq!(
            menu.handle_input(MenuCommand::Select),
            Some(MenuAction::Back)
        );
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

    // --- save_slot_menu tests ---

    fn sample_metadata() -> SlotMetadata {
        SlotMetadata {
            turn_count: 42,
            player_hp: 20,
            player_max_hp: 30,
            explored_pct: 35,
            player_name: None,
            depth: 1,
        }
    }

    #[test]
    fn save_slot_menu_shows_five_slots_plus_back() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = save_slot_menu(&slots, false);
        assert_eq!(menu.title, "Save Game");
        assert_eq!(menu.items.len(), 6); // 5 slots + Back
        assert_eq!(menu.items[5].action, MenuAction::Back);
    }

    #[test]
    fn save_slot_menu_all_slots_enabled() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = save_slot_menu(&slots, false);
        for i in 0..5 {
            assert!(menu.items[i].enabled, "Slot {} should be enabled", i);
            assert_eq!(menu.items[i].action, MenuAction::SelectSlot(i as u8));
        }
    }

    #[test]
    fn save_slot_menu_occupied_shows_stats() {
        let mut slots: [Option<SlotMetadata>; 5] = Default::default();
        slots[0] = Some(sample_metadata());
        let menu = save_slot_menu(&slots, true);
        assert!(menu.items[0].label.contains("Turn 42"));
        assert!(menu.items[0].label.contains("20/30 HP"));
        assert!(menu.items[0].label.contains("35%"));
    }

    #[test]
    fn save_slot_menu_occupied_hides_pct_when_off() {
        let mut slots: [Option<SlotMetadata>; 5] = Default::default();
        slots[0] = Some(sample_metadata());
        let menu = save_slot_menu(&slots, false);
        assert!(menu.items[0].label.contains("Turn 42"));
        assert!(menu.items[0].label.contains("20/30 HP"));
        assert!(!menu.items[0].label.contains("35%"));
    }

    #[test]
    fn save_slot_menu_empty_shows_empty() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = save_slot_menu(&slots, false);
        assert!(menu.items[0].label.contains("Empty"));
    }

    // --- load_slot_menu tests ---

    #[test]
    fn load_slot_menu_structure() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = load_slot_menu(false, &None, &slots, false);
        assert_eq!(menu.title, "Load Game");
        assert_eq!(menu.items.len(), 7); // Autosave + 5 slots + Back
        assert_eq!(menu.items[0].action, MenuAction::LoadGame); // Autosave
        assert_eq!(menu.items[6].action, MenuAction::Back);
    }

    #[test]
    fn load_slot_menu_empty_slots_disabled() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = load_slot_menu(false, &None, &slots, false);
        for i in 1..=5 {
            assert!(
                !menu.items[i].enabled,
                "Empty slot {} should be disabled",
                i
            );
        }
    }

    #[test]
    fn load_slot_menu_occupied_slot_enabled() {
        let mut slots: [Option<SlotMetadata>; 5] = Default::default();
        slots[2] = Some(sample_metadata());
        let menu = load_slot_menu(false, &None, &slots, false);
        assert!(menu.items[3].enabled); // index 3 = slot 2 (offset by autosave)
        assert_eq!(menu.items[3].action, MenuAction::SelectSlot(2));
    }

    #[test]
    fn load_slot_menu_autosave_enabled_when_present() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let auto_meta = Some(sample_metadata());
        let menu = load_slot_menu(true, &auto_meta, &slots, true);
        assert!(menu.items[0].enabled);
        assert!(menu.items[0].label.contains("Autosave"));
        assert!(menu.items[0].label.contains("Turn 42"));
        assert!(menu.items[0].label.contains("35%"));
    }

    #[test]
    fn load_slot_menu_autosave_hides_pct_when_off() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let auto_meta = Some(sample_metadata());
        let menu = load_slot_menu(true, &auto_meta, &slots, false);
        assert!(menu.items[0].label.contains("Autosave"));
        assert!(menu.items[0].label.contains("Turn 42"));
        assert!(!menu.items[0].label.contains("35%"));
    }

    #[test]
    fn load_slot_menu_autosave_disabled_when_absent() {
        let slots: [Option<SlotMetadata>; 5] = Default::default();
        let menu = load_slot_menu(false, &None, &slots, false);
        assert!(!menu.items[0].enabled);
    }
}
