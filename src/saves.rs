use serde::{Deserialize, Serialize};

/// Lightweight metadata for a save slot, stored as a sidecar `.meta.json` file.
///
/// This allows the menu to display slot info (turn, HP, exploration %) without
/// deserializing the full game state. All fields are value types — no platform
/// dependencies, no file I/O.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotMetadata {
    pub turn_count: i32,
    pub player_hp: i32,
    pub player_max_hp: i32,
    pub explored_pct: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serde() {
        let original = SlotMetadata {
            turn_count: 42,
            player_hp: 20,
            player_max_hp: 30,
            explored_pct: 35,
        };
        let json = serde_json::to_string(&original).unwrap();
        let loaded: SlotMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.turn_count, 42);
        assert_eq!(loaded.player_hp, 20);
        assert_eq!(loaded.player_max_hp, 30);
        assert_eq!(loaded.explored_pct, 35);
    }

    #[test]
    fn deserialize_handles_extra_fields() {
        // Forward compatibility: a future version might add fields.
        // The current struct should still deserialize fine, ignoring extras.
        let json = r#"{
            "turn_count": 10,
            "player_hp": 25,
            "player_max_hp": 30,
            "explored_pct": 50,
            "new_future_field": "hello"
        }"#;
        let meta: SlotMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.turn_count, 10);
        assert_eq!(meta.player_hp, 25);
    }
}
