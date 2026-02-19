use crate::data::GameData;
use crate::settings::{LeftHandLayout, Settings};

/// Generate help text lines based on current settings and game data.
///
/// Returns a `Vec<String>` suitable for display in `MessageHistoryViewer`.
/// Each string is one line of the help screen.
pub fn help_lines(settings: &Settings, game_data: &GameData) -> Vec<String> {
    let mut lines = vec![
        "=== CONTROLS ===".to_string(),
        String::new(),
        // Always-available controls
        "Arrow keys    Move / attack".to_string(),
        "Shift+Arrow   Autorun (keep moving until obstacle)".to_string(),
        ".             Wait one turn".to_string(),
        "o             Auto-explore".to_string(),
        "x / Tab       Look mode (examine tiles)".to_string(),
        "Ctrl+P        Message history".to_string(),
        "?             This help screen".to_string(),
        "q / Esc       Pause menu".to_string(),
    ];

    // Vi keys
    if settings.vi_keys {
        lines.push(String::new());
        lines.push("--- Vi Keys ---".to_string());
        lines.push("h j k l       Move W/S/N/E".to_string());
        lines.push("y u b n       Move NW/NE/SW/SE".to_string());
        lines.push("Uppercase     Autorun in that direction".to_string());
    }

    // Numpad
    if settings.numpad {
        lines.push(String::new());
        lines.push("--- Numpad ---".to_string());
        lines.push("7 8 9         NW / N / NE".to_string());
        lines.push("4   6         W  /   / E".to_string());
        lines.push("1 2 3         SW / S / SE".to_string());
        lines.push("5             Wait".to_string());
        lines.push("Shift+digit   Autorun".to_string());
    }

    // Left-hand layouts
    match settings.left_hand_layout {
        LeftHandLayout::Off => {}
        LeftHandLayout::Qweasdzxc | LeftHandLayout::Weasdzxcr => {
            lines.push(String::new());
            lines.push(format!(
                "--- Left-Hand: {} ---",
                settings.left_hand_layout.display_name()
            ));
            lines.push("q w e         NW / N / NE".to_string());
            lines.push("a   d         W  /   / E".to_string());
            lines.push("z x c         SW / S / SE".to_string());
            lines.push("s             Wait".to_string());
            lines.push("Uppercase     Autorun".to_string());
            lines.push("Note: q=NW (use Esc for pause), x=S (use Tab for look)".to_string());
        }
    }

    // Combat mechanics
    lines.push(String::new());
    lines.push("=== COMBAT ===".to_string());
    lines.push(String::new());
    lines.push("Damage = ATK - DEF (minimum 0)".to_string());
    lines.push("Walk into a monster to attack it.".to_string());
    lines.push(format!(
        "HP regenerates 1 point every {} turns.",
        game_data.config.regen_interval
    ));

    // Player stats
    lines.push(String::new());
    lines.push(format!(
        "Player: {} HP, {} ATK, {} DEF",
        game_data.player.hp, game_data.player.attack, game_data.player.defense
    ));

    // Monster table
    lines.push(String::new());
    lines.push("=== MONSTERS ===".to_string());
    lines.push(String::new());
    lines.push(format!(
        "{:<2} {:<12} {:>3} {:>4} {:>4} {:>5}",
        "", "Name", "HP", "ATK", "DEF", "Sight"
    ));
    for m in &game_data.monsters {
        lines.push(format!(
            "{:<2} {:<12} {:>3} {:>4} {:>4} {:>5}",
            m.glyph, m.name, m.hp, m.attack, m.defense, m.sight_radius
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    fn test_game_data() -> GameData {
        data::defaults().clone()
    }

    #[test]
    fn help_lines_not_empty() {
        let settings = Settings::default();
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(!lines.is_empty());
    }

    #[test]
    fn help_lines_includes_combat_formula() {
        let settings = Settings::default();
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(lines.iter().any(|l| l.contains("ATK - DEF")));
    }

    #[test]
    fn help_lines_includes_monster_names() {
        let settings = Settings::default();
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        for m in &gd.monsters {
            assert!(
                lines.iter().any(|l| l.contains(&m.name)),
                "Missing monster: {}",
                m.name
            );
        }
    }

    #[test]
    fn help_lines_includes_vi_section_when_enabled() {
        let settings = Settings {
            vi_keys: true,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(lines.iter().any(|l| l.contains("Vi Keys")));
    }

    #[test]
    fn help_lines_excludes_vi_section_when_disabled() {
        let settings = Settings {
            vi_keys: false,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(!lines.iter().any(|l| l.contains("Vi Keys")));
    }

    #[test]
    fn help_lines_includes_numpad_section_when_enabled() {
        let settings = Settings {
            numpad: true,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(lines.iter().any(|l| l.contains("Numpad")));
    }

    #[test]
    fn help_lines_excludes_numpad_section_when_disabled() {
        let settings = Settings {
            numpad: false,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(!lines.iter().any(|l| l.contains("Numpad")));
    }

    #[test]
    fn help_lines_includes_left_hand_section_when_active() {
        let settings = Settings {
            left_hand_layout: LeftHandLayout::Qweasdzxc,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(lines.iter().any(|l| l.contains("Left-Hand")));
    }

    #[test]
    fn help_lines_excludes_left_hand_section_when_off() {
        let settings = Settings {
            left_hand_layout: LeftHandLayout::Off,
            ..Settings::default()
        };
        let gd = test_game_data();
        let lines = help_lines(&settings, &gd);
        assert!(!lines.iter().any(|l| l.contains("Left-Hand")));
    }
}
