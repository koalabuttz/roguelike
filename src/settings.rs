use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub casual_mode: bool,
    #[serde(default)]
    pub show_explored_pct: bool,
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
}
