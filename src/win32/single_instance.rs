//! 单实例守卫，并向已运行实例发送唤醒信号。
//!
//! 首个实例创建进程生命周期内持有的命名互斥体，以及由专用线程监视的自动复位事件。
//! 第二次启动打开该事件、置位后退出；监视线程将信号转发为
//! `TrayCommand::Optimize`（与左键清理相同）。

use anyhow::{Context, Result, anyhow, bail};
use std::sync::{OnceLock, mpsc::Sender};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForSingleObject,
};
use windows::core::{HRESULT, PCWSTR};

use super::wide_null;
use crate::tray::TrayCommand;
use crate::win32::handle::OwnedWin32Handle;

const MUTEX_NAME: &str = "MemoryCleanerLite_{C4E91F0A-7B2D-4A63-9E18-5F6D0C8B3A21}";
const SHOW_EVENT_NAME: &str = "MemoryCleanerLite_{C4E91F0A-7B2D-4A63-9E18-5F6D0C8B3A21}_ShowWindow";
static INSTANCE_MUTEX: OnceLock<OwnedWin32Handle> = OnceLock::new();

/// 尝试唤醒可能已在运行的实例的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceSignal {
    /// 未检测到运行中实例；可继续启动。
    NotRunning,
    /// 已通知运行中实例；本次启动应退出。
    Signaled,
    /// 已有实例在跑，但本进程无法发信号
    /// （提权前完整性级别不匹配）。继续启动；
    /// 提权后重试或互斥检查会判定最终结果。
    AccessDenied,
}

/// 尝试唤醒已运行实例。
pub fn signal_existing_instance() -> InstanceSignal {
    let name = wide_null(SHOW_EVENT_NAME);

    let event = match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) } {
        Ok(event) => unsafe { OwnedWin32Handle::from_raw(event) },
        Err(error) => return signal_from_open_event_error(error.code()),
    };

    let signaled = unsafe { SetEvent(event.raw()) };
    match signaled {
        Ok(()) => InstanceSignal::Signaled,
        Err(_) => InstanceSignal::AccessDenied,
    }
}

/// 确保应用仅有一个实例。成功时本进程即为该实例，
/// 且 `event_watcher` 将唤醒信号转发到 `command_tx`。
pub fn ensure_single_instance(command_tx: Sender<TrayCommand>) -> Result<()> {
    let mutex_name = wide_null(MUTEX_NAME);

    let mutex = unsafe {
        OwnedWin32Handle::from_raw(
            CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())).context("CreateMutexW failed")?,
        )
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        bail!("Application is already running");
    }

    let event_name = wide_null(SHOW_EVENT_NAME);
    let event = unsafe {
        OwnedWin32Handle::from_raw(
            CreateEventW(None, false, false, PCWSTR(event_name.as_ptr()))
                .context("CreateEventW failed")?,
        )
    };
    spawn_event_watcher(event, command_tx)?;
    INSTANCE_MUTEX
        .set(mutex)
        .map_err(|_| anyhow!("single-instance guard was initialized twice"))?;

    Ok(())
}

fn spawn_event_watcher(event: OwnedWin32Handle, command_tx: Sender<TrayCommand>) -> Result<()> {
    std::thread::Builder::new()
        .name("single-instance-watcher".into())
        .spawn(move || watcher_loop(event, command_tx))
        .context("single-instance watcher start failed")?;
    Ok(())
}

fn watcher_loop(event: OwnedWin32Handle, command_tx: Sender<TrayCommand>) {
    loop {
        let result = unsafe { WaitForSingleObject(event.raw(), INFINITE) };
        if result != WAIT_OBJECT_0 {
            break;
        }
        if command_tx.send(TrayCommand::Optimize).is_err() {
            break;
        }
    }
}

const HRESULT_ACCESS_DENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);

fn signal_from_open_event_error(code: HRESULT) -> InstanceSignal {
    if code == HRESULT_ACCESS_DENIED {
        InstanceSignal::AccessDenied
    } else {
        InstanceSignal::NotRunning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_denied_wake_signal_is_distinguished() {
        assert_eq!(
            signal_from_open_event_error(HRESULT_ACCESS_DENIED),
            InstanceSignal::AccessDenied
        );
        assert_eq!(
            signal_from_open_event_error(HRESULT(0x8007_0002_u32 as i32)),
            InstanceSignal::NotRunning
        );
    }
}
