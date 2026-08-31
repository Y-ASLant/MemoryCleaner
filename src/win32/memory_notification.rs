use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Arc, mpsc::Sender};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    CreateMemoryResourceNotification, HighMemoryResourceNotification, LowMemoryResourceNotification,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};

use crate::tray::TrayCommand;

/// Owns a Windows memory-resource notification handle.
struct MemoryNotification(OwnedHandle);

impl MemoryNotification {
    fn create_low() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }
            .map(|handle| Self(owned_handle(handle)))
            .context("CreateMemoryResourceNotification(LowMemoryResourceNotification) failed")
    }

    fn create_high() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(HighMemoryResourceNotification) }
            .map(|handle| Self(owned_handle(handle)))
            .context("CreateMemoryResourceNotification(HighMemoryResourceNotification) failed")
    }

    fn handle(&self) -> HANDLE {
        handle_from_owned(&self.0)
    }
}

fn owned_handle(handle: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

fn handle_from_owned(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitOutcome {
    Notification,
    Stop,
}

fn wait_or_stop(notification: &MemoryNotification, stop_event: HANDLE) -> Result<WaitOutcome> {
    let result =
        unsafe { WaitForMultipleObjects(&[notification.handle(), stop_event], false, INFINITE) };
    match result.0 {
        value if value == WAIT_OBJECT_0.0 => Ok(WaitOutcome::Notification),
        value if value == WAIT_OBJECT_0.0 + 1 => Ok(WaitOutcome::Stop),
        _ => bail!("WaitForMultipleObjects returned {result:?}"),
    }
}

/// Cancellable low-memory monitor. Dropping it wakes and joins its worker immediately.
pub struct MemoryNotificationMonitor {
    stop_event: Arc<OwnedHandle>,
    worker: Option<JoinHandle<()>>,
}

impl MemoryNotificationMonitor {
    pub fn start(command_tx: Sender<TrayCommand>) -> Result<Self> {
        let stop_event = Arc::new(owned_handle(
            unsafe { CreateEventW(None, true, false, None) }
                .context("CreateEventW(stop) failed")?,
        ));
        let low = MemoryNotification::create_low()?;
        let high = MemoryNotification::create_high()?;
        let worker_stop_event = Arc::clone(&stop_event);
        let worker = thread::Builder::new()
            .name("low-memory-monitor".into())
            .spawn(move || {
                if let Err(error) =
                    run(command_tx, handle_from_owned(&worker_stop_event), low, high)
                {
                    crate::log_msg(&format!("[memory-notification] monitor stopped: {error:#}"));
                }
            })
            .context("failed to spawn low-memory monitor")?;

        Ok(Self {
            stop_event,
            worker: Some(worker),
        })
    }
}

impl Drop for MemoryNotificationMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(handle_from_owned(&self.stop_event));
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    command_tx: Sender<TrayCommand>,
    stop_event: HANDLE,
    low: MemoryNotification,
    high: MemoryNotification,
) -> Result<()> {
    loop {
        if wait_or_stop(&low, stop_event)? == WaitOutcome::Stop {
            return Ok(());
        }
        if command_tx.send(TrayCommand::LowMemory).is_err() {
            return Ok(());
        }

        // Notification handles are level-triggered. Wait for high memory before rearming
        // low-memory cleanup so sustained pressure cannot produce a busy loop or cache churn.
        if wait_or_stop(&high, stop_event)? == WaitOutcome::Stop {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_low_and_high_memory_notifications() {
        let _low = MemoryNotification::create_low().expect("create low-memory notification");
        let _high = MemoryNotification::create_high().expect("create high-memory notification");
    }

    #[test]
    fn monitor_stops_when_dropped() {
        let (command_tx, _command_rx) = std::sync::mpsc::channel();
        let monitor =
            MemoryNotificationMonitor::start(command_tx).expect("start low-memory monitor");
        drop(monitor);
    }
}
