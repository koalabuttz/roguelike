use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub casual_mode: bool,
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
        let original = Settings { casual_mode: true };
        let json = serde_json::to_string(&original).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert!(loaded.casual_mode);
    }
}
