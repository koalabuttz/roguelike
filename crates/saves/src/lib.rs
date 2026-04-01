use roguelike_core::saves::SlotMetadata;
use roguelike_core::settings::Settings;

/// Abstraction over save persistence (I/O only).
///
/// Backends handle reading/writing JSON strings and metadata to storage.
/// Serialization/deserialization of game state is the caller's
/// responsibility, via `GameStep::save_to_json()` and
/// `game_step::load_game_from_json()`.
pub trait SaveBackend {
    /// Whether an autosave file exists.
    fn has_autosave(&self) -> bool;

    /// Load the autosave JSON string.
    fn load_autosave_json(&self) -> Result<String, String>;

    /// Write autosave JSON and sidecar metadata.
    fn write_autosave(&self, json: &str, meta: &SlotMetadata);

    /// Delete the autosave and its metadata.
    fn delete_autosave(&self);

    /// Load autosave metadata without loading the full game state.
    fn load_autosave_metadata(&self) -> Option<SlotMetadata>;

    /// Write game JSON and metadata to a numbered slot (0-indexed).
    fn write_slot(&self, json: &str, meta: &SlotMetadata, slot: u8) -> Result<(), String>;

    /// Load JSON from a numbered slot.
    fn load_slot_json(&self, slot: u8) -> Result<String, String>;

    /// Load metadata for all 5 save slots.
    fn load_all_slot_metadata(&self) -> [Option<SlotMetadata>; 5];

    /// Whether any save exists (autosave or any slot).
    fn has_any_save(&self) -> bool;

    /// Whether a save exists for the title screen load button.
    /// Classic mode: only autosave. Casual mode: autosave or any slot.
    fn has_save_for_title(&self, casual_mode: bool) -> bool;

    /// Load user settings, falling back to platform defaults.
    fn load_settings(&self) -> Settings;

    /// Persist user settings.
    fn save_settings(&self, settings: &Settings);
}
