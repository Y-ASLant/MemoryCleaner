use crate::locale::locale_from_lang_id;

/// Windows 11 starts at build 22000.
const WINDOWS_11_BUILD: u32 = 22000;

/// Returns the system UI locale mapped to a supported app locale (`zh-CN` or `en`).
pub fn system_ui_locale() -> &'static str {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;

    let lang_id = unsafe { GetUserDefaultUILanguage() };
    locale_from_lang_id(lang_id)
}

/// Returns `true` on Windows 11 and later (build >= 22000).
pub fn is_windows_11_or_later() -> bool {
    windows_build_number().is_some_and(|build| build >= WINDOWS_11_BUILD)
}

fn windows_build_number() -> Option<u32> {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

    unsafe {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        if RtlGetVersion(&mut version).is_ok() {
            Some(version.dwBuildNumber)
        } else {
            None
        }
    }
}
