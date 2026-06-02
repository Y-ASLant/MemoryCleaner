use anyhow::{Context, Result};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

#[derive(Debug, Clone)]
pub struct MemorySection {
    pub header: String,
    pub used_label: String,
    pub free_label: String,
    pub used_percent: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryStatus {
    pub memory_load: u32,
    pub total_phys: u64,
    pub avail_phys: u64,
    pub total_page_file: u64,
    pub avail_page_file: u64,
}

impl MemoryStatus {
    pub fn query() -> Result<Self> {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };

        unsafe {
            GlobalMemoryStatusEx(&mut status).context("GlobalMemoryStatusEx failed")?;
        }

        Ok(Self {
            memory_load: status.dwMemoryLoad,
            total_phys: status.ullTotalPhys,
            avail_phys: status.ullAvailPhys,
            total_page_file: status.ullTotalPageFile,
            avail_page_file: status.ullAvailPageFile,
        })
    }

    pub fn used_phys(&self) -> u64 {
        self.total_phys.saturating_sub(self.avail_phys)
    }

    pub fn format_bytes(bytes: u64) -> String {
        const GB: u64 = 1024 * 1024 * 1024;
        const MB: u64 = 1024 * 1024;

        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else {
            format!("{:.0} MB", bytes as f64 / MB as f64)
        }
    }
}
