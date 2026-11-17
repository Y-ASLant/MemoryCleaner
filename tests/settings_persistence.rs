use memory_cleaner::optimize::MemoryAreas;
use memory_cleaner::settings::Settings;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static APPDATA_TEST_LOCK: Mutex<()> = Mutex::new(());

fn with_temp_appdata<F>(run: F)
where
    F: FnOnce(&std::path::Path),
{
    let _guard = APPDATA_TEST_LOCK.lock().expect("app_data test lock");
    let temp = tempfile::tempdir().expect("tempdir");
    let previous = std::env::var_os("APPDATA");
    // SAFETY: 测试默认串行，返回前恢复 APPDATA。
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
fn settings_save_and_load_roundtrip_in_temp_config_dir() {
    with_temp_appdata(|app_data| {
        fs::create_dir_all(app_data.join("MemoryCleaner")).expect("create config dir");

        let settings = Settings {
            memory_areas: MemoryAreas::WORKING_SET.bits(),
            language: "zh-CN".into(),
            debug_logging: true,
            auto_cleanup_enabled: true,
            ..Settings::default()
        };
        settings.save();

        let loaded = Settings::load();
        assert_eq!(loaded.memory_areas, MemoryAreas::WORKING_SET.bits());
        assert_eq!(loaded.language, "zh-CN");
        assert!(loaded.debug_logging);
        assert!(loaded.auto_cleanup_enabled);
    });
}

#[test]
fn settings_save_uses_atomic_replace() {
    with_temp_appdata(|app_data| {
        Settings::default().save();

        let final_path: PathBuf = app_data.join("MemoryCleaner").join("settings.toml");
        let tmp_path = final_path.with_extension("toml.tmp");

        assert!(final_path.is_file());
        assert!(!tmp_path.exists());
    });
}

#[test]
fn settings_save_reports_directory_errors() {
    with_temp_appdata(|app_data| {
        fs::write(app_data.join("MemoryCleaner"), "blocks config directory").unwrap();
        assert!(Settings::default().try_save().is_err());
    });
}

#[test]
fn settings_save_replaces_existing_file() {
    with_temp_appdata(|_| {
        Settings::default().save();
        let updated = Settings {
            debug_logging: true,
            ..Settings::default()
        };
        updated.save();
        assert!(Settings::load().debug_logging);
    });
}
