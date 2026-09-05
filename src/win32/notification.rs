use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::InitPropVariantFromStringAsVector;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
use windows::Win32::UI::Shell::{
    IShellLinkW, PropertiesSystem::IPropertyStore, SetCurrentProcessExplicitAppUserModelID,
    ShellLink,
};
use windows::core::{HSTRING, Interface};

use crate::win32::com::ComApartment;

pub const APP_USER_MODEL_ID: &str = "MemoryCleaner.App";

pub fn init() -> Result<()> {
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_USER_MODEL_ID))
            .context("SetCurrentProcessExplicitAppUserModelID failed")?;
    }
    ensure_start_menu_shortcut()?;
    Ok(())
}

pub fn show(title: &str, body: &str) -> Result<()> {
    let xml = format!(
        r#"<toast><visual><binding template="ToastText02"><text id="1">{}</text><text id="2">{}</text></binding></visual></toast>"#,
        escape_xml(title),
        escape_xml(body),
    );

    ComApartment::run(|| {
        // Static WinRT factory caches can outlive a torn-down apartment.
        // Activate locally and release every factory before ComApartment::run returns.
        let doc: XmlDocument = unsafe {
            windows::Win32::System::WinRT::RoActivateInstance(&HSTRING::from(
                <XmlDocument as windows::core::RuntimeName>::NAME,
            ))
        }
        .context("XmlDocument activation failed")?
        .cast()?;
        doc.LoadXml(&HSTRING::from(xml))
            .context("toast XML load failed")?;

        let toast_factory = windows::core::factory::<
            ToastNotification,
            windows::UI::Notifications::IToastNotificationFactory,
        >()
        .context("ToastNotification factory failed")?;
        let toast: ToastNotification = unsafe {
            let mut result = std::ptr::null_mut();
            (toast_factory.vtable().CreateToastNotification)(
                toast_factory.as_raw(),
                doc.as_raw(),
                &mut result,
            )
            .and_then(|| windows::core::Type::from_abi(result))
        }
        .context("CreateToastNotification failed")?;
        let manager = windows::core::factory::<
            ToastNotificationManager,
            windows::UI::Notifications::IToastNotificationManagerStatics,
        >()
        .context("ToastNotificationManager factory failed")?;
        let app_id = HSTRING::from(APP_USER_MODEL_ID);
        let notifier: windows::UI::Notifications::ToastNotifier = unsafe {
            let mut result = std::ptr::null_mut();
            (manager.vtable().CreateToastNotifierWithId)(
                manager.as_raw(),
                std::mem::transmute_copy(&app_id),
                &mut result,
            )
            .and_then(|| windows::core::Type::from_abi(result))
        }
        .context("CreateToastNotifierWithId failed")?;
        notifier
            .Show(&toast)
            .context("ToastNotifier::Show failed")?;

        Ok(())
    })
}

fn ensure_start_menu_shortcut() -> Result<()> {
    let shortcut_path = start_menu_shortcut_path()?;
    let exe = std::env::current_exe().context("current_exe failed")?;
    if shortcut_path.is_file() && shortcut_target_matches(&shortcut_path, &exe) {
        return Ok(());
    }

    if let Some(parent) = shortcut_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    ComApartment::run(|| {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .context("ShellLink failed")?;
            link.SetPath(&HSTRING::from(exe.as_os_str()))
                .context("SetPath failed")?;
            link.SetArguments(&HSTRING::from(""))
                .context("SetArguments failed")?;

            let property_store: IPropertyStore =
                link.cast().context("IPropertyStore cast failed")?;
            let app_id = InitPropVariantFromStringAsVector(&HSTRING::from(APP_USER_MODEL_ID))
                .context("InitPropVariantFromStringAsVector failed")?;
            property_store
                .SetValue(&PKEY_AppUserModel_ID, &app_id)
                .context("SetValue PKEY_AppUserModel_ID failed")?;
            property_store
                .Commit()
                .context("property store Commit failed")?;

            let persist_file: IPersistFile = link.cast().context("IPersistFile cast failed")?;
            persist_file
                .Save(&HSTRING::from(shortcut_path.as_os_str()), true)
                .context("shortcut Save failed")?;
        }

        Ok(())
    })
}

/// Read the shortcut's target path and compare with `current_exe`.
/// Returns `true` if they match, or if the shortcut cannot be read (conservative).
fn shortcut_target_matches(shortcut_path: &Path, current_exe: &Path) -> bool {
    read_shortcut_target(shortcut_path)
        .ok()
        .and_then(|target| {
            let a = std::fs::canonicalize(target).ok()?;
            let b = std::fs::canonicalize(current_exe).ok()?;
            Some(a == b)
        })
        .unwrap_or(false)
}

/// Load an existing `.lnk` and return its target path.
fn read_shortcut_target(shortcut_path: &Path) -> Result<PathBuf> {
    ComApartment::run(|| unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).context("ShellLink failed")?;
        let persist_file: IPersistFile = link.cast().context("IPersistFile cast failed")?;
        persist_file
            .Load(
                &HSTRING::from(shortcut_path.as_os_str()),
                windows::Win32::System::Com::STGM(0),
            )
            .context("shortcut Load failed")?;

        let mut buf = [0u16; MAX_PATH as usize];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0)
            .context("GetPath failed")?;

        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(PathBuf::from(OsString::from_wide(&buf[..len])))
    })
}

fn start_menu_shortcut_path() -> Result<PathBuf> {
    let mut appdata = [0u16; MAX_PATH as usize];
    let len = unsafe {
        windows::Win32::System::Environment::GetEnvironmentVariableW(
            windows::core::w!("APPDATA"),
            Some(&mut appdata),
        )
    };
    if len == 0 {
        anyhow::bail!("APPDATA not set");
    }

    let base = OsString::from_wide(&appdata[..len as usize]);
    Ok(PathBuf::from(base)
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join("Memory Cleaner.lnk"))
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{CO_E_NOTINITIALIZED, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{
        APTTYPE, APTTYPE_MTA, APTTYPEQUALIFIER, COINIT_MULTITHREADED, CoGetApartmentType,
        CoInitializeEx, CoUninitialize,
    };

    // An explicit MTA elsewhere can make fresh threads report an implicit MTA.
    static COM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn apartment_type() -> windows::core::Result<APTTYPE> {
        let mut apartment = APTTYPE::default();
        let mut qualifier = APTTYPEQUALIFIER::default();
        unsafe { CoGetApartmentType(&mut apartment, &mut qualifier) }?;
        Ok(apartment)
    }

    fn assert_com_uninitialized() {
        assert_eq!(
            apartment_type()
                .expect_err("thread must not own a COM apartment")
                .code(),
            CO_E_NOTINITIALIZED,
        );
    }

    #[test]
    fn failed_toasts_release_thread_apartment() {
        let _serial = COM_TEST_LOCK.lock().expect("COM test lock");
        std::thread::spawn(|| {
            assert_com_uninitialized();
            for _ in 0..3 {
                // U+0001 is invalid XML, so Show cannot submit a visible toast.
                show("invalid\u{1}", "body").expect_err("invalid toast XML must fail");
                assert_com_uninitialized();
            }
        })
        .join()
        .expect("COM regression thread");
    }

    #[test]
    fn failed_toasts_preserve_existing_sta_apartment() {
        let _serial = COM_TEST_LOCK.lock().expect("COM test lock");
        std::thread::spawn(|| {
            assert_com_uninitialized();
            let caller = ComApartment::initialize().expect("caller STA");
            let original = apartment_type().expect("caller apartment");
            for _ in 0..3 {
                show("invalid\u{1}", "body").expect_err("invalid toast XML must fail");
                assert_eq!(apartment_type().expect("caller STA preserved"), original);
            }
            drop(caller);
            assert_com_uninitialized();
        })
        .join()
        .expect("COM regression thread");
    }

    #[test]
    fn incompatible_toasts_preserve_existing_mta_apartment() {
        let _serial = COM_TEST_LOCK.lock().expect("COM test lock");
        std::thread::spawn(|| {
            assert_com_uninitialized();
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .expect("caller MTA");
            for _ in 0..3 {
                let error = show("invalid\u{1}", "body").expect_err("STA must reject caller MTA");
                assert_eq!(
                    error
                        .downcast_ref::<windows::core::Error>()
                        .expect("COM error")
                        .code(),
                    RPC_E_CHANGED_MODE,
                );
                assert_eq!(apartment_type().expect("caller MTA preserved"), APTTYPE_MTA);
            }
            unsafe { CoUninitialize() };
            assert_com_uninitialized();
        })
        .join()
        .expect("COM regression thread");
    }

    #[test]
    fn escape_xml_escapes_special_chars() {
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("<tag>"), "&lt;tag&gt;");
    }
}
