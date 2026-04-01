use roguelike_core::saves::SlotMetadata;
use roguelike_core::settings::Settings;

use roguelike_saves::SaveBackend;

const SAVE_FILE: &str = "savegame.json";
const AUTOSAVE_META_FILE: &str = "savegame.meta.json";
const SETTINGS_FILE: &str = "settings.json";
const NUM_SLOTS: u8 = 5;

/// Local filesystem save backend for the terminal binary.
pub struct LocalSaveBackend;

fn slot_save_path(slot: u8) -> String {
    format!("savegame_{}.json", slot + 1)
}

fn slot_meta_path(slot: u8) -> String {
    format!("savegame_{}.meta.json", slot + 1)
}

fn write_metadata(path: &str, meta: &SlotMetadata) {
    if let Ok(json) = serde_json::to_string(meta) {
        let _ = std::fs::write(path, json);
    }
}

impl SaveBackend for LocalSaveBackend {
    fn has_autosave(&self) -> bool {
        std::path::Path::new(SAVE_FILE).exists()
    }

    fn load_autosave_json(&self) -> Result<String, String> {
        std::fs::read_to_string(SAVE_FILE).map_err(|e| format!("Load failed: {e}"))
    }

    fn write_autosave(&self, json: &str, meta: &SlotMetadata) {
        let _ = std::fs::write(SAVE_FILE, json);
        write_metadata(AUTOSAVE_META_FILE, meta);
    }

    fn delete_autosave(&self) {
        let _ = std::fs::remove_file(SAVE_FILE);
        let _ = std::fs::remove_file(AUTOSAVE_META_FILE);
    }

    fn load_autosave_metadata(&self) -> Option<SlotMetadata> {
        std::fs::read_to_string(AUTOSAVE_META_FILE)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn write_slot(&self, json: &str, meta: &SlotMetadata, slot: u8) -> Result<(), String> {
        std::fs::write(slot_save_path(slot), json).map_err(|e| format!("Save failed: {e}"))?;
        write_metadata(&slot_meta_path(slot), meta);
        Ok(())
    }

    fn load_slot_json(&self, slot: u8) -> Result<String, String> {
        std::fs::read_to_string(slot_save_path(slot)).map_err(|e| format!("Load failed: {e}"))
    }

    fn load_all_slot_metadata(&self) -> [Option<SlotMetadata>; 5] {
        [
            self.load_slot_metadata(0),
            self.load_slot_metadata(1),
            self.load_slot_metadata(2),
            self.load_slot_metadata(3),
            self.load_slot_metadata(4),
        ]
    }

    fn has_any_save(&self) -> bool {
        if self.has_autosave() {
            return true;
        }
        (0..NUM_SLOTS).any(|i| std::path::Path::new(&slot_save_path(i)).exists())
    }

    fn has_save_for_title(&self, casual_mode: bool) -> bool {
        if casual_mode {
            self.has_any_save()
        } else {
            self.has_autosave()
        }
    }

    fn load_settings(&self) -> Settings {
        std::fs::read_to_string(SETTINGS_FILE)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    fn save_settings(&self, settings: &Settings) {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(SETTINGS_FILE, json);
        }
    }
}

impl LocalSaveBackend {
    fn load_slot_metadata(&self, slot: u8) -> Option<SlotMetadata> {
        std::fs::read_to_string(slot_meta_path(slot))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }
}
