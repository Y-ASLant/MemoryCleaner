use crate::settings::Settings;

/// Maps a Windows UI language ID to a supported app locale.
pub fn locale_from_lang_id(lang_id: u16) -> &'static str {
    match lang_id & 0x3FF {
        0x04 => "zh-CN",
        _ => "en",
    }
}

/// Returns the list separator for the active locale.
pub fn list_separator() -> &'static str {
    match rust_i18n::locale().as_ref() {
        "en" => ", ",
        _ => "、",
    }
}

/// Applies the effective locale from settings to the global i18n state.
pub fn apply(settings: &Settings) {
    rust_i18n::set_locale(settings.effective_locale());
}

#[cfg(test)]
pub fn with_locale<F: FnOnce()>(locale: &str, run: F) {
    use std::sync::Mutex;

    static LOCALE_TEST_LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCALE_TEST_LOCK.lock().expect("locale test lock");
    rust_i18n::set_locale(locale);
    run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_from_lang_id_maps_chinese_and_english() {
        assert_eq!(locale_from_lang_id(0x0804), "zh-CN");
        assert_eq!(locale_from_lang_id(0x0409), "en");
        assert_eq!(locale_from_lang_id(0x040c), "en");
    }

    #[test]
    fn list_separator_follows_active_locale() {
        with_locale("zh-CN", || assert_eq!(list_separator(), "、"));
        with_locale("en", || assert_eq!(list_separator(), ", "));
    }
}
