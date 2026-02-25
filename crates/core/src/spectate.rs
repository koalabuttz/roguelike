use crate::game::GameObservation;
use crate::game_step::GameStep;

/// Trait for delivering rendered frames to external viewers.
///
/// Implementations decide *how* to deliver (file write, network, no-op, etc.).
/// The game loop calls `write_frame` after each turn without caring about the
/// transport. Accepts `&dyn GameStep` so any capability tier can be spectated.
pub trait FrameSink {
    fn write_frame(&self, state: &dyn GameStep);
}

/// A no-op sink that discards every frame.
///
/// Used when spectating is disabled or not configured.
pub struct NullFrameSink;

impl FrameSink for NullFrameSink {
    fn write_frame(&self, _state: &dyn GameStep) {}
}

/// Render a plain-text ASCII frame from a game observation.
///
/// Shows the visible map, a status line, and the last 4 log messages.
/// Works with any tier's observation via `GameStep::observe()`.
pub fn render_frame(obs: &GameObservation) -> String {
    let mut frame = String::new();

    // Map
    for line in &obs.map_ascii {
        frame.push_str(line);
        frame.push('\n');
    }

    // Status line
    let equip = if obs.weapon.is_some() || obs.armor.is_some() {
        format!(" | ATK {} DEF {}", obs.player_atk, obs.player_def)
    } else {
        String::new()
    };
    frame.push_str(&format!(
        "HP {}/{}{} | Depth {}/{} | Turn {} | Kills {} | Explored {}% | Seed {}\n",
        obs.player_hp,
        obs.player_max_hp,
        equip,
        obs.depth,
        obs.target_depth,
        obs.turn_count,
        obs.kills,
        obs.explored_pct,
        obs.seed,
    ));

    // Last 4 messages
    let end = obs.recent_messages.len();
    let start = end.saturating_sub(4);
    let messages = &obs.recent_messages[start..end];
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
    use crate::data;
    use crate::game::GameState;

    fn test_game() -> GameState {
        let gd = data::load_game_data();
        let mut state = GameState::with_data(20, 20, 42, &gd);
        state.update_fov();
        state
    }

    #[test]
    fn render_frame_contains_player() {
        let gs = test_game();
        let obs = gs.observe();
        let frame = render_frame(&obs);
        assert!(frame.contains('@'), "Frame should contain player glyph");
    }

    #[test]
    fn render_frame_contains_hp_line() {
        let gs = test_game();
        let obs = gs.observe();
        let frame = render_frame(&obs);
        assert!(frame.contains("HP 30/30"), "Frame should contain HP status");
        assert!(frame.contains("Seed 42"), "Frame should contain seed");
    }

    #[test]
    fn render_frame_contains_messages() {
        let mut gs = test_game();
        gs.log.add("Test combat message");
        let obs = gs.observe();
        let frame = render_frame(&obs);
        assert!(
            frame.contains("Test combat message"),
            "Frame should contain log messages"
        );
    }

    #[test]
    fn null_frame_sink_is_noop() {
        let sink = NullFrameSink;
        let gs = test_game();
        sink.write_frame(&gs); // should not panic
    }
}
