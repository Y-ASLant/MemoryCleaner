use std::sync::mpsc::Sender;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    CreateMemoryResourceNotification, HighMemoryResourceNotification, LowMemoryResourceNotification,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

use crate::tray::TrayCommand;

/// Owns a Windows memory-resource notification handle.
struct MemoryNotification(HANDLE);

impl MemoryNotification {
    fn create_low() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) }
            .map(Self)
            .context("CreateMemoryResourceNotification(LowMemoryResourceNotification) failed")
    }

    fn create_high() -> Result<Self> {
        unsafe { CreateMemoryResourceNotification(HighMemoryResourceNotification) }
            .map(Self)
            .context("CreateMemoryResourceNotification(HighMemoryResourceNotification) failed")
    }

    fn wait(&self) -> Result<()> {
        let result = unsafe { WaitForSingleObject(self.0, INFINITE) };
        if result == WAIT_OBJECT_0 {
            Ok(())
        } else {
            bail!("WaitForSingleObject returned {result:?}")
        }
    }
}

impl Drop for MemoryNotification {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Starts a process-lifetime monitor that emits at most one cleanup command for each
/// low-to-high memory-pressure cycle. The operating system blocks the thread while
/// memory is not under pressure; no periodic polling is performed.
pub fn start(command_tx: Sender<TrayCommand>) {
    let result = std::thread::Builder::new()
        .name("low-memory-monitor".into())
        .spawn(move || {
            let result = run(command_tx);
            if let Err(error) = result {
                crate::log_msg(&format!("[memory-notification] monitor stopped: {error:#}"));
            }
        });

    if let Err(error) = result {
        crate::log_msg(&format!(
            "[memory-notification] monitor start failed: {error}"
        ));
    }
}

fn run(command_tx: Sender<TrayCommand>) -> Result<()> {
    let low = MemoryNotification::create_low()?;
    let high = MemoryNotification::create_high()?;

    loop {
        low.wait()?;
        if command_tx.send(TrayCommand::LowMemory).is_err() {
            return Ok(());
        }

        // Notification handles are level-triggered. Wait for high memory before rearming
        // low-memory cleanup so sustained pressure cannot produce a busy loop or cache churn.
        high.wait()?;
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
}
