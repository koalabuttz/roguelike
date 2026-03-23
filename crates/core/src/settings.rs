use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Terminal,
    Ssh,
    Mcp,
    Gba,
    Vita,
    C64,
}

/// Color palette for accessibility (colorblind modes).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorPalette {
    #[default]
    Default,
    Protanopia,
    Deuteranopia,
    HighContrast,
}

impl ColorPalette {
    pub const ALL: [ColorPalette; 4] = [
        ColorPalette::Default,
        ColorPalette::Protanopia,
        ColorPalette::Deuteranopia,
        ColorPalette::HighContrast,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            ColorPalette::Default => "Default",
            ColorPalette::Protanopia => "Protanopia",
            ColorPalette::Deuteranopia => "Deuteranopia",
            ColorPalette::HighContrast => "High Contrast",
        }
    }

    pub fn next(self) -> ColorPalette {
        match self {
            ColorPalette::Default => ColorPalette::Protanopia,
            ColorPalette::Protanopia => ColorPalette::Deuteranopia,
            ColorPalette::Deuteranopia => ColorPalette::HighContrast,
            ColorPalette::HighContrast => ColorPalette::Default,
        }
    }
}

/// Left-hand keyboard layout for one-handed play.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftHandLayout {
    #[default]
    Off,
    Qweasdzxc,
    Weasdzxcr,
}

impl LeftHandLayout {
    pub fn display_name(self) -> &'static str {
        match self {
            LeftHandLayout::Off => "Off",
            LeftHandLayout::Qweasdzxc => "QWEASDZXC",
            LeftHandLayout::Weasdzxcr => "WEASDZXCR",
        }
    }

    pub fn next(self) -> LeftHandLayout {
        match self {
            LeftHandLayout::Off => LeftHandLayout::Qweasdzxc,
            LeftHandLayout::Qweasdzxc => LeftHandLayout::Weasdzxcr,
            LeftHandLayout::Weasdzxcr => LeftHandLayout::Off,
        }
    }
}

/// Player pronoun options.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pronouns {
    #[default]
    TheyThem,
    HeHim,
    SheHer,
    ItIts,
}

impl Pronouns {
    pub fn display_name(self) -> &'static str {
        match self {
            Pronouns::TheyThem => "They/Them",
            Pronouns::HeHim => "He/Him",
            Pronouns::SheHer => "She/Her",
            Pronouns::ItIts => "It/Its",
        }
    }

    pub fn next(self) -> Pronouns {
        match self {
            Pronouns::TheyThem => Pronouns::HeHim,
            Pronouns::HeHim => Pronouns::SheHer,
            Pronouns::SheHer => Pronouns::ItIts,
            Pronouns::ItIts => Pronouns::TheyThem,
        }
    }

    pub fn subject(self) -> &'static str {
        match self {
            Pronouns::TheyThem => "they",
            Pronouns::HeHim => "he",
            Pronouns::SheHer => "she",
            Pronouns::ItIts => "it",
        }
    }

    pub fn was_were(self) -> &'static str {
        match self {
            Pronouns::TheyThem => "were",
            Pronouns::HeHim => "was",
            Pronouns::SheHer => "was",
            Pronouns::ItIts => "was",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    CasualMode,
    AutosaveFrequency,
    ShowCoordinates,
    ShowKeybindHints,
    ShowKills,
    ShowTurnCount,
    MessageLogLines,
    AnimationSpeed,
    ViKeys,
    Numpad,
    ColorPalette,
    LeftHandLayout,
    PlayerName,
    Pronouns,
    AutoPickup,
}

impl Setting {
    pub fn is_available(self, platform: Platform) -> bool {
        match platform {
            Platform::Terminal | Platform::Ssh => true,
            Platform::Mcp => !matches!(
                self,
                Setting::AnimationSpeed
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::ShowKeybindHints
                    | Setting::ColorPalette
                    | Setting::LeftHandLayout
                    | Setting::PlayerName
                    | Setting::Pronouns
            ),
            Platform::Gba => !matches!(
                self,
                Setting::AutosaveFrequency
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::LeftHandLayout
                    | Setting::PlayerName
                    | Setting::Pronouns
                    | Setting::AutoPickup
            ),
            Platform::Vita => !matches!(
                self,
                Setting::AutosaveFrequency
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::LeftHandLayout
                    | Setting::PlayerName
                    | Setting::Pronouns
                    | Setting::AutoPickup
            ),
            Platform::C64 => !matches!(
                self,
                Setting::AutosaveFrequency
                    | Setting::ViKeys
                    | Setting::Numpad
                    | Setting::AnimationSpeed
                    | Setting::LeftHandLayout
                    | Setting::PlayerName
                    | Setting::Pronouns
                    | Setting::ShowKills
                    | Setting::ShowTurnCount
                    | Setting::AutoPickup
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

fn default_palette() -> ColorPalette {
    ColorPalette::Default
}

fn default_left_hand_layout() -> LeftHandLayout {
    LeftHandLayout::Off
}

fn default_pronouns() -> Pronouns {
    Pronouns::TheyThem
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub casual_mode: bool,
    #[serde(default)]
    pub auto_pickup: bool,
    #[serde(default)]
    pub show_coordinates: bool,
    #[serde(default = "default_true")]
    pub show_keybind_hints: bool,
    #[serde(default = "default_true")]
    pub show_kills: bool,
    #[serde(default = "default_true")]
    pub show_turn_count: bool,
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
    #[serde(default = "default_palette")]
    pub color_palette: ColorPalette,
    #[serde(default = "default_left_hand_layout")]
    pub left_hand_layout: LeftHandLayout,
    #[serde(default)]
    pub player_name: String,
    #[serde(default = "default_pronouns")]
    pub pronouns: Pronouns,
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults_for(Platform::Terminal)
    }
}

impl Settings {
    pub fn defaults_for(platform: Platform) -> Self {
        match platform {
            Platform::Terminal | Platform::Ssh => Settings {
                casual_mode: false,
                auto_pickup: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_kills: true,
                show_turn_count: true,
                vi_keys: true,
                numpad: true,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
                color_palette: ColorPalette::Default,
                left_hand_layout: LeftHandLayout::Off,
                player_name: String::new(),
                pronouns: Pronouns::TheyThem,
            },
            Platform::Mcp => Settings {
                casual_mode: false,
                auto_pickup: false,
                show_coordinates: false,
                show_keybind_hints: false,
                show_kills: true,
                show_turn_count: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 0,
                message_log_lines: 4,
                color_palette: ColorPalette::Default,
                left_hand_layout: LeftHandLayout::Off,
                player_name: String::new(),
                pronouns: Pronouns::TheyThem,
            },
            Platform::Gba => Settings {
                casual_mode: false,
                auto_pickup: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_kills: true,
                show_turn_count: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
                color_palette: ColorPalette::Default,
                left_hand_layout: LeftHandLayout::Off,
                player_name: String::new(),
                pronouns: Pronouns::TheyThem,
            },
            Platform::Vita => Settings {
                casual_mode: false,
                auto_pickup: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_kills: true,
                show_turn_count: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 50,
                message_log_lines: 4,
                color_palette: ColorPalette::Default,
                left_hand_layout: LeftHandLayout::Off,
                player_name: String::new(),
                pronouns: Pronouns::TheyThem,
            },
            Platform::C64 => Settings {
                casual_mode: false,
                auto_pickup: false,
                show_coordinates: false,
                show_keybind_hints: true,
                show_kills: true,
                show_turn_count: true,
                vi_keys: false,
                numpad: false,
                autosave_frequency: 1,
                animation_speed_ms: 0,
                message_log_lines: 4,
                color_palette: ColorPalette::Default,
                left_hand_layout: LeftHandLayout::Off,
                player_name: String::new(),
                pronouns: Pronouns::TheyThem,
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
    }

    #[test]
    fn round_trip_serde() {
        let original = Settings {
            casual_mode: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert!(loaded.casual_mode);
    }

    #[test]
    fn terminal_defaults() {
        let s = Settings::defaults_for(Platform::Terminal);
        assert!(!s.casual_mode);
        assert!(!s.show_coordinates);
        assert!(s.show_keybind_hints);
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
            Setting::ShowCoordinates,
            Setting::ShowKeybindHints,
            Setting::MessageLogLines,
            Setting::AnimationSpeed,
            Setting::ViKeys,
            Setting::Numpad,
            Setting::ColorPalette,
            Setting::LeftHandLayout,
            Setting::PlayerName,
            Setting::Pronouns,
            Setting::AutoPickup,
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
        assert_eq!(default.show_coordinates, terminal.show_coordinates);
        assert_eq!(default.show_keybind_hints, terminal.show_keybind_hints);
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
        assert_eq!(loaded.color_palette, ColorPalette::Default);
    }

    #[test]
    fn color_palette_default_value() {
        let s = Settings::default();
        assert_eq!(s.color_palette, ColorPalette::Default);
    }

    #[test]
    fn color_palette_cycle_order() {
        assert_eq!(ColorPalette::Default.next(), ColorPalette::Protanopia);
        assert_eq!(ColorPalette::Protanopia.next(), ColorPalette::Deuteranopia);
        assert_eq!(
            ColorPalette::Deuteranopia.next(),
            ColorPalette::HighContrast
        );
        assert_eq!(ColorPalette::HighContrast.next(), ColorPalette::Default);
    }

    #[test]
    fn color_palette_display_names() {
        assert_eq!(ColorPalette::Default.display_name(), "Default");
        assert_eq!(ColorPalette::Protanopia.display_name(), "Protanopia");
        assert_eq!(ColorPalette::Deuteranopia.display_name(), "Deuteranopia");
        assert_eq!(ColorPalette::HighContrast.display_name(), "High Contrast");
    }

    #[test]
    fn color_palette_all_has_correct_length() {
        assert_eq!(ColorPalette::ALL.len(), 4);
    }

    #[test]
    fn color_palette_serde_roundtrip() {
        let original = Settings {
            color_palette: ColorPalette::Protanopia,
            ..Settings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.color_palette, ColorPalette::Protanopia);
    }

    #[test]
    fn pronouns_cycle() {
        assert_eq!(Pronouns::TheyThem.next(), Pronouns::HeHim);
        assert_eq!(Pronouns::HeHim.next(), Pronouns::SheHer);
        assert_eq!(Pronouns::SheHer.next(), Pronouns::ItIts);
        assert_eq!(Pronouns::ItIts.next(), Pronouns::TheyThem);
    }

    #[test]
    fn pronouns_display_names() {
        assert_eq!(Pronouns::TheyThem.display_name(), "They/Them");
        assert_eq!(Pronouns::HeHim.display_name(), "He/Him");
        assert_eq!(Pronouns::SheHer.display_name(), "She/Her");
        assert_eq!(Pronouns::ItIts.display_name(), "It/Its");
    }

    #[test]
    fn pronouns_subject() {
        assert_eq!(Pronouns::TheyThem.subject(), "they");
        assert_eq!(Pronouns::HeHim.subject(), "he");
        assert_eq!(Pronouns::SheHer.subject(), "she");
        assert_eq!(Pronouns::ItIts.subject(), "it");
    }

    #[test]
    fn pronouns_was_were() {
        assert_eq!(Pronouns::TheyThem.was_were(), "were");
        assert_eq!(Pronouns::HeHim.was_were(), "was");
        assert_eq!(Pronouns::SheHer.was_were(), "was");
        assert_eq!(Pronouns::ItIts.was_were(), "was");
    }

    #[test]
    fn pronouns_not_available_on_mcp() {
        assert!(!Setting::Pronouns.is_available(Platform::Mcp));
        assert!(!Setting::PlayerName.is_available(Platform::Mcp));
    }

    #[test]
    fn pronouns_available_on_terminal() {
        assert!(Setting::Pronouns.is_available(Platform::Terminal));
        assert!(Setting::PlayerName.is_available(Platform::Terminal));
    }

    #[test]
    fn left_hand_layout_cycle() {
        assert_eq!(LeftHandLayout::Off.next(), LeftHandLayout::Qweasdzxc);
        assert_eq!(LeftHandLayout::Qweasdzxc.next(), LeftHandLayout::Weasdzxcr);
        assert_eq!(LeftHandLayout::Weasdzxcr.next(), LeftHandLayout::Off);
    }

    #[test]
    fn left_hand_layout_display_names() {
        assert_eq!(LeftHandLayout::Off.display_name(), "Off");
        assert_eq!(LeftHandLayout::Qweasdzxc.display_name(), "QWEASDZXC");
        assert_eq!(LeftHandLayout::Weasdzxcr.display_name(), "WEASDZXCR");
    }

    #[test]
    fn left_hand_layout_not_available_on_mcp() {
        assert!(!Setting::LeftHandLayout.is_available(Platform::Mcp));
    }

    #[test]
    fn left_hand_layout_not_available_on_gba_vita_c64() {
        assert!(!Setting::LeftHandLayout.is_available(Platform::Gba));
        assert!(!Setting::LeftHandLayout.is_available(Platform::Vita));
        assert!(!Setting::LeftHandLayout.is_available(Platform::C64));
    }

    #[test]
    fn left_hand_layout_available_on_terminal() {
        assert!(Setting::LeftHandLayout.is_available(Platform::Terminal));
    }

    #[test]
    fn ssh_defaults_match_terminal() {
        let ssh = Settings::defaults_for(Platform::Ssh);
        let terminal = Settings::defaults_for(Platform::Terminal);
        assert_eq!(ssh.casual_mode, terminal.casual_mode);
        assert_eq!(ssh.vi_keys, terminal.vi_keys);
        assert_eq!(ssh.numpad, terminal.numpad);
        assert_eq!(ssh.animation_speed_ms, terminal.animation_speed_ms);
        assert_eq!(ssh.message_log_lines, terminal.message_log_lines);
    }

    #[test]
    fn ssh_all_settings_available() {
        let all = [
            Setting::CasualMode,
            Setting::AutosaveFrequency,
            Setting::ShowCoordinates,
            Setting::ShowKeybindHints,
            Setting::MessageLogLines,
            Setting::AnimationSpeed,
            Setting::ViKeys,
            Setting::Numpad,
            Setting::ColorPalette,
            Setting::LeftHandLayout,
            Setting::PlayerName,
            Setting::Pronouns,
        ];
        for s in all {
            assert!(
                s.is_available(Platform::Ssh),
                "{s:?} should be available on Ssh"
            );
        }
    }

    #[test]
    fn color_palette_not_available_on_mcp() {
        assert!(!Setting::ColorPalette.is_available(Platform::Mcp));
    }

    #[test]
    fn color_palette_available_on_terminal_gba_vita_c64() {
        assert!(Setting::ColorPalette.is_available(Platform::Terminal));
        assert!(Setting::ColorPalette.is_available(Platform::Gba));
        assert!(Setting::ColorPalette.is_available(Platform::Vita));
        assert!(Setting::ColorPalette.is_available(Platform::C64));
    }
}
