use crate::optimize::MemoryAreas;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
#[cfg(windows)]
use windows::core::PCWSTR;

const REMOVED_REGISTRY_CACHE_BIT: u32 = 1 << 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 经最高权限登录任务静默启动到通知区域。
    pub run_at_startup: bool,
    pub memory_areas: u32,
    /// 内存清理完成时弹出 Windows 通知。
    pub show_optimization_notifications: bool,
    /// 界面语言："auto"、"zh-CN" 或 "en"。
    pub language: String,
    /// 将调试输出写入可执行文件旁的 `App.log`。
    pub debug_logging: bool,
    /// 通过 `RegisterHotKey` 启用全局清理热键。
    pub cleanup_hotkey_enabled: bool,
    /// 热键组合，如 `Ctrl+Alt+C`。空字符串表示不注册。
    pub cleanup_hotkey: String,
    /// Windows 报告物理内存不足时启用清理。
    pub auto_cleanup_enabled: bool,
    /// 触发自动清理的物理内存占用百分比（0–100）。
    /// `0` 关闭阈值触发。
    pub auto_cleanup_threshold: u32,
    /// 在系统任务栏显示占用信息窗口（TrafficMonitor 同类嵌入/贴靠）。
    pub show_taskbar_window: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            run_at_startup: false,
            show_optimization_notifications: true,
            memory_areas: MemoryAreas::DEFAULT.bits(),
            language: "auto".into(),
            debug_logging: false,
            cleanup_hotkey_enabled: true,
            cleanup_hotkey: crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP.into(),
            auto_cleanup_enabled: false,
            auto_cleanup_threshold: 0,
            show_taskbar_window: true,
        }
    }
}

impl Settings {
    #[cfg(windows)]
    fn replace_config_file(tmp: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
        let tmp_w: Vec<u16> = tmp
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dest_w: Vec<u16> = dest
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            MoveFileExW(
                PCWSTR(tmp_w.as_ptr()),
                PCWSTR(dest_w.as_ptr()),
                MOVEFILE_REPLACE_EXISTING,
            )
        }
        .map_err(std::io::Error::other)
    }

    #[cfg(not(windows))]
    fn replace_config_file(tmp: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
        std::fs::rename(tmp, dest)
    }

    pub fn config_dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MemoryCleaner")
    }

    fn config_path() -> PathBuf {
        Self::config_dir().join("settings.toml")
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
        settings.normalize();
        settings
    }

    fn normalize(&mut self) {
        self.normalize_memory_areas();
        self.normalize_language();
        self.normalize_cleanup_hotkey();
        self.normalize_auto_cleanup();
    }
    fn normalize_language(&mut self) {
        if !matches!(self.language.as_str(), "auto" | "zh-CN" | "en") {
            self.language = "auto".into();
        }
    }

    /// 将自动清理阈值钳到合法占用百分比范围。
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
        self.memory_areas &= !REMOVED_REGISTRY_CACHE_BIT;
        let known_areas = MemoryAreas::from_bits_truncate(self.memory_areas);
        if known_areas.contains(MemoryAreas::STANDBY_LIST)
            && known_areas.contains(MemoryAreas::STANDBY_LIST_LOW_PRIORITY)
        {
            self.memory_areas &= !MemoryAreas::STANDBY_LIST_LOW_PRIORITY.bits();
        }
    }

    #[cfg(test)]
    pub(crate) fn from_toml(content: &str) -> Self {
        let mut settings: Settings = toml::from_str(content).expect("valid settings toml");
        settings.normalize();
        settings
    }

    pub fn save(&self) {
        if let Err(error) = self.try_save() {
            crate::log_msg(&format!("[settings] save failed: {error}"));
        }
    }

    pub fn try_save(&self) -> std::io::Result<()> {
        static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = SAVE_LOCK
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::create_dir_all(Self::config_dir())?;
        let content = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let final_path = Self::config_path();
        let tmp_path = final_path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)?;
        if let Err(error) = Self::replace_config_file(&tmp_path, &final_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(error);
        }
        Ok(())
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

    /// 返回供 `rust_i18n::set_locale` 使用的有效语言字符串。
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
        assert!(!settings.run_at_startup);
        assert!(settings.show_optimization_notifications);
        assert_eq!(settings.memory_areas, MemoryAreas::DEFAULT.bits());
        assert_eq!(settings.memory_areas, 43);
        assert_eq!(settings.language, "auto");
        assert!(settings.cleanup_hotkey_enabled);
        assert_eq!(
            settings.cleanup_hotkey,
            crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP
        );
        assert!(!settings.auto_cleanup_enabled);
        assert_eq!(settings.auto_cleanup_threshold, 0);
    }

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let original = Settings {
            debug_logging: true,
            language: "en".into(),
            memory_areas: MemoryAreas::WORKING_SET.bits(),
            auto_cleanup_enabled: true,
            ..Default::default()
        };
        let restored: Settings = toml::from_str(&toml::to_string(&original).unwrap()).unwrap();
        assert!(restored.debug_logging);
        assert_eq!(restored.language, "en");
        assert_eq!(restored.memory_areas, MemoryAreas::WORKING_SET.bits());
        assert!(restored.auto_cleanup_enabled);
    }

    #[test]
    fn legacy_fields_are_ignored_on_load() {
        let settings = Settings::from_toml(
            r#"
always_on_top = true
close_to_notification_area = false
auto_cleanup_enabled = true
auto_cleanup_threshold = 90
auto_cleanup_interval_minutes = 30
[[cleanup_history]]
completed_at_unix_secs = 1
source = "manual"
selected_areas = 43
completed_count = 4
failed_count = 0
duration_millis = 1
memory_load_before = 50
memory_load_after = 49
available_before = 1
available_after = 2
"#,
        );
        assert!(settings.auto_cleanup_enabled);
        assert_eq!(settings.auto_cleanup_threshold, 90);
        let serialized = toml::to_string(&settings).expect("settings serialize");
        assert!(!serialized.contains("always_on_top"));
        assert!(!serialized.contains("close_to_notification_area"));
        assert!(!serialized.contains("cleanup_history"));
        assert!(!serialized.contains("auto_cleanup_interval_minutes"));
    }

    #[test]
    fn auto_cleanup_threshold_is_clamped_to_100() {
        let settings = Settings::from_toml("auto_cleanup_threshold = 150");
        assert_eq!(settings.auto_cleanup_threshold, 100);
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
    fn removed_memory_area_bits_are_discarded() {
        let removed_registry_cache = 1u32 << 7;
        let stored = MemoryAreas::WORKING_SET.bits() | removed_registry_cache;
        let settings = Settings::from_toml(&format!("memory_areas = {stored}"));
        assert_eq!(settings.memory_areas, MemoryAreas::WORKING_SET.bits());
    }

    #[test]
    fn unknown_memory_area_bits_are_preserved() {
        let future_area = 1u32 << 31;
        let stored = MemoryAreas::WORKING_SET.bits() | future_area;
        let settings = Settings::from_toml(&format!("memory_areas = {stored}"));
        assert_eq!(settings.memory_areas, stored);
        assert_eq!(settings.memory_areas(), MemoryAreas::WORKING_SET);
    }
}
