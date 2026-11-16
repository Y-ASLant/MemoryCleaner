//! 通过任务计划程序的登录任务（最高权限）实现开机自启。
//!
//! `HKCU\...\Run` 无法提权，每次登录都会触发 UAC
//!（或以无特权降级启动）。因此自启注册为带 `/RL HIGHEST` 的计划任务，
//! 无需提示即可提权启动。仍会删除旧版 Run 项，便于从旧版本迁移。

use anyhow::{Context, Result};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_TIMEOUT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE, RegDeleteValueW,
};
use windows::Win32::System::TaskScheduler::{ITaskService, TaskScheduler};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessW, GetExitCodeProcess, PROCESS_INFORMATION, STARTUPINFOW,
    TerminateProcess, WaitForSingleObject,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Error, HRESULT, PCWSTR, PWSTR};

use super::wide_null;
use crate::version::PROCESS_BASE_NAME;
use crate::win32::com::ComApartment;
use crate::win32::elevation::ELEVATED_ARG;
use crate::win32::handle::{OwnedRegistryKey, OwnedWin32Handle};

/// 静默登录自启的注册表/CLI 标志（仅托盘，无主窗口）。
pub const STARTUP_ARG: &str = "--startup";

pub fn is_startup_launch() -> bool {
    std::env::args().any(|arg| arg == STARTUP_ARG)
}

/// 传给提权子进程的参数，使启动模式在 UAC 重拉起后仍保留。
pub fn elevation_relaunch_args() -> String {
    if is_startup_launch() {
        format!("{ELEVATED_ARG} {STARTUP_ARG}")
    } else {
        ELEVATED_ARG.to_string()
    }
}

/// 任务计划程序根目录中的任务名。
const TASK_NAME: &str = "MemoryCleaner_Autostart";

/// 旧版自启位置（`HKCU\...\Run`），同步时删除以完成迁移。
const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// 任务操作命令行：带引号的 exe 路径加上启动标志。
fn task_trigger_value() -> Result<String> {
    let exe = std::env::current_exe().context("current_exe unavailable")?;
    Ok(format!("\"{}\" {STARTUP_ARG}", exe.display()))
}

fn schtasks_create_arguments() -> Result<String> {
    // 内层引号经 CommandLineToArgvW 保留为 `\"`，含空格路径
    // 作为单个 /TR 值交给 schtasks。
    Ok(format!(
        "/Create /F /SC ONLOGON /RL HIGHEST /TN \"{TASK_NAME}\" /TR \"{}\"",
        task_trigger_value()?.replace('"', "\\\"")
    ))
}

const SCHTASKS_TIMEOUT_MS: u32 = 10_000;

/// 仅等待本进程拥有的子进程；超时则请求终止。
fn wait_for_process(process: &OwnedWin32Handle, timeout_ms: u32) -> Result<i32> {
    let wait = unsafe { WaitForSingleObject(process.raw(), timeout_ms) };
    if wait == WAIT_TIMEOUT {
        let termination = unsafe { TerminateProcess(process.raw(), ERROR_TIMEOUT.0) };
        let error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("process timed out after {timeout_ms} ms"),
        ));
        // 终止为异步；切勿再用一次等待去延长截止时间。
        return Err(match termination {
            Ok(()) => error,
            Err(termination_error) => {
                error.context(format!("TerminateProcess failed: {termination_error}"))
            }
        });
    }
    if wait == WAIT_FAILED {
        return Err(Error::from_thread()).context("WaitForSingleObject failed");
    }
    if wait != WAIT_OBJECT_0 {
        anyhow::bail!("process wait returned {wait:?}");
    }

    let mut exit_code: u32 = 0;
    unsafe { GetExitCodeProcess(process.raw(), &mut exit_code) }.context("GetExitCodeProcess")?;
    Ok(exit_code as i32)
}

/// 在无控制台环境下运行 `schtasks` 并返回其退出码。
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

        let process = OwnedWin32Handle::from_raw(process_info.hProcess);
        let _thread = OwnedWin32Handle::from_raw(process_info.hThread);
        wait_for_process(&process, SCHTASKS_TIMEOUT_MS).context("schtasks failed")
    }
}

/// HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)，由 `ITaskFolder::GetTask` 返回。
const TASK_NOT_FOUND_HRESULT: HRESULT = HRESULT(0x8007_0002_u32 as i32);

fn is_task_not_found(code: HRESULT) -> bool {
    code == TASK_NOT_FOUND_HRESULT
}

fn task_exists() -> Result<bool> {
    ComApartment::run(|| {
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
    })
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
        // 迁移并清除残留的旧版 Run 项。
        delete_value_best_effort();
    } else {
        delete_task()?;
        delete_value_best_effort();
    }
    Ok(())
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
    let key = unsafe { OwnedRegistryKey::from_raw(key) };
    let delete = unsafe { RegDeleteValueW(key.raw(), PCWSTR(value_name.as_ptr())) };
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
    use windows::Win32::System::Threading::{CREATE_SUSPENDED, PROCESS_CREATION_FLAGS};

    fn disposable_child(flags: PROCESS_CREATION_FLAGS) -> OwnedWin32Handle {
        let executable = std::env::var("ComSpec").expect("ComSpec");
        let application = wide_null(&executable);
        let mut command_line = wide_null(&format!("\"{executable}\" /D /C exit /B 37"));
        let startup_info = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_info = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                flags | CREATE_NO_WINDOW,
                None,
                None,
                &startup_info,
                &mut process_info,
            )
            .expect("create disposable cmd child");
            let _thread = OwnedWin32Handle::from_raw(process_info.hThread);
            OwnedWin32Handle::from_raw(process_info.hProcess)
        }
    }

    #[test]
    fn process_wait_preserves_exit_code() {
        let process = disposable_child(PROCESS_CREATION_FLAGS(0));
        assert_eq!(wait_for_process(&process, 5_000).expect("child exits"), 37);
    }

    #[test]
    fn process_wait_times_out_and_terminates_owned_child() {
        // 挂起的一次性子进程在超时前无法自行退出。
        let process = disposable_child(CREATE_SUSPENDED);
        let started = std::time::Instant::now();
        let result = wait_for_process(&process, 20);
        let elapsed = started.elapsed();
        let stopped = unsafe { WaitForSingleObject(process.raw(), 5_000) };
        if stopped != WAIT_OBJECT_0 {
            // 若终止失败，避免回归测试本身泄漏子进程。
            unsafe { TerminateProcess(process.raw(), ERROR_TIMEOUT.0) }
                .expect("clean up disposable child");
        }
        let error = result.expect_err("suspended child must time out");
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .expect("timeout error")
                .kind(),
            std::io::ErrorKind::TimedOut,
        );
        assert!(elapsed < std::time::Duration::from_secs(5));
        assert_eq!(
            stopped, WAIT_OBJECT_0,
            "timeout must request child termination"
        );
        let mut exit_code = 0;
        unsafe { GetExitCodeProcess(process.raw(), &mut exit_code) }.expect("child exit code");
        assert_eq!(exit_code, ERROR_TIMEOUT.0);
    }

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
        // exe 路径须用转义引号包裹，使 /TR 保持为单一值。
        assert!(arguments.contains("\\\""));
        assert!(arguments.ends_with(format!("{STARTUP_ARG}\"").as_str()));
    }

    #[test]
    fn task_query_only_treats_missing_task_as_absent() {
        assert!(is_task_not_found(TASK_NOT_FOUND_HRESULT));
        assert!(!is_task_not_found(HRESULT(0x8007_0005_u32 as i32)));
    }
}
