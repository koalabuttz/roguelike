use roguelike_core::game::GameState;
use roguelike_core::saves::SlotMetadata;
use roguelike_core::settings::Settings;

/// Abstraction over save persistence.
///
/// The terminal backend uses local filesystem paths; the SSH backend
/// uses per-user directories under a shared data dir. Both implement
/// the same logical operations.
pub trait SaveBackend {
    /// Whether an autosave file exists.
    fn has_autosave(&self) -> bool;

    /// Load the autosave game state.
    fn load_autosave(&self) -> Result<GameState, String>;

    /// Write autosave JSON and sidecar metadata.
    fn write_autosave(&self, json: &str, meta: &SlotMetadata);

    /// Delete the autosave and its metadata.
    fn delete_autosave(&self);

    /// Load autosave metadata without loading the full game state.
    fn load_autosave_metadata(&self) -> Option<SlotMetadata>;

    /// Save to a numbered slot (0-indexed). Returns a status message.
    fn save_to_slot(&self, state: &GameState, slot: u8, player_name: &str) -> String;

    /// Load from a numbered slot.
    fn load_from_slot(&self, slot: u8) -> Result<GameState, String>;

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
