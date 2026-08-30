//! Run-at-startup via a Task Scheduler logon task with highest privileges.
//!
//! A `HKCU\...\Run` value cannot grant elevation, so it would trigger a UAC
//! prompt (or a degraded, unprivileged start) at every sign-in. Instead the
//! autostart is registered as a scheduled task created with `/RL HIGHEST`,
//! which starts the process elevated without prompting. The legacy Run value
//! is still deleted so users migrating from older versions are moved over.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, WAIT_OBJECT_0};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE, RegCloseKey, RegDeleteValueW,
};
use windows::Win32::System::TaskScheduler::{ITaskService, TaskScheduler};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessW, GetExitCodeProcess, INFINITE, PROCESS_INFORMATION,
    STARTUPINFOW, WaitForSingleObject,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Error, HRESULT, PCWSTR, PWSTR};

use crate::settings::Settings;
use crate::version::PROCESS_BASE_NAME;

/// Registry / CLI flag for silent login autostart (tray only, no main window).
pub const STARTUP_ARG: &str = "--startup";

pub fn is_startup_launch() -> bool {
    std::env::args().any(|arg| arg == STARTUP_ARG)
}

/// Args passed to the elevated child so startup mode survives UAC relaunch.
pub fn elevation_relaunch_args() -> String {
    if is_startup_launch() {
        format!("{ELEVATED_ARG} {STARTUP_ARG}")
    } else {
        ELEVATED_ARG.to_string()
    }
}

const ELEVATED_ARG: &str = "--elevated";

/// Name of the Task Scheduler task in the root folder.
const TASK_NAME: &str = "MemoryCleaner_Autostart";

/// Legacy autostart location (`HKCU\...\Run`), deleted on sync for migration.
const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Task action command line: quoted exe path plus the startup flag.
fn task_trigger_value() -> Result<String> {
    let exe = std::env::current_exe().context("current_exe unavailable")?;
    Ok(format!("\"{}\" {STARTUP_ARG}", exe.display()))
}

fn schtasks_create_arguments() -> Result<String> {
    // The inner quotes survive CommandLineToArgvW as `\"`, so paths with
    // spaces reach schtasks as a single /TR value.
    Ok(format!(
        "/Create /F /SC ONLOGON /RL HIGHEST /TN \"{TASK_NAME}\" /TR \"{}\"",
        task_trigger_value()?.replace('"', "\\\"")
    ))
}

/// Run `schtasks` detached from any console and return its exit code.
fn run_schtasks(arguments: &str) -> Result<i32> {
    let mut command_line = wide_null(&format!("schtasks {arguments}"));
    let startup_info = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();

    unsafe {
        CreateProcessW(
            None,
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            None,
            &startup_info,
            &mut process_info,
        )
        .context("CreateProcessW (schtasks) failed")?;

        let wait = WaitForSingleObject(process_info.hProcess, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(process_info.hThread);
            let _ = CloseHandle(process_info.hProcess);
            anyhow::bail!("schtasks wait returned {wait:?}");
        }

        let mut exit_code: u32 = 0;
        let result =
            GetExitCodeProcess(process_info.hProcess, &mut exit_code).context("GetExitCodeProcess");
        let _ = CloseHandle(process_info.hThread);
        let _ = CloseHandle(process_info.hProcess);
        result?;
        Ok(exit_code as i32)
    }
}

/// HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND), returned by `ITaskFolder::GetTask`.
const TASK_NOT_FOUND_HRESULT: HRESULT = HRESULT(0x8007_0002_u32 as i32);

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .context("CoInitializeEx(Task Scheduler) failed")?;
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn is_task_not_found(code: HRESULT) -> bool {
    code == TASK_NOT_FOUND_HRESULT
}

fn task_exists() -> Result<bool> {
    let _com = ComApartment::initialize()?;
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
            .context("CoCreateInstance(TaskScheduler) failed")?;
    let empty = VARIANT::default();
    let folder = unsafe {
        service
            .Connect(&empty, &empty, &empty, &empty)
            .context("ITaskService::Connect failed")?;
        service
            .GetFolder(&BSTR::from("\\"))
            .context("ITaskService::GetFolder failed")?
    };

    match unsafe { folder.GetTask(&BSTR::from(TASK_NAME)) } {
        Ok(_) => Ok(true),
        Err(error) if is_task_not_found(error.code()) => Ok(false),
        Err(error) => Err(error).context("ITaskFolder::GetTask failed"),
    }
}

fn ensure_task() -> Result<()> {
    let exit_code = run_schtasks(&schtasks_create_arguments()?)?;
    if exit_code != 0 {
        anyhow::bail!("schtasks create failed with exit code {exit_code}");
    }
    Ok(())
}

fn delete_task() -> Result<()> {
    if !task_exists()? {
        return Ok(());
    }
    let exit_code = run_schtasks(&format!("/Delete /F /TN \"{TASK_NAME}\""))?;
    if exit_code != 0 {
        anyhow::bail!("schtasks delete failed with exit code {exit_code}");
    }
    Ok(())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    if enabled {
        ensure_task()?;
        // Migrate away from a leftover legacy Run-key entry.
        delete_value_best_effort();
    } else {
        delete_task()?;
        delete_value_best_effort();
    }
    Ok(())
}

pub fn sync(settings: &Settings) -> Result<()> {
    set_enabled(settings.run_at_startup)
}

fn win32_ok(status: windows::Win32::Foundation::WIN32_ERROR) -> Result<()> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(Error::from(status).into())
    }
}

fn delete_value_best_effort() {
    let subkey = wide_null(RUN_KEY_PATH);
    let value_name = wide_null(PROCESS_BASE_NAME);
    let mut key = HKEY::default();
    let status = unsafe {
        windows::Win32::System::Registry::RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            windows::Win32::System::Registry::REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if !status.is_ok() {
        return;
    }
    let delete = unsafe { RegDeleteValueW(key, PCWSTR(value_name.as_ptr())) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if let Err(error) = win32_ok(delete) {
        let missing = delete == ERROR_FILE_NOT_FOUND;
        if !missing {
            crate::log_msg(&format!(
                "[startup] legacy Run value delete failed: {error:#}"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_trigger_value_quotes_exe_and_appends_startup_flag() {
        let trigger = task_trigger_value().expect("current_exe");
        let exe = std::env::current_exe().expect("current_exe");
        assert_eq!(trigger, format!("\"{}\" {STARTUP_ARG}", exe.display()));
    }

    #[test]
    fn create_arguments_escape_inner_quotes_for_tr_value() {
        let arguments = schtasks_create_arguments().expect("arguments");
        assert!(arguments.contains("/SC ONLOGON"));
        assert!(arguments.contains("/RL HIGHEST"));
        assert!(arguments.contains(&format!("/TN \"{TASK_NAME}\"")));
        // The exe path must be wrapped in escaped quotes so /TR stays one value.
        assert!(arguments.contains("\\\""));
        assert!(arguments.ends_with(format!("{STARTUP_ARG}\"").as_str()));
    }

    #[test]
    fn task_query_only_treats_missing_task_as_absent() {
        assert!(is_task_not_found(TASK_NOT_FOUND_HRESULT));
        assert!(!is_task_not_found(HRESULT(0x8007_0005_u32 as i32)));
    }
}
