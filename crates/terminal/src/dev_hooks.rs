#[cfg(all(debug_assertions, feature = "dev-tools"))]
mod inner {
    use std::io::{self, Write};

    use crossterm::event::{KeyCode, KeyEvent};

    use roguelike_core::command::GameCommand;
    use roguelike_core::data::GameData;
    use roguelike_core::dev_tools::{self, DevCommand, DevSession, OverlayLayer};
    use roguelike_core::game::{GameState, LookOptions};
    use roguelike_core::settings::ColorPalette;

    use roguelike_tui::game_loop::DevHooks;
    use roguelike_tui::render;

    /// Terminal dev-tools hooks backed by a `DevSession`.
    pub struct TerminalDevHooks {
        pub session: DevSession,
    }

    impl TerminalDevHooks {
        pub fn new() -> Self {
            Self {
                session: DevSession::default(),
            }
        }
    }

    impl DevHooks for TerminalDevHooks {
        fn handle_dev_key(
            &mut self,
            key: KeyEvent,
            state: &mut GameState,
            game_data: &mut GameData,
        ) -> bool {
            let session = &mut self.session;

            // Overlay cursor mode: arrow keys move cursor, Esc exits.
            if session.overlay_cursor.is_some() {
                let handled = match key.code {
                    KeyCode::Up => {
                        session.overlay_cursor.as_mut().unwrap().1 -= 1;
                        true
                    }
                    KeyCode::Down => {
                        session.overlay_cursor.as_mut().unwrap().1 += 1;
                        true
                    }
                    KeyCode::Left => {
                        session.overlay_cursor.as_mut().unwrap().0 -= 1;
                        true
                    }
                    KeyCode::Right => {
                        session.overlay_cursor.as_mut().unwrap().0 += 1;
                        true
                    }
                    KeyCode::Esc => {
                        session.overlay_cursor = None;
                        state.log.add("Pathfinding overlay: frontier mode.");
                        true
                    }
                    _ => false,
                };
                if handled {
                    return true;
                }
            }

            // Monster FOV cursor mode.
            if session.monster_fov_cursor.is_some() {
                let handled = match key.code {
                    KeyCode::Up => {
                        session.monster_fov_cursor.as_mut().unwrap().1 -= 1;
                        true
                    }
                    KeyCode::Down => {
                        session.monster_fov_cursor.as_mut().unwrap().1 += 1;
                        true
                    }
                    KeyCode::Left => {
                        session.monster_fov_cursor.as_mut().unwrap().0 -= 1;
                        true
                    }
                    KeyCode::Right => {
                        session.monster_fov_cursor.as_mut().unwrap().0 += 1;
                        true
                    }
                    KeyCode::Esc => {
                        session.monster_fov_cursor = None;
                        state.log.add("Monster FOV overlay: union mode.");
                        true
                    }
                    _ => false,
                };
                if handled {
                    return true;
                }
            }

            // F-key dev commands.
            let dev_cmd = match key.code {
                KeyCode::F(1) => Some(DevCommand::DumpStats),
                KeyCode::F(2) => Some(DevCommand::ToggleFov),
                KeyCode::F(3) => Some(DevCommand::ToggleGodMode),
                KeyCode::F(4) => Some(DevCommand::RevealMap),
                KeyCode::F(5) => Some(DevCommand::KillAll),
                KeyCode::F(6) => Some(DevCommand::ToggleOverlay(OverlayLayer::Fov)),
                KeyCode::F(7) => Some(DevCommand::ToggleOverlay(OverlayLayer::MonsterTargets)),
                KeyCode::F(8) => Some(DevCommand::ToggleOverlay(OverlayLayer::Pathfinding)),
                KeyCode::F(9) => Some(DevCommand::ToggleOverlay(OverlayLayer::Frontiers)),
                KeyCode::F(10) => Some(DevCommand::ReloadData),
                KeyCode::F(11) => Some(DevCommand::ToggleOverlay(OverlayLayer::RevealMonsters)),
                KeyCode::F(12) => Some(DevCommand::ToggleOverlay(OverlayLayer::MonsterFov)),
                _ => None,
            };

            if let Some(cmd) = dev_cmd {
                let is_reload = matches!(cmd, DevCommand::ReloadData);
                let msg = dev_tools::exec_dev(state, session, cmd);
                state.log.add(&msg);
                if is_reload && let Some(ref d) = session.game_data {
                    *game_data = d.clone();
                }
                return true;
            }

            false
        }

        fn after_step(&mut self, state: &mut GameState, cmd: GameCommand) {
            dev_tools::after_step(state, &mut self.session, cmd);
        }

        fn fov_disabled(&self) -> bool {
            self.session.fov_disabled
        }

        fn apply_fov_override(&self, state: &mut GameState) {
            dev_tools::apply_fov_override(state);
        }

        fn look_options(&self) -> LookOptions {
            LookOptions {
                reveal_monsters: self.session.overlay_flags & (1 << 4) != 0,
            }
        }

        fn render_overlay<W2: Write>(
            &self,
            w: &mut W2,
            state: &GameState,
            pal: ColorPalette,
        ) -> io::Result<()> {
            if self.session.overlay_flags != 0 {
                let cells = dev_tools::compute_overlay(state, &self.session);
                render::render_overlay(w, &cells, pal)?;
            }
            Ok(())
        }
    }
}

#[cfg(all(debug_assertions, feature = "dev-tools"))]
pub use inner::TerminalDevHooks;
