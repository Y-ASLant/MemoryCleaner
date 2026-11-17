use std::mem::MaybeUninit;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, GetLastError,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA,
};

use crate::win32::handle::OwnedWin32Handle;

/// 规范化进程名供排除匹配：小写、去空白、去 `.exe`。
pub fn normalize_process_name(name: &str) -> String {
    normalize_process_chars(name.chars(), name.len())
}

fn normalize_process_chars(chars: impl Iterator<Item = char>, capacity: usize) -> String {
    let mut normalized = String::with_capacity(capacity);
    normalized.extend(
        chars
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase()),
    );
    if normalized.ends_with(".exe") {
        normalized.truncate(normalized.len() - 4);
    }
    normalized
}

fn exe_base_name_from_entry(entry: &PROCESSENTRY32W) -> String {
    let name = &entry.szExeFile;
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let chars = char::decode_utf16(name[..len].iter().copied())
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER));
    normalize_process_chars(chars, len)
}

fn with_process_snapshot<F>(mut f: F) -> Result<()>
where
    F: FnMut(&PROCESSENTRY32W) -> bool,
{
    unsafe {
        let snapshot = OwnedWin32Handle::from_raw(
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).context("CreateToolhelp32Snapshot")?,
        );
        let mut entry = MaybeUninit::<PROCESSENTRY32W>::zeroed();
        (*entry.as_mut_ptr()).dwSize = size_of::<PROCESSENTRY32W>() as u32;

        if let Err(error) = Process32FirstW(snapshot.raw(), entry.as_mut_ptr()) {
            if error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                return Ok(());
            }
            return Err(error).context("Process32FirstW");
        }

        loop {
            if f(entry.assume_init_ref()) {
                break;
            }
            if let Err(error) = Process32NextW(snapshot.raw(), entry.as_mut_ptr()) {
                if error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                    break;
                }
                return Err(error).context("Process32NextW");
            }
        }
    }
    Ok(())
}

fn complete_working_set_cleanup(
    succeeded: usize,
    access_denied: usize,
    errors: Vec<String>,
) -> Result<()> {
    if !errors.is_empty() {
        bail!("Working Set per-process errors: {}", errors.join(", "));
    }
    if succeeded == 0 && access_denied > 0 {
        bail!(
            "Working Set per-process cleanup emptied no processes ({access_denied} access denied)"
        );
    }
    Ok(())
}

/// 清空除 `excluded` 外所有运行进程的工作集。
pub fn empty_working_sets_except(excluded: &[String]) -> Result<()> {
    let mut succeeded = 0usize;
    let mut access_denied = 0usize;
    let mut errors = Vec::new();

    with_process_snapshot(|entry| {
        let name = exe_base_name_from_entry(entry);
        if excluded.iter().any(|excluded| excluded == &name) {
            return false;
        }

        let pid = entry.th32ProcessID;
        let handle =
            match unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SET_QUOTA, false, pid) }
            {
                Ok(handle) => unsafe { OwnedWin32Handle::from_raw(handle) },
                Err(error)
                    if error.code()
                        == windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0) =>
                {
                    access_denied += 1;
                    return false;
                }
                // 快照与打开句柄之间进程可能已退出。
                Err(error)
                    if error.code()
                        == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) =>
                {
                    return false;
                }
                Err(error) => {
                    errors.push(format!("{name} (pid {pid}) open: {error}"));
                    return false;
                }
            };

        let result = unsafe { K32EmptyWorkingSet(handle.raw()) };
        if result.as_bool() {
            succeeded += 1;
        } else {
            let last_error = unsafe { GetLastError() };
            if last_error == ERROR_ACCESS_DENIED {
                access_denied += 1;
            } else {
                errors.push(format!("{name} (pid {pid}) empty: {last_error:?}"));
            }
        }
        false
    })?;

    complete_working_set_cleanup(succeeded, access_denied, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_process_name_strips_exe_and_whitespace() {
        assert_eq!(normalize_process_name(" Chrome.EXE "), "chrome");
        assert_eq!(normalize_process_name("firefox"), "firefox");
    }

    #[test]
    fn working_set_cleanup_rejects_total_access_denial() {
        assert!(complete_working_set_cleanup(0, 2, Vec::new()).is_err());
        assert!(complete_working_set_cleanup(1, 2, Vec::new()).is_ok());
    }
}
