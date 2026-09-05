use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::win32::{com::ComApartment, handle::OwnedWin32Handle};

pub(super) const ELEVATED_ARG: &str = "--elevated";

fn is_elevated() -> bool {
    let mut raw_token = HANDLE::default();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) }.is_err() {
        return false;
    }
    let token = unsafe { OwnedWin32Handle::from_raw(raw_token) };
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned_length = 0u32;
    let result = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenElevation,
            Some((&raw mut elevation).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned_length,
        )
    };
    result.is_ok() && elevation.TokenIsElevated != 0
}

fn shell_execute_process(exe: &Path, parameters: &str, verb: &str) -> Result<OwnedWin32Handle> {
    ComApartment::run(|| {
        let path: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = verb.encode_utf16().chain(Some(0)).collect();
        let params: Vec<u16> = parameters.encode_utf16().chain(Some(0)).collect();
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(path.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        unsafe { ShellExecuteExW(&raw mut info) }.context("ShellExecuteExW failed")?;
        if info.hProcess.is_invalid() {
            bail!("ShellExecuteExW returned no process handle");
        }
        Ok(unsafe { OwnedWin32Handle::from_raw(info.hProcess) })
    })
}

fn launch_elevated(exe: &Path, parameters: &str) -> Result<OwnedWin32Handle> {
    shell_execute_process(exe, parameters, "runas").context("elevated process launch failed")
}

/// Relaunches through UAC when needed. Returns `true` after creating the
/// specific elevated child, so the unelevated caller can return normally.
pub fn ensure_elevated() -> bool {
    if std::env::args().any(|arg| arg == ELEVATED_ARG) || is_elevated() {
        return false;
    }

    let result = std::env::current_exe()
        .context("cannot determine executable path")
        .and_then(|exe| launch_elevated(&exe, &super::startup::elevation_relaunch_args()));
    match result {
        Ok(_child) => true,
        Err(error) => {
            crate::log_msg(&format!("[elevation] relaunch failed: {error:#}"));
            // UAC was declined or launch failed. Continue without admin so
            // non-privileged features remain available.
            false
        }
    }
}
