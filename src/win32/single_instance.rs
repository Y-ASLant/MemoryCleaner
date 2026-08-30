//! Single-instance guard plus a wake-up signal for the already-running app.
//!
//! The first instance creates a named mutex (leaked handle keeps the kernel
//! object alive for the process lifetime) and an auto-reset event watched by a
//! dedicated thread. A second launch opens the event, sets it, and exits; the
//! watcher forwards the signal as `TrayCommand::ActivateWindow` so the running
//! instance shows its main window.

use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForSingleObject,
};
use windows::core::{HRESULT, PCWSTR};

use crate::tray::TrayCommand;

const MUTEX_NAME: &str = "MemoryCleaner_{B8F3A7E2-4C1D-4F5A-9B6E-2D8C3F7A1E9B}";
const SHOW_EVENT_NAME: &str = "MemoryCleaner_{B8F3A7E2-4C1D-4F5A-9B6E-2D8C3F7A1E9B}_ShowWindow";

/// Outcome of trying to wake a possibly running instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceSignal {
    /// No running instance was detected; startup may continue.
    NotRunning,
    /// A running instance was signaled to show its window; this launch should exit.
    Signaled,
    /// An instance is running but cannot be signaled from this process
    /// (integrity-level mismatch before elevation). Startup continues; the
    /// post-elevation retry or the mutex check resolves the outcome.
    AccessDenied,
}

/// Try to wake an already-running instance so it shows its main window.
pub fn signal_existing_instance() -> InstanceSignal {
    let name = wide_null(SHOW_EVENT_NAME);

    let event = match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) } {
        Ok(event) => event,
        Err(error) => return signal_from_open_event_error(error.code()),
    };

    let signaled = unsafe { SetEvent(event) };
    unsafe {
        let _ = CloseHandle(event);
    }
    match signaled {
        Ok(()) => InstanceSignal::Signaled,
        Err(_) => InstanceSignal::AccessDenied,
    }
}

/// Ensure only one instance of the application is running. On success this
/// process is the instance and `event_watcher` forwards wake-up signals to
/// `command_tx`.
pub fn ensure_single_instance(
    command_tx: Sender<TrayCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mutex_name = wide_null(MUTEX_NAME);

    let mutex = unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) }
        .map_err(|error| format!("CreateMutexW failed: {error}"))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return Err("Application is already running".into());
    }

    let event_name = wide_null(SHOW_EVENT_NAME);
    let event = unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) }
        .map_err(|error| format!("CreateEventW failed: {error}"))?;
    spawn_event_watcher(event, command_tx)?;

    // The mutex handle is intentionally leaked to preserve the process-lifetime lock.
    let _ = mutex;

    Ok(())
}

/// A raw Win32 handle is a process-wide token; the Win32 API contract allows
/// using it from any thread, so it is safe to move into the watcher thread.
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

fn spawn_event_watcher(
    event: HANDLE,
    command_tx: Sender<TrayCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let wrapped = SendHandle(event);
    std::thread::Builder::new()
        .name("single-instance-watcher".into())
        .spawn(move || watcher_loop(wrapped, command_tx))
        .map_err(|error| {
            unsafe {
                let _ = CloseHandle(event);
            }
            format!("single-instance watcher start failed: {error}")
        })?;
    Ok(())
}

/// Body of the watcher thread. Taking the wrapped handle as a by-value
/// argument sidesteps closure field capture, which would move the raw
/// `HANDLE` (not `Send`) into the thread.
fn watcher_loop(event: SendHandle, command_tx: Sender<TrayCommand>) {
    let event = event.0;
    loop {
        let result = unsafe { WaitForSingleObject(event, INFINITE) };
        if result != WAIT_OBJECT_0 {
            break;
        }
        if command_tx.send(TrayCommand::ActivateWindow).is_err() {
            break;
        }
    }
    unsafe {
        let _ = CloseHandle(event);
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

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_event_name_is_derived_from_mutex_name() {
        assert!(SHOW_EVENT_NAME.starts_with(MUTEX_NAME));
        assert!(SHOW_EVENT_NAME.ends_with("_ShowWindow"));
    }

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
