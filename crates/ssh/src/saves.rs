use std::path::{Path, PathBuf};

use roguelike_core::game::GameState;
use roguelike_core::saves::SlotMetadata;
use roguelike_core::settings::Settings;

const SAVE_FILE: &str = "savegame.json";
const AUTOSAVE_META_FILE: &str = "savegame.meta.json";
const SETTINGS_FILE: &str = "settings.json";
const NUM_SLOTS: u8 = 5;

/// Per-user save directory manager.
///
/// Each user gets their own directory under `{data_dir}/saves/{username}/`
/// containing autosave, slot saves, and settings.
pub struct SaveManager {
    user_dir: PathBuf,
}

impl SaveManager {
    pub fn new(data_dir: &Path, username: &str) -> Self {
        let user_dir = data_dir.join("saves").join(username);
        let _ = std::fs::create_dir_all(&user_dir);
        Self { user_dir }
    }

    fn save_path(&self) -> PathBuf {
        self.user_dir.join(SAVE_FILE)
    }

    fn autosave_meta_path(&self) -> PathBuf {
        self.user_dir.join(AUTOSAVE_META_FILE)
    }

    fn settings_path(&self) -> PathBuf {
        self.user_dir.join(SETTINGS_FILE)
    }

    fn slot_save_path(&self, slot: u8) -> PathBuf {
        self.user_dir.join(format!("savegame_{}.json", slot + 1))
    }

    fn slot_meta_path(&self, slot: u8) -> PathBuf {
        self.user_dir
            .join(format!("savegame_{}.meta.json", slot + 1))
    }

    // --- Autosave ---

    pub fn has_autosave(&self) -> bool {
        self.save_path().exists()
    }

    pub fn write_autosave(&self, json: &str, meta: &SlotMetadata) {
        let _ = std::fs::write(self.save_path(), json);
        if let Ok(meta_json) = serde_json::to_string(meta) {
            let _ = std::fs::write(self.autosave_meta_path(), meta_json);
        }
    }

    pub fn load_autosave(&self) -> Result<GameState, String> {
        let json =
            std::fs::read_to_string(self.save_path()).map_err(|e| format!("Load failed: {e}"))?;
        GameState::load_from_json(&json).map_err(|e| format!("Load failed: {e}"))
    }

    pub fn load_autosave_metadata(&self) -> Option<SlotMetadata> {
        std::fs::read_to_string(self.autosave_meta_path())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn delete_autosave(&self) {
        let _ = std::fs::remove_file(self.save_path());
        let _ = std::fs::remove_file(self.autosave_meta_path());
    }

    // --- Slot saves ---

    pub fn save_to_slot(&self, state: &GameState, slot: u8, player_name: &str) -> String {
        match state.save_to_json() {
            Ok(json) => match std::fs::write(self.slot_save_path(slot), json) {
                Ok(()) => {
                    let mut meta = state.extract_metadata();
                    if !player_name.is_empty() {
                        meta.player_name = Some(player_name.to_string());
                    }
                    if let Ok(meta_json) = serde_json::to_string(&meta) {
                        let _ = std::fs::write(self.slot_meta_path(slot), meta_json);
                    }
                    "Game saved.".to_string()
                }
                Err(e) => format!("Save failed: {e}"),
            },
            Err(e) => format!("Save failed: {e}"),
        }
    }

    pub fn load_from_slot(&self, slot: u8) -> Result<GameState, String> {
        let json = std::fs::read_to_string(self.slot_save_path(slot))
            .map_err(|e| format!("Load failed: {e}"))?;
        GameState::load_from_json(&json).map_err(|e| format!("Load failed: {e}"))
    }

    pub fn load_slot_metadata(&self, slot: u8) -> Option<SlotMetadata> {
        std::fs::read_to_string(self.slot_meta_path(slot))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn load_all_slot_metadata(&self) -> [Option<SlotMetadata>; 5] {
        [
            self.load_slot_metadata(0),
            self.load_slot_metadata(1),
            self.load_slot_metadata(2),
            self.load_slot_metadata(3),
            self.load_slot_metadata(4),
        ]
    }

    /// Check if any save exists: autosave or any slot.
    pub fn has_any_save(&self) -> bool {
        if self.has_autosave() {
            return true;
        }
        (0..NUM_SLOTS).any(|i| self.slot_save_path(i).exists())
    }

    pub fn has_save_for_title(&self, casual_mode: bool) -> bool {
        if casual_mode {
            self.has_any_save()
        } else {
            self.has_autosave()
        }
    }

    // --- Settings ---

    pub fn load_settings(&self) -> Settings {
        std::fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_else(|| Settings::defaults_for(roguelike_core::settings::Platform::Ssh))
    }

    pub fn save_settings(&self, settings: &Settings) {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(self.settings_path(), json);
        }
    }
}

impl roguelike_saves::SaveBackend for SaveManager {
    fn has_autosave(&self) -> bool {
        self.has_autosave()
    }
    fn load_autosave(&self) -> Result<GameState, String> {
        self.load_autosave()
    }
    fn write_autosave(&self, json: &str, meta: &SlotMetadata) {
        self.write_autosave(json, meta);
    }
    fn delete_autosave(&self) {
        self.delete_autosave();
    }
    fn load_autosave_metadata(&self) -> Option<SlotMetadata> {
        self.load_autosave_metadata()
    }
    fn save_to_slot(&self, state: &GameState, slot: u8, player_name: &str) -> String {
        self.save_to_slot(state, slot, player_name)
    }
    fn load_from_slot(&self, slot: u8) -> Result<GameState, String> {
        self.load_from_slot(slot)
    }
    fn load_all_slot_metadata(&self) -> [Option<SlotMetadata>; 5] {
        self.load_all_slot_metadata()
    }
    fn has_any_save(&self) -> bool {
        self.has_any_save()
    }
    fn has_save_for_title(&self, casual_mode: bool) -> bool {
        self.has_save_for_title(casual_mode)
    }
    fn load_settings(&self) -> Settings {
        self.load_settings()
    }
    fn save_settings(&self, settings: &Settings) {
        self.save_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_manager(username: &str) -> (SaveManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let manager = SaveManager::new(dir.path(), username);
        (manager, dir)
    }

    #[test]
    fn new_creates_directory() {
        let (manager, _dir) = temp_manager("alice");
        assert!(manager.user_dir.exists());
    }

    #[test]
    fn no_save_initially() {
        let (manager, _dir) = temp_manager("alice");
        assert!(!manager.has_autosave());
        assert!(!manager.has_any_save());
    }

    #[test]
    fn settings_defaults_to_ssh_platform() {
        let (manager, _dir) = temp_manager("alice");
        let settings = manager.load_settings();
        // SSH defaults match Terminal defaults
        assert!(settings.vi_keys);
        assert!(settings.numpad);
    }

    #[test]
    fn settings_roundtrip() {
        let (manager, _dir) = temp_manager("alice");
        let mut settings = Settings::defaults_for(roguelike_core::settings::Platform::Ssh);
        settings.casual_mode = true;
        manager.save_settings(&settings);
        let loaded = manager.load_settings();
        assert!(loaded.casual_mode);
    }
}
