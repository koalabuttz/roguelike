use std::path::PathBuf;

use roguelike_core::game::GameState;
use roguelike_core::spectate::{FrameSink, render_frame};

/// Writes plain-text ASCII frames to a file for external viewers.
///
/// Opt-in: set `ROGUELIKE_SPECTATE_PATH` to a file path to enable.
/// Disabled by default (no env var or empty string).
/// Uses atomic write (write tmp + rename) to prevent partial reads.
pub struct FileFrameSink {
    pub(crate) path: Option<PathBuf>,
}

impl Default for FileFrameSink {
    fn default() -> Self {
        Self::new()
    }
}

impl FileFrameSink {
    pub fn new() -> Self {
        let path = match std::env::var("ROGUELIKE_SPECTATE_PATH") {
            Ok(val) if val.is_empty() => None,
            Ok(val) => Some(PathBuf::from(val)),
            Err(_) => None,
        };
        FileFrameSink { path }
    }
}

impl FrameSink for FileFrameSink {
    /// Write a rendered frame to the spectator file.
    ///
    /// Does nothing if spectating is disabled. Errors are silently ignored
    /// (spectating is best-effort and must never break the MCP server).
    fn write_frame(&self, state: &GameState) {
        if let Some(ref path) = self.path {
            let frame = render_frame(state);
            let tmp_path = path.with_extension("tmp");
            if std::fs::write(&tmp_path, &frame).is_ok() {
                let _ = std::fs::rename(&tmp_path, path);
            }
        }
    }
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
            preset: None,
            dirty: false,
            regen_interval: roguelike_core::data::config().regen_interval,
            max_autorun_steps: roguelike_core::data::config().max_autorun_steps,
            wandering_seed: 0,
            wandering_config: Default::default(),
            idle_count: 0,
            wandering_spawned: 0,
            wandering_spawn_table: Vec::new(),
        }
    }

    #[test]
    fn spectator_writer_disabled() {
        let writer = FileFrameSink { path: None };
        // write_frame should be a no-op when disabled
        let gs = test_game();
        writer.write_frame(&gs); // should not panic
    }

    #[test]
    fn spectator_writer_writes_file() {
        let gs = test_game();
        let path = std::env::temp_dir().join("roguelike-spectate-test.txt");
        let writer = FileFrameSink {
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
