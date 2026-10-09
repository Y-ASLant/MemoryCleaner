use anyhow::{Context, Result};
use rust_i18n::t;
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

#[derive(Debug, Clone, PartialEq)]
pub struct MemorySection {
    pub title: String,
    pub total: u64,
    pub used: u64,
    pub avail: u64,
    pub used_percent: f32,
}

impl MemorySection {
    pub fn header(&self) -> String {
        format!(
            "{} ({})",
            self.title,
            MemoryStatus::format_bytes(self.total)
        )
    }

    pub fn usage_summary(&self) -> String {
        if self.total == 0 {
            return "—".into();
        }
        t!(
            "memory.used_avail",
            used = MemoryStatus::format_bytes(self.used),
            avail = MemoryStatus::format_bytes(self.avail),
        )
        .to_string()
    }

    pub fn unavailable(title: &str) -> Self {
        Self {
            title: title.into(),
            total: 0,
            used: 0,
            avail: 0,
            used_percent: 0.0,
        }
    }

    pub fn is_unavailable(&self) -> bool {
        self.total == 0
    }

    pub fn percent_label(&self) -> String {
        if self.is_unavailable() {
            "—".into()
        } else {
            format!("{}%", self.used_percent.round() as u32)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemWorkingSet {
    pub current: u64,
    pub peak: u64,
}

impl SystemWorkingSet {
    pub fn query() -> Result<Self> {
        let info = crate::win32::nt::query_file_cache_information()?;
        Ok(Self {
            current: info.current_size as u64,
            peak: info.peak_size as u64,
        })
    }

    pub fn percent_of_peak(&self) -> Option<f64> {
        (self.peak > 0).then(|| self.current as f64 / self.peak as f64 * 100.0)
    }

    pub fn header(&self) -> String {
        t!(
            "memory.system_working_set_peak",
            peak = MemoryStatus::format_bytes(self.peak),
        )
        .to_string()
    }

    pub fn summary(&self) -> String {
        t!(
            "memory.current",
            current = MemoryStatus::format_bytes(self.current)
        )
        .to_string()
    }

    pub fn percent_label(&self) -> String {
        self.percent_of_peak()
            .map(|percent| {
                t!("memory.percent_of_peak", percent = format!("{percent:.1}")).to_string()
            })
            .unwrap_or_else(|| "—".into())
    }
}

pub const WORKING_SET_HISTORY_SECS: u64 = 30;

#[derive(Debug, Clone, Copy)]
pub struct WorkingSetSample {
    pub at: std::time::Instant,
    pub working_set: SystemWorkingSet,
}

#[derive(Default)]
pub struct WorkingSetHistory {
    samples: std::collections::VecDeque<WorkingSetSample>,
    started_at: Option<std::time::Instant>,
}

impl WorkingSetHistory {
    pub fn samples(&self) -> &std::collections::VecDeque<WorkingSetSample> {
        &self.samples
    }

    pub fn current(&self) -> Option<&SystemWorkingSet> {
        self.samples.back().map(|sample| &sample.working_set)
    }

    pub fn elapsed_seconds(&self, at: std::time::Instant) -> u64 {
        self.started_at
            .map(|start| at.saturating_duration_since(start).as_secs())
            .unwrap_or(0)
    }

    pub fn record(&mut self, at: std::time::Instant, value: Option<SystemWorkingSet>) -> bool {
        let Some(value) = value else {
            let changed = !self.samples.is_empty();
            self.samples.clear();
            self.started_at = None;
            return changed;
        };
        if let Some(last) = self.samples.back_mut() {
            let elapsed = at.saturating_duration_since(last.at);
            if elapsed < std::time::Duration::from_secs(1) {
                let changed = last.working_set != value;
                last.working_set = value;
                return changed;
            }
            // A paused or failed poll must not appear as a continuous trend.
            if elapsed > std::time::Duration::from_secs(2) {
                self.samples.clear();
                self.started_at = None;
            }
        }
        self.started_at.get_or_insert(at);
        self.samples.push_back(WorkingSetSample {
            at,
            working_set: value,
        });
        while self.samples.front().is_some_and(|sample| {
            at.saturating_duration_since(sample.at).as_secs() > WORKING_SET_HISTORY_SECS
        }) || self.samples.len() > WORKING_SET_HISTORY_SECS as usize + 1
        {
            self.samples.pop_front();
        }
        true
    }
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

        let mut performance = PERFORMANCE_INFORMATION::default();
        unsafe {
            GetPerformanceInfo(
                &mut performance,
                std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            )
            .context("GetPerformanceInfo failed")?;
        }
        let total_commit =
            (performance.CommitLimit as u64).saturating_mul(performance.PageSize as u64);
        let used_commit =
            (performance.CommitTotal as u64).saturating_mul(performance.PageSize as u64);
        Ok(Self {
            memory_load: status.dwMemoryLoad,
            total_phys: status.ullTotalPhys,
            avail_phys: status.ullAvailPhys,
            total_page_file: total_commit,
            avail_page_file: total_commit.saturating_sub(used_commit),
        })
    }

    pub fn used_phys(&self) -> u64 {
        self.total_phys.saturating_sub(self.avail_phys)
    }

    pub fn format_bytes(bytes: u64) -> String {
        const GB: u64 = 1024 * 1024 * 1024;
        const MB: u64 = 1024 * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::with_locale;

    fn sample_section(used_percent: f32) -> MemorySection {
        MemorySection {
            title: "物理内存".into(),
            total: 8 * 1024 * 1024 * 1024,
            used: 4 * 1024 * 1024 * 1024,
            avail: 4 * 1024 * 1024 * 1024,
            used_percent,
        }
    }

    #[test]
    fn query_reads_physical_and_system_commit_memory() {
        let status = MemoryStatus::query().expect("read memory status");
        assert!(status.total_phys > 0);
        assert!(status.avail_phys <= status.total_phys);
        assert!(status.total_page_file > 0);
        assert!(status.avail_page_file <= status.total_page_file);
    }

    #[test]
    fn system_working_set_ratio_uses_peak_and_handles_zero() {
        let working_set = SystemWorkingSet {
            current: 256,
            peak: 1024,
        };
        assert_eq!(working_set.percent_of_peak(), Some(25.0));
        assert_eq!(
            SystemWorkingSet {
                current: 0,
                peak: 0
            }
            .percent_of_peak(),
            None
        );
        assert_eq!(
            SystemWorkingSet {
                current: 0,
                peak: 1024
            }
            .percent_of_peak(),
            Some(0.0)
        );
    }

    #[test]
    fn working_set_history_is_bounded_and_records_unchanged_values() {
        let start = std::time::Instant::now();
        let value = Some(SystemWorkingSet {
            current: 1024,
            peak: 2048,
        });
        let mut history = WorkingSetHistory::default();
        for second in 0..120 {
            assert!(history.record(start + std::time::Duration::from_secs(second), value));
            let first = history.samples().front().unwrap();
            let last = history.samples().back().unwrap();
            assert_eq!(history.elapsed_seconds(first.at), second.saturating_sub(30));
            assert_eq!(history.elapsed_seconds(last.at), second);
        }
        assert_eq!(history.samples().len(), 31);
        assert_eq!(
            history.samples().front().unwrap().at,
            start + std::time::Duration::from_secs(89)
        );
        assert!(!history.record(start + std::time::Duration::from_millis(119_500), value));
        assert_eq!(history.samples().len(), 31);
    }

    #[test]
    fn working_set_history_preserves_peak_for_each_sample() {
        let start = std::time::Instant::now();
        let mut history = WorkingSetHistory::default();
        history.record(
            start,
            Some(SystemWorkingSet {
                current: 256,
                peak: 1024,
            }),
        );
        history.record(
            start + std::time::Duration::from_secs(1),
            Some(SystemWorkingSet {
                current: 256,
                peak: 2048,
            }),
        );
        assert_eq!(
            history.samples()[0].working_set.percent_of_peak(),
            Some(25.0)
        );
        assert_eq!(
            history.samples()[1].working_set.percent_of_peak(),
            Some(12.5)
        );
        assert!(history.record(
            start + std::time::Duration::from_millis(1500),
            Some(SystemWorkingSet {
                current: 256,
                peak: 4096
            }),
        ));
        assert_eq!(history.samples().len(), 2);
        assert_eq!(
            history.samples()[1].working_set.percent_of_peak(),
            Some(6.25)
        );
        assert_eq!(history.current().unwrap().percent_of_peak(), Some(6.25));
    }

    #[test]
    fn working_set_history_restarts_after_missing_or_paused_samples() {
        let start = std::time::Instant::now();
        let value = Some(SystemWorkingSet {
            current: 1024,
            peak: 2048,
        });
        let mut history = WorkingSetHistory::default();
        assert_eq!(history.current(), None);
        history.record(start, value);
        assert_eq!(history.current().copied(), value);
        history.record(start + std::time::Duration::from_secs(1), value);
        assert!(history.record(start + std::time::Duration::from_secs(2), None));
        assert!(history.samples().is_empty());
        assert_eq!(history.current(), None);
        history.record(start + std::time::Duration::from_secs(3), value);
        assert_eq!(history.elapsed_seconds(history.samples()[0].at), 0);
        history.record(start + std::time::Duration::from_secs(4), value);
        assert_eq!(history.elapsed_seconds(history.samples()[1].at), 1);
        history.record(start + std::time::Duration::from_secs(10), value);
        assert_eq!(history.samples().len(), 1);
        assert_eq!(history.elapsed_seconds(history.samples()[0].at), 0);
    }

    #[test]
    fn query_reads_system_working_set() {
        let working_set = SystemWorkingSet::query().expect("read system working set");
        assert!(working_set.current <= working_set.peak);
    }

    #[test]
    fn format_bytes_uses_gb_and_mb() {
        assert_eq!(
            MemoryStatus::format_bytes(2 * 1024 * 1024 * 1024),
            "2.00 GB"
        );
        assert_eq!(MemoryStatus::format_bytes(512 * 1024 * 1024), "512.00 MB");
    }

    #[test]
    fn percent_label_rounds_and_handles_unavailable() {
        assert_eq!(sample_section(45.4).percent_label(), "45%");
        assert_eq!(sample_section(45.6).percent_label(), "46%");
        assert_eq!(MemorySection::unavailable("物理内存").percent_label(), "—");
    }

    #[test]
    fn usage_summary_formats_used_and_available_zh() {
        with_locale("zh-CN", || {
            let summary = sample_section(50.0).usage_summary();
            assert!(summary.contains("已用 4.00 GB"));
            assert!(summary.contains("可用 4.00 GB"));
            assert_eq!(MemorySection::unavailable("物理内存").usage_summary(), "—");
        });
    }

    #[test]
    fn usage_summary_formats_used_and_available_en() {
        with_locale("en", || {
            let summary = sample_section(50.0).usage_summary();
            assert!(summary.contains("Used 4.00 GB"));
            assert!(summary.contains("Available 4.00 GB"));
            assert_eq!(
                MemorySection::unavailable("Physical Memory").usage_summary(),
                "—"
            );
        });
    }
}
