use std::path::PathBuf;

use roguelike_core::game::GameState;

/// Writes plain-text ASCII frames to a file for external viewers.
///
/// Opt-in: set `ROGUELIKE_SPECTATE_PATH` to a file path to enable.
/// Disabled by default (no env var or empty string).
/// Uses atomic write (write tmp + rename) to prevent partial reads.
pub struct SpectatorWriter {
    pub(crate) path: Option<PathBuf>,
}

impl SpectatorWriter {
    pub fn new() -> Self {
        let path = match std::env::var("ROGUELIKE_SPECTATE_PATH") {
            Ok(val) if val.is_empty() => None,
            Ok(val) => Some(PathBuf::from(val)),
            Err(_) => None,
        };
        SpectatorWriter { path }
    }

    /// Write a rendered frame to the spectator file.
    ///
    /// Does nothing if spectating is disabled. Errors are silently ignored
    /// (spectating is best-effort and must never break the MCP server).
    pub fn write_frame(&self, state: &GameState) {
        if let Some(ref path) = self.path {
            let frame = render_frame(state);
            let tmp_path = path.with_extension("tmp");
            if std::fs::write(&tmp_path, &frame).is_ok() {
                let _ = std::fs::rename(&tmp_path, path);
            }
        }
    }
}

/// Render a plain-text ASCII frame from the game state.
///
/// Shows the full explored map (with entities in FOV, frontiers as `~`),
/// a status line, and the last 4 log messages.
pub fn render_frame(state: &GameState) -> String {
    let map_lines = state.explored_map();
    let player = &state.entities[0];
    let kills = state.kill_count();
    let explored_pct = state.explored_pct();

    let mut frame = String::new();

    // Map
    for line in &map_lines {
        frame.push_str(line);
        frame.push('\n');
    }

    // Status line
    frame.push_str(&format!(
        "HP {}/{} | Turn {} | Kills {} | Explored {}% | Seed {}\n",
        player.hp, player.max_hp, state.turn_count, kills, explored_pct, state.seed,
    ));

    // Last 4 messages
    let messages = state.log.recent(4);
    if !messages.is_empty() {
        for msg in messages {
            frame.push_str(msg);
            frame.push('\n');
        }
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use roguelike_core::entity::Entity;
    use roguelike_core::fov;
    use roguelike_core::map::{Map, Tile};
    use roguelike_core::message_log::MessageLog;

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
            seed: 42,
            dirty: false,
            regen_interval: roguelike_core::data::config().regen_interval,
            max_autorun_steps: roguelike_core::data::config().max_autorun_steps,
        }
    }

    #[test]
    fn render_frame_contains_player() {
        let gs = test_game();
        let frame = render_frame(&gs);
        assert!(frame.contains('@'), "Frame should contain player glyph");
    }

    #[test]
    fn render_frame_contains_hp_line() {
        let gs = test_game();
        let frame = render_frame(&gs);
        assert!(frame.contains("HP 30/30"), "Frame should contain HP status");
        assert!(frame.contains("Seed 42"), "Frame should contain seed");
    }

    #[test]
    fn render_frame_contains_messages() {
        let mut gs = test_game();
        gs.log.add("Test combat message");
        let frame = render_frame(&gs);
        assert!(
            frame.contains("Test combat message"),
            "Frame should contain log messages"
        );
    }

    #[test]
    fn spectator_writer_disabled() {
        let writer = SpectatorWriter { path: None };
        // write_frame should be a no-op when disabled
        let gs = test_game();
        writer.write_frame(&gs); // should not panic
    }

    #[test]
    fn spectator_writer_writes_file() {
        let gs = test_game();
        let path = PathBuf::from("/tmp/roguelike-spectate-test.txt");
        let writer = SpectatorWriter {
            path: Some(path.clone()),
        };
        writer.write_frame(&gs);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains('@'));
        assert!(contents.contains("HP 30/30"));
        // Clean up
        let _ = std::fs::remove_file(&path);
    }
}
