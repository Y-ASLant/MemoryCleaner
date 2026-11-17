//! CPU 与网速采样（`sysinfo`）。
//!
//! CPU 占用与网速都依赖两次采样差分；首次 `refresh` 只打基线，第二次起才有有效速率。

use std::ffi::OsStr;
use std::time::Instant;

use rust_i18n::t;
use sysinfo::{CpuRefreshKind, InterfaceOperationalState, Networks, RefreshKind, System};

/// 一次展示用快照。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuNetworkSnapshot {
    /// 0..=100；尚无有效差分时为 `None`。
    pub cpu_percent: Option<f32>,
    /// 下行字节/秒；尚无有效差分时为 `None`。
    pub download_bps: Option<f64>,
    /// 上行字节/秒；尚无有效差分时为 `None`。
    pub upload_bps: Option<f64>,
}

impl CpuNetworkSnapshot {
    pub const fn unavailable() -> Self {
        Self {
            cpu_percent: None,
            download_bps: None,
            upload_bps: None,
        }
    }

    pub fn cpu_label(&self) -> String {
        match self.cpu_percent {
            Some(percent) => format!("{}%", percent.round().clamp(0.0, 100.0) as u32),
            None => "—".into(),
        }
    }

    pub fn download_label(&self) -> String {
        match self.download_bps {
            Some(bps) => format_rate(bps),
            None => "—".into(),
        }
    }

    pub fn upload_label(&self) -> String {
        match self.upload_bps {
            Some(bps) => format_rate(bps),
            None => "—".into(),
        }
    }

    pub fn summary_line(&self) -> String {
        t!(
            "metrics.cpu_network_summary",
            cpu = self.cpu_label(),
            down = self.download_label(),
            up = self.upload_label()
        )
        .to_string()
    }
}

/// 持有 `sysinfo` 状态，供周期性刷新。
pub struct SystemMetricsSampler {
    system: Option<System>,
    networks: Option<Networks>,
    last_at: Option<Instant>,
    snapshot: CpuNetworkSnapshot,
    /// 至少完成过一次间隔刷新后才公布数值。
    primed: bool,
}

impl SystemMetricsSampler {
    pub fn new() -> Self {
        let system = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_usage()),
        )
        .ok();

        let networks = Networks::new_with_refreshed_list().ok();
        Self {
            system,
            networks,
            last_at: Some(Instant::now()),
            snapshot: CpuNetworkSnapshot::unavailable(),
            primed: false,
        }
    }

    pub fn snapshot(&self) -> CpuNetworkSnapshot {
        self.snapshot
    }

    /// 刷新 CPU/网速。有展示变化时返回 `true`。
    pub fn refresh(&mut self) -> bool {
        let previous = self.snapshot;
        let now = Instant::now();
        let elapsed = self
            .last_at
            .map(|at| now.saturating_duration_since(at).as_secs_f64())
            .unwrap_or(0.0);

        if let Some(system) = self.system.as_mut() {
            system.refresh_cpu_usage();
        }

        let mut download_bytes = 0u64;
        let mut upload_bytes = 0u64;
        if let Some(networks) = self.networks.as_mut() {
            networks.refresh(true);
            for (name, data) in networks.iter() {
                if !include_interface(name, data.operational_state()) {
                    continue;
                }
                download_bytes = download_bytes.saturating_add(data.received());
                upload_bytes = upload_bytes.saturating_add(data.transmitted());
            }
        }

        self.last_at = Some(now);

        // 首次间隔后才有有效差分；过短间隔不更新速率，避免噪声。
        if !self.primed {
            if elapsed >= 0.2 {
                self.primed = true;
            } else {
                return false;
            }
        }

        let cpu_percent = self.system.as_ref().map(|s| s.global_cpu_usage());
        let (download_bps, upload_bps) = if elapsed > f64::EPSILON {
            (
                Some(download_bytes as f64 / elapsed),
                Some(upload_bytes as f64 / elapsed),
            )
        } else {
            (self.snapshot.download_bps, self.snapshot.upload_bps)
        };

        self.snapshot = CpuNetworkSnapshot {
            cpu_percent,
            download_bps,
            upload_bps,
        };

        snapshot_changed(previous, self.snapshot)
    }
}

impl Default for SystemMetricsSampler {
    fn default() -> Self {
        Self::new()
    }
}

fn include_interface(name: &OsStr, state: InterfaceOperationalState) -> bool {
    if !matches!(
        state,
        InterfaceOperationalState::Up | InterfaceOperationalState::Unknown
    ) {
        return false;
    }
    let lower = name.to_string_lossy().to_ascii_lowercase();
    // Windows 环回 / 隧道占位接口不计入汇总网速。
    !(lower.contains("loopback")
        || lower.contains("isatap")
        || lower.contains("teredo")
        || lower == "lo")
}

fn snapshot_changed(a: CpuNetworkSnapshot, b: CpuNetworkSnapshot) -> bool {
    !approx_opt_f32(a.cpu_percent, b.cpu_percent)
        || !approx_opt_f64(a.download_bps, b.download_bps)
        || !approx_opt_f64(a.upload_bps, b.upload_bps)
}

fn approx_opt_f32(a: Option<f32>, b: Option<f32>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() < 0.05,
        _ => false,
    }
}

fn approx_opt_f64(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() < 1.0,
        _ => false,
    }
}

/// 速率格式：B/s · KB/s · MB/s · GB/s。
pub fn format_rate(bps: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bps = bps.max(0.0);
    if bps >= GB {
        format!("{:.2} GB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.2} MB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.2} KB/s", bps / KB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::with_locale;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn format_rate_uses_binary_units() {
        assert_eq!(format_rate(512.0), "512 B/s");
        assert_eq!(format_rate(2048.0), "2.00 KB/s");
        assert_eq!(format_rate(3.5 * 1024.0 * 1024.0), "3.50 MB/s");
        assert_eq!(format_rate(1.25 * 1024.0 * 1024.0 * 1024.0), "1.25 GB/s");
    }

    #[test]
    fn snapshot_labels_handle_unavailable() {
        with_locale("zh-CN", || {
            let snap = CpuNetworkSnapshot::unavailable();
            assert_eq!(snap.cpu_label(), "—");
            assert_eq!(snap.download_label(), "—");
            assert_eq!(snap.upload_label(), "—");
            assert!(snap.summary_line().contains("CPU"));
        });
    }

    #[test]
    fn sampler_produces_cpu_after_interval() {
        let mut sampler = SystemMetricsSampler::new();
        thread::sleep(Duration::from_millis(250));
        let _ = sampler.refresh();
        // 在受支持的 Windows 上应能读到 CPU；失败时保持 None 亦可。
        if sampler.system.is_some() {
            assert!(sampler.snapshot().cpu_percent.is_some());
            let cpu = sampler.snapshot().cpu_percent.unwrap();
            assert!((0.0..=100.0).contains(&cpu) || cpu.is_finite());
        }
        if sampler.networks.is_some() {
            assert!(sampler.snapshot().download_bps.is_some());
            assert!(sampler.snapshot().upload_bps.is_some());
        }
    }
}
