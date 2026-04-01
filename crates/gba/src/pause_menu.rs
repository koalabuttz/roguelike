//! GBA pause menu — START button opens this overlay during gameplay.
//!
//! Wraps the generic [`menu::run_menu`] with pause-specific items and result mapping.

use crate::menu::{MenuConfig, MenuResult};

/// What the player chose from the pause menu.
pub enum PauseResult {
    Resume,
    Help,
    Settings,
    SaveAndQuit,
    TitleScreen,
}

/// Run the pause menu overlay. Blocks until the player picks an action.
#[inline(never)]
pub fn run_pause() -> PauseResult {
    let config = MenuConfig {
        title: "PAUSED",
        items: &["Resume", "Help", "Settings", "Save & Quit", "Title Screen"],
        x: 8,
        y: 4,
        spacing: 2,
        dim_bg0: true,
    };
    match crate::menu::run_menu(&config) {
        MenuResult::Selected(0) | MenuResult::Cancelled => PauseResult::Resume,
        MenuResult::Selected(1) => PauseResult::Help,
        MenuResult::Selected(2) => PauseResult::Settings,
        MenuResult::Selected(3) => PauseResult::SaveAndQuit,
        MenuResult::Selected(_) => PauseResult::TitleScreen,
    }
}
