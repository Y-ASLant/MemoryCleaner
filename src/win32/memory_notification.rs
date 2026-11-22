use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    CreateMemoryResourceNotification, HighMemoryResourceNotification, LowMemoryResourceNotification,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};

use crate::tray::{CommandTx, TrayCommand};
use crate::win32::handle::OwnedWin32Handle;

/// 持有 Windows 内存资源通知句柄。
struct MemoryNotification(OwnedWin32Handle);

impl MemoryNotification {
    fn create_low() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }
            .map(|handle| Self(unsafe { OwnedWin32Handle::from_raw(handle) }))
            .context("CreateMemoryResourceNotification(LowMemoryResourceNotification) failed")
    }

    fn create_high() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(HighMemoryResourceNotification) }
            .map(|handle| Self(unsafe { OwnedWin32Handle::from_raw(handle) }))
            .context("CreateMemoryResourceNotification(HighMemoryResourceNotification) failed")
    }

    fn handle(&self) -> HANDLE {
        self.0.raw()
    }
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

/// 可取消的低内存监视器。drop 时立即唤醒并 join worker。
pub struct MemoryNotificationMonitor {
    stop_event: Arc<OwnedWin32Handle>,
    worker: Option<JoinHandle<()>>,
}

impl MemoryNotificationMonitor {
    pub fn start(command_tx: CommandTx) -> Result<Self> {
        let stop_event = Arc::new(unsafe {
            OwnedWin32Handle::from_raw(
                CreateEventW(None, true, false, None).context("CreateEventW(stop) failed")?,
            )
        });
        let low = MemoryNotification::create_low()?;
        let high = MemoryNotification::create_high()?;
        let worker_stop_event = Arc::clone(&stop_event);
        let worker = thread::Builder::new()
            .name("low-memory-monitor".into())
            .spawn(move || {
                if let Err(error) = run(command_tx, worker_stop_event.raw(), low, high) {
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
            let _ = SetEvent(self.stop_event.raw());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    command_tx: CommandTx,
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

        // 通知句柄为电平触发。先等高内存再重新武装低内存清理，
        // 避免持续压力导致忙等或缓存抖动。
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
        let (command_tx, _command_rx) = crate::tray::command_channel().expect("command channel");
        let monitor =
            MemoryNotificationMonitor::start(command_tx).expect("start low-memory monitor");
        drop(monitor);
    }
}
