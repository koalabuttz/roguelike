use std::path::PathBuf;

use roguelike_core::game::GameObservation;
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
    fn write_frame(&self, obs: &GameObservation) {
        if let Some(ref path) = self.path {
            let frame = render_frame(obs);
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
    use roguelike_core::data;
    use roguelike_core::game::GameState;

    fn test_game() -> GameState {
        let gd = data::load_game_data();
        let mut state = GameState::with_data(20, 20, 42, &gd);
        state.update_fov();
        state
    }

    #[test]
    fn spectator_writer_disabled() {
        let writer = FileFrameSink { path: None };
        let gs = test_game();
        let obs = gs.observe();
        writer.write_frame(&obs); // should not panic
    }

    #[test]
    fn spectator_writer_writes_file() {
        let gs = test_game();
        let obs = gs.observe();
        let path = std::env::temp_dir().join("roguelike-spectate-test.txt");
        let writer = FileFrameSink {
            path: Some(path.clone()),
        };
        writer.write_frame(&obs);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains('@'));
        assert!(contents.contains("HP 30/30"));
        // Clean up
        let _ = std::fs::remove_file(&path);
    }
}
