use crate::optimize::MemoryAreas;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub always_on_top: bool,
    /// Launch silently to the notification area through a highest-privilege logon task.
    pub run_at_startup: bool,
    pub close_to_notification_area: bool,
    pub memory_areas: u32,
    /// Show a Windows toast when memory cleanup completes.
    pub show_optimization_notifications: bool,
    /// UI language: "auto", "zh-CN", or "en".
    pub language: String,
    /// Write debug output to `App.log` next to the executable.
    pub debug_logging: bool,
    /// Enable global cleanup hotkey via `RegisterHotKey`.
    pub cleanup_hotkey_enabled: bool,
    /// Hotkey chord, e.g. `Ctrl+Alt+C`. Empty disables registration.
    pub cleanup_hotkey: String,
    /// Process base names excluded from Working Set cleanup (lowercase, no `.exe`).
    pub excluded_processes: Vec<String>,
    /// Enable cleanup when Windows reports low physical memory.
    pub auto_cleanup_enabled: bool,
    /// Physical memory usage percent (0–100) that triggers automatic cleanup.
    /// `0` disables the threshold trigger.
    pub auto_cleanup_threshold: u32,
    /// Minutes between scheduled automatic cleanups. `0` disables the interval trigger.
    pub auto_cleanup_interval_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_on_top: false,
            run_at_startup: false,
            close_to_notification_area: true,
            show_optimization_notifications: true,
            memory_areas: MemoryAreas::DEFAULT.bits(),
            language: "auto".into(),
            debug_logging: false,
            cleanup_hotkey_enabled: true,
            cleanup_hotkey: crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP.into(),
            excluded_processes: Vec::new(),
            auto_cleanup_enabled: false,
            auto_cleanup_threshold: 0,
            auto_cleanup_interval_minutes: 0,
        }
    }
}

impl Settings {
    fn config_dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MemoryCleaner")
    }

    fn config_path() -> PathBuf {
        Self::config_dir().join("settings.toml")
    }

    fn ensure_config_dir() {
        let _ = std::fs::create_dir_all(Self::config_dir());
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut settings = match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(settings) => settings,
                Err(e) => {
                    crate::log_msg(&format!("Failed to parse {}: {e}", path.display()));
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                crate::log_msg(&format!("Failed to read {}: {e}", path.display()));
                Self::default()
            }
        };
        settings.normalize_memory_areas();
        settings.normalize_language();
        settings.normalize_cleanup_hotkey();
        settings.normalize_excluded_processes();
        settings.normalize_auto_cleanup();
        settings
    }

    fn normalize_language(&mut self) {
        if !matches!(self.language.as_str(), "auto" | "zh-CN" | "en") {
            self.language = "auto".into();
        }
    }

    /// Clamp auto-cleanup trigger inputs to their valid ranges. The threshold is a
    /// usage percent; the interval is minutes. `0` disables each trigger.
    fn normalize_auto_cleanup(&mut self) {
        self.auto_cleanup_threshold = self.auto_cleanup_threshold.min(100);
    }

    fn normalize_cleanup_hotkey(&mut self) {
        if !self.cleanup_hotkey_enabled {
            return;
        }
        if crate::win32::hotkey::HotkeyBinding::parse(&self.cleanup_hotkey).is_none() {
            self.cleanup_hotkey = crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP.into();
        }
    }

    fn normalize_memory_areas(&mut self) {
        let mut areas = MemoryAreas::from_bits_truncate(self.memory_areas);
        if areas.contains(MemoryAreas::STANDBY_LIST)
            && areas.contains(MemoryAreas::STANDBY_LIST_LOW_PRIORITY)
        {
            areas.remove(MemoryAreas::STANDBY_LIST_LOW_PRIORITY);
            self.memory_areas = areas.bits();
        }
    }

    fn normalize_excluded_processes(&mut self) {
        let mut normalized: Vec<String> = self
            .excluded_processes
            .iter()
            .map(|name| crate::win32::process::normalize_process_name(name))
            .filter(|name| !name.is_empty())
            .collect();
        normalized.sort();
        normalized.dedup();
        self.excluded_processes = normalized;
    }

    #[cfg(test)]
    pub(crate) fn from_toml(content: &str) -> Self {
        let mut settings: Settings = toml::from_str(content).expect("valid settings toml");
        settings.normalize_memory_areas();
        settings.normalize_language();
        settings.normalize_cleanup_hotkey();
        settings.normalize_excluded_processes();
        settings.normalize_auto_cleanup();
        settings
    }

    pub fn save(&self) {
        Self::ensure_config_dir();
        let Ok(content) = toml::to_string_pretty(self) else {
            crate::log_msg("[settings] failed to serialize config");
            return;
        };
        let final_path = Self::config_path();
        let tmp_path = final_path.with_extension("toml.tmp");
        if let Err(e) = std::fs::write(&tmp_path, &content) {
            crate::log_msg(&format!(
                "[settings] failed to write {}: {e}",
                tmp_path.display()
            ));
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
            crate::log_msg(&format!(
                "[settings] failed to rename {} -> {}: {e}",
                tmp_path.display(),
                final_path.display()
            ));
            let _ = std::fs::remove_file(&tmp_path);
        }
    }

    pub fn memory_areas(&self) -> MemoryAreas {
        let mut areas = MemoryAreas::from_bits_truncate(self.memory_areas);
        if areas.contains(MemoryAreas::STANDBY_LIST)
            && areas.contains(MemoryAreas::STANDBY_LIST_LOW_PRIORITY)
        {
            areas.remove(MemoryAreas::STANDBY_LIST_LOW_PRIORITY);
        }
        areas
    }

    /// Returns the effective locale string for `rust_i18n::set_locale`.
    pub fn effective_locale(&self) -> &'static str {
        match self.language.as_str() {
            "zh-CN" => "zh-CN",
            "en" => "en",
            _ => crate::win32::os::system_ui_locale(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::MemoryAreas;

    #[test]
    fn default_settings_match_documented_values() {
        let settings = Settings::default();
        assert!(settings.close_to_notification_area);
        assert!(!settings.run_at_startup);
        assert!(settings.show_optimization_notifications);
        assert_eq!(settings.memory_areas, MemoryAreas::DEFAULT.bits());
        assert_eq!(settings.language, "auto");
        assert!(settings.cleanup_hotkey_enabled);
        assert_eq!(
            settings.cleanup_hotkey,
            crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP
        );
        assert!(!settings.auto_cleanup_enabled);
        assert_eq!(settings.auto_cleanup_threshold, 0);
        assert_eq!(settings.auto_cleanup_interval_minutes, 0);
    }

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let original = Settings {
            always_on_top: true,
            debug_logging: true,
            language: "en".into(),
            memory_areas: MemoryAreas::WORKING_SET.bits(),
            auto_cleanup_enabled: true,
            ..Default::default()
        };
        let restored: Settings = toml::from_str(&toml::to_string(&original).unwrap()).unwrap();
        assert!(restored.always_on_top);
        assert!(restored.debug_logging);
        assert_eq!(restored.language, "en");
        assert_eq!(restored.memory_areas, MemoryAreas::WORKING_SET.bits());
        assert!(restored.auto_cleanup_enabled);
    }

    #[test]
    fn auto_cleanup_schedule_fields_are_parsed() {
        let settings = Settings::from_toml(
            "auto_cleanup_enabled = true\nauto_cleanup_threshold = 90\nauto_cleanup_interval_minutes = 30",
        );
        assert!(settings.auto_cleanup_enabled);
        assert_eq!(settings.auto_cleanup_threshold, 90);
        assert_eq!(settings.auto_cleanup_interval_minutes, 30);
    }

    #[test]
    fn auto_cleanup_defaults_disable_triggers() {
        let settings = Settings::default();
        assert_eq!(settings.auto_cleanup_threshold, 0);
        assert_eq!(settings.auto_cleanup_interval_minutes, 0);
    }

    #[test]
    fn auto_cleanup_threshold_is_clamped_to_100() {
        let settings = Settings::from_toml("auto_cleanup_threshold = 150");
        assert_eq!(settings.auto_cleanup_threshold, 100);
    }

    #[test]
    fn auto_cleanup_fields_roundtrip() {
        let original = Settings {
            auto_cleanup_enabled: true,
            auto_cleanup_threshold: 85,
            auto_cleanup_interval_minutes: 60,
            ..Default::default()
        };
        let restored: Settings = toml::from_str(&toml::to_string(&original).unwrap()).unwrap();
        assert_eq!(restored.auto_cleanup_threshold, 85);
        assert_eq!(restored.auto_cleanup_interval_minutes, 60);
    }

    #[test]
    fn effective_locale_resolves_explicit_values() {
        let settings = Settings {
            language: "en".into(),
            ..Default::default()
        };
        assert_eq!(settings.effective_locale(), "en");
        let settings = Settings {
            language: "zh-CN".into(),
            ..Default::default()
        };
        assert_eq!(settings.effective_locale(), "zh-CN");
    }

    #[test]
    fn effective_locale_auto_returns_supported_locale() {
        let settings = Settings::default();
        assert!(matches!(settings.effective_locale(), "zh-CN" | "en"));
    }

    #[test]
    fn normalize_language_resets_unknown_values() {
        let settings = Settings::from_toml("language = \"fr\"");
        assert_eq!(settings.language, "auto");
    }

    #[test]
    fn standby_areas_are_mutually_exclusive_after_normalize() {
        let both = MemoryAreas::STANDBY_LIST.bits() | MemoryAreas::STANDBY_LIST_LOW_PRIORITY.bits();
        let settings = Settings::from_toml(&format!("memory_areas = {both}"));
        let areas = settings.memory_areas();
        assert!(areas.contains(MemoryAreas::STANDBY_LIST));
        assert!(!areas.contains(MemoryAreas::STANDBY_LIST_LOW_PRIORITY));
    }

    #[test]
    fn normalize_excluded_processes_dedupes_and_strips_exe() {
        let settings =
            Settings::from_toml(r#"excluded_processes = ["Chrome.exe", "chrome", "  Firefox  "]"#);
        assert_eq!(
            settings.excluded_processes,
            vec!["chrome".to_string(), "firefox".to_string()]
        );
    }
}
