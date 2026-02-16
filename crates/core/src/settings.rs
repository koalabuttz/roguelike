use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Terminal,
    Mcp,
    Gba,
    Vita,
    C64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    CasualMode,
    AutosaveFrequency,
    ShowExploredPct,
    ShowCoordinates,
    ShowKeybindHints,
    ShowCorpses,
    MessageLogLines,
    AnimationSpeed,
    ViKeys,
    Numpad,
}

impl Setting {
    pub fn is_available(self, platform: Platform) -> bool {
        match platform {
            Platform::Terminal => true,
            Platform::Mcp => !matches!(
                self,
                Setting::AnimationSpeed
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::ShowKeybindHints
            ),
            Platform::Gba => !matches!(
                self,
                Setting::AutosaveFrequency | Setting::ViKeys | Setting::Numpad
            ),
            Platform::Vita => !matches!(
                self,
                Setting::AutosaveFrequency | Setting::ViKeys | Setting::Numpad
            ),
            Platform::C64 => !matches!(
                self,
                Setting::AutosaveFrequency
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::AnimationSpeed
            ),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_autosave_frequency() -> u32 {
    1
}

fn default_animation_speed_ms() -> u32 {
    50
}

fn default_message_log_lines() -> u8 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub casual_mode: bool,
    #[serde(default)]
    pub show_explored_pct: bool,
    #[serde(default)]
    pub show_coordinates: bool,
    #[serde(default = "default_true")]
    pub show_keybind_hints: bool,
    #[serde(default = "default_true")]
    pub show_corpses: bool,
    #[serde(default = "default_true")]
    pub vi_keys: bool,
    #[serde(default = "default_true")]
    pub numpad: bool,
    #[serde(default = "default_autosave_frequency")]
    pub autosave_frequency: u32,
    #[serde(default = "default_animation_speed_ms")]
    pub animation_speed_ms: u32,
    #[serde(default = "default_message_log_lines")]
    pub message_log_lines: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults_for(Platform::Terminal)
    }
}

impl Settings {
    pub fn defaults_for(platform: Platform) -> Self {
        match platform {
            Platform::Terminal => Settings {
                casual_mode: false,
                show_explored_pct: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_corpses: true,
                vi_keys: true,
                numpad: true,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
            },
            Platform::Mcp => Settings {
                casual_mode: false,
                show_explored_pct: false,
                show_coordinates: false,
                show_keybind_hints: false,
                show_corpses: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 0,
                message_log_lines: 4,
            },
            Platform::Gba => Settings {
                casual_mode: false,
                show_explored_pct: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_corpses: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
            },
            Platform::Vita => Settings {
                casual_mode: false,
                show_explored_pct: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_corpses: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
            },
            Platform::C64 => Settings {
                casual_mode: false,
                show_explored_pct: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_corpses: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 0,
                message_log_lines: 4,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_not_casual() {
        let settings = Settings::default();
        assert!(!settings.casual_mode);
        assert!(!settings.show_explored_pct);
    }

    #[test]
    fn round_trip_serde() {
        let original = Settings {
            casual_mode: true,
            show_explored_pct: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert!(loaded.casual_mode);
        assert!(loaded.show_explored_pct);
    }

    #[test]
    fn missing_show_explored_pct_defaults_false() {
        let json = r#"{"casual_mode": true}"#;
        let loaded: Settings = serde_json::from_str(json).unwrap();
        assert!(loaded.casual_mode);
        assert!(!loaded.show_explored_pct);
    }

    #[test]
    fn terminal_defaults() {
        let s = Settings::defaults_for(Platform::Terminal);
        assert!(!s.casual_mode);
        assert!(!s.show_explored_pct);
        assert!(!s.show_coordinates);
        assert!(s.show_keybind_hints);
        assert!(s.show_corpses);
        assert!(s.vi_keys);
        assert!(s.numpad);
        assert_eq!(s.autosave_frequency, 1);
        assert_eq!(s.animation_speed_ms, 50);
        assert_eq!(s.message_log_lines, 4);
    }

    #[test]
    fn mcp_defaults() {
        let s = Settings::defaults_for(Platform::Mcp);
        assert!(!s.show_keybind_hints);
        assert!(!s.vi_keys);
        assert!(!s.numpad);
        assert_eq!(s.animation_speed_ms, 0);
    }

    #[test]
    fn gba_no_autosave_frequency() {
        assert!(!Setting::AutosaveFrequency.is_available(Platform::Gba));
        assert!(!Setting::ViKeys.is_available(Platform::Gba));
        assert!(!Setting::Numpad.is_available(Platform::Gba));
    }

    #[test]
    fn terminal_all_settings_available() {
        let all = [
            Setting::CasualMode,
            Setting::AutosaveFrequency,
            Setting::ShowExploredPct,
            Setting::ShowCoordinates,
            Setting::ShowKeybindHints,
            Setting::ShowCorpses,
            Setting::MessageLogLines,
            Setting::AnimationSpeed,
            Setting::ViKeys,
            Setting::Numpad,
        ];
        for s in all {
            assert!(
                s.is_available(Platform::Terminal),
                "{s:?} should be available on Terminal"
            );
        }
    }

    #[test]
    fn default_is_terminal() {
        let default = Settings::default();
        let terminal = Settings::defaults_for(Platform::Terminal);
        assert_eq!(default.casual_mode, terminal.casual_mode);
        assert_eq!(default.show_explored_pct, terminal.show_explored_pct);
        assert_eq!(default.show_coordinates, terminal.show_coordinates);
        assert_eq!(default.show_keybind_hints, terminal.show_keybind_hints);
        assert_eq!(default.show_corpses, terminal.show_corpses);
        assert_eq!(default.vi_keys, terminal.vi_keys);
        assert_eq!(default.numpad, terminal.numpad);
        assert_eq!(default.autosave_frequency, terminal.autosave_frequency);
        assert_eq!(default.animation_speed_ms, terminal.animation_speed_ms);
        assert_eq!(default.message_log_lines, terminal.message_log_lines);
    }

    #[test]
    fn vita_defaults() {
        let s = Settings::defaults_for(Platform::Vita);
        assert!(!s.casual_mode);
        assert!(!s.vi_keys);
        assert!(!s.numpad);
        assert_eq!(s.animation_speed_ms, 50);
        assert!(s.show_keybind_hints);
        assert!(s.show_corpses);
    }

    #[test]
    fn vita_no_autosave_vi_numpad() {
        assert!(!Setting::AutosaveFrequency.is_available(Platform::Vita));
        assert!(!Setting::ViKeys.is_available(Platform::Vita));
        assert!(!Setting::Numpad.is_available(Platform::Vita));
        assert!(Setting::AnimationSpeed.is_available(Platform::Vita));
        assert!(Setting::CasualMode.is_available(Platform::Vita));
    }

    #[test]
    fn forward_compatible_deserialization() {
        // Old settings JSON with only casual_mode — new fields should get defaults.
        let json = r#"{"casual_mode": true}"#;
        let loaded: Settings = serde_json::from_str(json).unwrap();
        assert!(loaded.casual_mode);
        assert!(loaded.show_keybind_hints);
        assert!(loaded.vi_keys);
        assert!(loaded.numpad);
        assert_eq!(loaded.autosave_frequency, 1);
        assert_eq!(loaded.animation_speed_ms, 50);
        assert_eq!(loaded.message_log_lines, 4);
    }
}
