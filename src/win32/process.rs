use std::mem::MaybeUninit;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

/// Return true if another process with the same executable name is running.
pub fn has_sibling_process(current_pid: u32, exe_name: &str) -> bool {
    let exe_name_wide: Vec<u16> = exe_name.encode_utf16().collect();

    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let mut found = false;
        let mut entry = MaybeUninit::<PROCESSENTRY32W>::zeroed();
        (*entry.as_mut_ptr()).dwSize = size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, entry.as_mut_ptr()).is_ok() {
            loop {
                let e = entry.assume_init_ref();
                if e.th32ProcessID != current_pid {
                    let name = e.szExeFile;
                    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                    if name[..len] == exe_name_wide[..] {
                        found = true;
                        break;
                    }
                }
                if Process32NextW(snapshot, entry.as_mut_ptr()).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        found
    }
}

/// Best-effort wait until an elevated relaunch is observed, or timeout.
pub fn wait_for_elevated_relaunch(current_pid: u32, exe_name: &str, timeout_ms: u32) -> bool {
    let steps = timeout_ms / 100;
    for _ in 0..steps {
        if has_sibling_process(current_pid, exe_name) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}
