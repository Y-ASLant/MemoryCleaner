use crate::optimize::MemoryAreas;
use serde::{Deserialize, Deserializer, Serialize};
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
    /// 在系统任务栏显示占用信息窗口（嵌入或贴靠）。
    pub show_taskbar_window: bool,
    /// 任务栏指标条字体磅值（整数 pt；`lfHeight = -MulDiv(pt, dpi, 72)`）。
    #[serde(deserialize_with = "deserialize_font_point_size")]
    pub taskbar_font_point_size: u32,
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
            auto_cleanup_enabled: true,
            auto_cleanup_threshold: 0,
            show_taskbar_window: true,
            taskbar_font_point_size: TASKBAR_FONT_PT_DEFAULT,
        }
    }
}

/// 任务栏字体：整数磅值 5–72，默认 10。
pub const TASKBAR_FONT_PT_DEFAULT: u32 = 10;
pub const TASKBAR_FONT_PT_MIN: u32 = 5;
pub const TASKBAR_FONT_PT_MAX: u32 = 72;

pub(crate) fn normalize_font_point_size(pt: u32) -> u32 {
    pt.clamp(TASKBAR_FONT_PT_MIN, TASKBAR_FONT_PT_MAX)
}

/// 接受整数或小数 toml（小数四舍五入），兼容旧一位小数写法。
fn deserialize_font_point_size<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    use serde::de::{self, Visitor};
    use std::fmt;

    struct FontPt;
    impl<'de> Visitor<'de> for FontPt {
        type Value = u32;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an integer or float font point size")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u32, E> {
            Ok(v.min(u64::from(u32::MAX)) as u32)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u32, E> {
            if v <= 0 {
                Ok(0)
            } else {
                Ok(v.min(i64::from(u32::MAX)) as u32)
            }
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<u32, E> {
            if !v.is_finite() || v <= 0.0 {
                return Ok(TASKBAR_FONT_PT_DEFAULT);
            }
            Ok(v.round().clamp(0.0, f64::from(u32::MAX)) as u32)
        }
    }

    deserializer.deserialize_any(FontPt)
}

impl Settings {
    /// 落盘文件头标记：用于判断是否需重写中英注释。
    const TOML_COMMENT_MARKER: &str = "Memory Cleaner Lite — settings.toml";

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
            .join(crate::version::CONFIG_DIR_NAME)
    }

    fn config_path() -> PathBuf {
        Self::config_dir().join("settings.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let (mut settings, seed_file) = match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<Settings>(&content) {
                Ok(settings) => {
                    // 缺键或无标准注释头：写回完整带注释配置，方便手改。
                    let seed = Self::toml_needs_rewrite(&content);
                    (settings, seed)
                }
                Err(e) => {
                    crate::log_msg(&format!("Failed to parse {}: {e}", path.display()));
                    (Self::default(), true)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), true),
            Err(e) => {
                crate::log_msg(&format!("Failed to read {}: {e}", path.display()));
                (Self::default(), false)
            }
        };
        settings.normalize();
        if seed_file && let Err(error) = settings.try_save() {
            crate::log_msg(&format!("[settings] seed failed: {error}"));
        }
        settings
    }

    /// 已知配置键是否都在文件顶层出现（忽略未知键）。
    fn toml_missing_known_keys(content: &str) -> bool {
        const KEYS: &[&str] = &[
            "run_at_startup",
            "memory_areas",
            "show_optimization_notifications",
            "language",
            "debug_logging",
            "cleanup_hotkey_enabled",
            "cleanup_hotkey",
            "auto_cleanup_enabled",
            "auto_cleanup_threshold",
            "show_taskbar_window",
            "taskbar_font_point_size",
        ];
        let Ok(table) = content.parse::<toml::Table>() else {
            return true;
        };
        KEYS.iter().any(|key| !table.contains_key(*key))
    }

    fn toml_needs_rewrite(content: &str) -> bool {
        Self::toml_missing_known_keys(content) || !content.contains(Self::TOML_COMMENT_MARKER)
    }

    /// 落盘用：带中英注释的完整 settings.toml 文本。
    fn to_toml_documented(&self) -> String {
        let mut out = String::with_capacity(1600);
        out.push_str("# ");
        out.push_str(Self::TOML_COMMENT_MARKER);
        out.push('\n');
        out.push_str(
            "# 每项：上一行中文说明，下一行 English。托盘改设置会整文件覆盖并保留本注释格式。\n\
# Per key: Chinese line, then English. Tray edits rewrite the whole file (comments kept).\n\
# 仅 toml 可改且需重启：cleanup_hotkey、taskbar_font_point_size。\n\
# TOML-only (restart required): cleanup_hotkey, taskbar_font_point_size.\n\n",
        );

        fn push_entry(out: &mut String, zh: &str, en: &str, key: &str, value: &str) {
            out.push_str("# ");
            out.push_str(zh);
            out.push('\n');
            out.push_str("# ");
            out.push_str(en);
            out.push('\n');
            out.push_str(key);
            out.push_str(" = ");
            out.push_str(value);
            out.push_str("\n\n");
        }

        let b = |v: bool| if v { "true" } else { "false" };
        let s = |v: &str| toml::Value::String(v.to_string()).to_string();

        push_entry(
            &mut out,
            "登录后最高权限计划任务，仅托盘启动",
            "Highest-privilege logon task; tray only",
            "run_at_startup",
            b(self.run_at_startup),
        );
        push_entry(
            &mut out,
            "清理区域位掩码（见 README；待机列表两项互斥）",
            "Cleanup area bitmask (see README; standby variants mutually exclusive)",
            "memory_areas",
            &self.memory_areas.to_string(),
        );
        push_entry(
            &mut out,
            "清理完成时弹出 Windows Toast 通知",
            "Windows toast when cleanup finishes",
            "show_optimization_notifications",
            b(self.show_optimization_notifications),
        );
        push_entry(
            &mut out,
            "界面语言：auto / zh-CN / en（auto 跟系统 UI）",
            "UI language: auto / zh-CN / en (auto follows system UI)",
            "language",
            &s(&self.language),
        );
        push_entry(
            &mut out,
            "调试日志写入 exe 旁 App.log（单文件 ≤ 2 MiB）",
            "Debug log to App.log beside the exe (single file ≤ 2 MiB)",
            "debug_logging",
            b(self.debug_logging),
        );
        push_entry(
            &mut out,
            "启用全局清理热键（RegisterHotKey）",
            "Enable global cleanup hotkey (RegisterHotKey)",
            "cleanup_hotkey_enabled",
            b(self.cleanup_hotkey_enabled),
        );
        push_entry(
            &mut out,
            "热键组合，如 Ctrl+Alt+C；仅 toml，改完需重启",
            "Hotkey chord e.g. Ctrl+Alt+C; TOML-only, restart required",
            "cleanup_hotkey",
            &s(&self.cleanup_hotkey),
        );
        push_entry(
            &mut out,
            "自动清理总开关（默认开；低内存通知 + 阈值轮询）",
            "Auto-cleanup master switch (default on; low-memory notify + threshold poll)",
            "auto_cleanup_enabled",
            b(self.auto_cleanup_enabled),
        );
        push_entry(
            &mut out,
            "物理内存占用阈值 0–100；0 表示不按阈值触发",
            "Physical memory usage threshold 0–100; 0 disables threshold triggers",
            "auto_cleanup_threshold",
            &self.auto_cleanup_threshold.to_string(),
        );
        push_entry(
            &mut out,
            "在系统任务栏显示 ↑/↓/CPU/RAM 指标条",
            "Show ↑/↓/CPU/RAM metrics strip on the taskbar",
            "show_taskbar_window",
            b(self.show_taskbar_window),
        );
        push_entry(
            &mut out,
            "任务栏字体磅值（整数 pt，5–72，默认 10）；仅 toml，改完需重启",
            "Taskbar font size in integer pt (5–72, default 10); TOML-only, restart required",
            "taskbar_font_point_size",
            &self.taskbar_font_point_size.to_string(),
        );

        while out.ends_with("\n\n") {
            out.pop();
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    fn normalize(&mut self) {
        self.normalize_memory_areas();
        self.normalize_language();
        self.normalize_cleanup_hotkey();
        self.normalize_auto_cleanup();
        self.normalize_taskbar_font();
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

    /// 任务栏字体磅值钳到可读范围。
    fn normalize_taskbar_font(&mut self) {
        self.taskbar_font_point_size = normalize_font_point_size(self.taskbar_font_point_size);
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
        let content = self.to_toml_documented();
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
        assert!(!settings.debug_logging);
        assert!(settings.cleanup_hotkey_enabled);
        assert_eq!(
            settings.cleanup_hotkey,
            crate::win32::hotkey::HotkeyBinding::DEFAULT_CLEANUP
        );
        assert!(settings.auto_cleanup_enabled);
        assert_eq!(settings.auto_cleanup_threshold, 0);
        assert!(settings.show_taskbar_window);
        assert_eq!(settings.taskbar_font_point_size, TASKBAR_FONT_PT_DEFAULT);
    }

    #[test]
    fn taskbar_font_point_size_is_clamped() {
        let settings = Settings::from_toml("taskbar_font_point_size = 3");
        assert_eq!(settings.taskbar_font_point_size, TASKBAR_FONT_PT_MIN);
        let settings = Settings::from_toml("taskbar_font_point_size = 99");
        assert_eq!(settings.taskbar_font_point_size, TASKBAR_FONT_PT_MAX);
    }

    #[test]
    fn taskbar_font_point_size_rounds_legacy_fraction() {
        let settings = Settings::from_toml("taskbar_font_point_size = 13.5");
        assert_eq!(settings.taskbar_font_point_size, 14);
        let settings = Settings::from_toml("taskbar_font_point_size = 10.4");
        assert_eq!(settings.taskbar_font_point_size, 10);
    }

    #[test]
    fn toml_missing_known_keys_detects_partial_file() {
        assert!(Settings::toml_missing_known_keys(
            "run_at_startup = false\n"
        ));
        let full = Settings::default().to_toml_documented();
        assert!(!Settings::toml_missing_known_keys(&full));
        assert!(full.contains(Settings::TOML_COMMENT_MARKER));
        assert!(full.contains("任务栏字体磅值"));
        assert!(full.contains("Taskbar font size"));
    }

    #[test]
    fn documented_toml_roundtrips_through_parser() {
        let original = Settings {
            debug_logging: true,
            language: "zh-CN".into(),
            cleanup_hotkey: "Ctrl+Shift+M".into(),
            taskbar_font_point_size: 16,
            ..Default::default()
        };
        let text = original.to_toml_documented();
        let restored: Settings = toml::from_str(&text).unwrap();
        assert!(restored.debug_logging);
        assert_eq!(restored.language, "zh-CN");
        assert_eq!(restored.cleanup_hotkey, "Ctrl+Shift+M");
        assert_eq!(restored.taskbar_font_point_size, 16);
        assert!(!Settings::toml_needs_rewrite(&text));
    }

    #[cfg(windows)]
    #[test]
    fn load_seeds_full_file_when_missing() {
        with_temp_appdata(|app_data| {
            let loaded = Settings::load();
            let path = app_data
                .join(crate::version::CONFIG_DIR_NAME)
                .join("settings.toml");
            let content = std::fs::read_to_string(&path).expect("seeded file");
            assert!(!Settings::toml_missing_known_keys(&content));
            assert!(content.contains(Settings::TOML_COMMENT_MARKER));
            assert!(content.contains("Highest-privilege"));
            assert_eq!(loaded.language, "auto");
            assert!(loaded.show_taskbar_window);
        });
    }

    #[cfg(windows)]
    #[test]
    fn load_fills_missing_keys_preserving_values() {
        with_temp_appdata(|app_data| {
            let dir = app_data.join(crate::version::CONFIG_DIR_NAME);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("settings.toml");
            std::fs::write(&path, "debug_logging = true\nlanguage = \"en\"\n").unwrap();
            let loaded = Settings::load();
            assert!(loaded.debug_logging);
            assert_eq!(loaded.language, "en");
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(!Settings::toml_missing_known_keys(&content));
            assert!(content.contains("debug_logging = true"));
            assert!(content.contains(Settings::TOML_COMMENT_MARKER));
        });
    }

    #[cfg(windows)]
    fn with_temp_appdata(run: impl FnOnce(&std::path::Path)) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().expect("appdata lock");
        let temp = tempfile::tempdir().expect("tempdir");
        let previous = std::env::var_os("APPDATA");
        // SAFETY: 测试串行持锁，返回前恢复 APPDATA。
        unsafe {
            std::env::set_var("APPDATA", temp.path());
        }
        run(temp.path());
        if let Some(path) = previous {
            unsafe {
                std::env::set_var("APPDATA", path);
            }
        } else {
            unsafe {
                std::env::remove_var("APPDATA");
            }
        }
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
        let restored: Settings = toml::from_str(&original.to_toml_documented()).unwrap();
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
        let serialized = settings.to_toml_documented();
        assert!(!serialized.contains("always_on_top ="));
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
