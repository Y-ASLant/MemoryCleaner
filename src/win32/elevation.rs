use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::version::PROCESS_BASE_NAME;
use crate::win32::handle::OwnedWin32Handle;

pub(super) const ELEVATED_ARG: &str = "--elevated";

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: isize,
        lpszverb: *const u16,
        lpszfile: *const u16,
        lpszparams: *const u16,
        lpszdir: *const u16,
        nshowcmd: i32,
    ) -> isize;
}

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

/// Relaunches the process through UAC unless it is already elevated.
pub fn ensure_elevated() {
    if std::env::args().any(|arg| arg == ELEVATED_ARG) || is_elevated() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else {
        crate::log_msg("[elevation] cannot determine executable path");
        return;
    };
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    let param_string = super::startup::elevation_relaunch_args();
    let params: Vec<u16> = param_string.encode_utf16().chain(Some(0)).collect();

    let result = unsafe {
        ShellExecuteW(
            0,
            verb.as_ptr(),
            path.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            1,
        )
    };
    // ShellExecute may return > 32 even when the user later cancels UAC.
    // Wait for the elevated child before exiting; otherwise continue unelevated.
    let process_name = format!("{PROCESS_BASE_NAME}.exe");
    if result as usize > 32
        && super::process::wait_for_elevated_relaunch(std::process::id(), &process_name, 10_000)
    {
        std::process::exit(0);
    }
    // User cancelled UAC — continue without admin; some cleanup areas will fail.
}
