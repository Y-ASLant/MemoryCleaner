use crate::locale::locale_from_lang_id;

/// Windows 8 起自 build 9200；更早版本不受支持。
const WINDOWS_8_MIN_BUILD: u32 = 9200;

/// Windows 11 起自 build 22000。
const WINDOWS_11_BUILD: u32 = 22000;

/// 在受支持系统（Windows 8 及更高）上返回 `true`。
pub fn is_supported() -> bool {
    is_build_at_least(WINDOWS_8_MIN_BUILD)
}

/// 显示中英双语「不支持的操作系统」消息框。仅在
/// [`is_supported`] 为 `false` 时调用。
pub fn reject_unsupported() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::PCWSTR;

    let message = crate::win32::wide_null(
        "Memory Cleaner 需要 Windows 8 或更高版本。\r\nMemory Cleaner requires Windows 8 or later.",
    );
    let caption = crate::win32::wide_null("Memory Cleaner");
    unsafe {
        MessageBoxW(
            Some(HWND::default()),
            PCWSTR(message.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// 返回映射到受支持应用语言的系统 UI 区域（`zh-CN` 或 `en`）。
pub fn system_ui_locale() -> &'static str {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;

    let lang_id = unsafe { GetUserDefaultUILanguage() };
    locale_from_lang_id(lang_id)
}

/// 在 Windows 11 及更高（build >= 22000）上返回 `true`。
pub fn is_windows_11_or_later() -> bool {
    is_build_at_least(WINDOWS_11_BUILD)
}

fn is_build_at_least(minimum: u32) -> bool {
    windows_build_number().is_some_and(|build| build >= minimum)
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
