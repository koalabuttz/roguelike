use crate::game::GameState;

/// Trait for delivering rendered frames to external viewers.
///
/// Implementations decide *how* to deliver (file write, network, no-op, etc.).
/// The game loop calls `write_frame` after each turn without caring about the
/// transport.
pub trait FrameSink {
    fn write_frame(&self, state: &GameState);
}

/// A no-op sink that discards every frame.
///
/// Used when spectating is disabled or not configured.
pub struct NullFrameSink;

impl FrameSink for NullFrameSink {
    fn write_frame(&self, _state: &GameState) {}
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
    use crate::entity::Entity;
    use crate::fov;
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
            seed: 42,
            preset: None,
            dirty: false,
            regen_interval: crate::data::config().regen_interval,
            max_autorun_steps: crate::data::config().max_autorun_steps,
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
    fn null_frame_sink_is_noop() {
        let sink = NullFrameSink;
        let gs = test_game();
        sink.write_frame(&gs); // should not panic
    }
}
