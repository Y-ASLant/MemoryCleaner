//! 可配置内存占用阈值的自动清理触发策略。
//! 系统低内存通知见 `win32::memory_notification`。

use std::time::Duration;

/// 自动清理监视器重新评估触发条件的间隔。
pub const AUTO_CLEANUP_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// 连续高于阈值多少次轮询后才触发阈值清理，
/// 避免瞬时尖峰误触发。
pub const AUTO_CLEANUP_SUSTAINED_TICKS: u32 = 2;
/// 阈值触发的自动清理之间的最短间隔。
pub const AUTO_CLEANUP_THRESHOLD_COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// 自动清理的请求来源；写入调试日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCleanupSource {
    LowMemoryNotification,
    Threshold,
}

impl AutoCleanupSource {
    pub fn log_label(self) -> &'static str {
        match self {
            Self::LowMemoryNotification => "Windows low-memory notification",
            Self::Threshold => "memory usage threshold",
        }
    }
}

impl From<AutoCleanupSource> for crate::settings::CleanupHistorySource {
    fn from(value: AutoCleanupSource) -> Self {
        match value {
            AutoCleanupSource::LowMemoryNotification => Self::LowMemoryNotification,
            AutoCleanupSource::Threshold => Self::Threshold,
        }
    }
}

/// 本轮轮询是否应触发阈值清理。
pub fn threshold_trigger_due(above_threshold_ticks: u32, cooldown_elapsed: bool) -> bool {
    above_threshold_ticks >= AUTO_CLEANUP_SUSTAINED_TICKS && cooldown_elapsed
}

/// 结合最近一次自动清理时间，判断阈值清理是否可执行。
///
/// 首次清理没有冷却起点。
pub fn threshold_cooldown_elapsed(elapsed_since_cleanup: Option<Duration>) -> bool {
    elapsed_since_cleanup.is_none_or(|elapsed| elapsed >= AUTO_CLEANUP_THRESHOLD_COOLDOWN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_trigger_requires_sustained_pressure() {
        let cooldown_elapsed = true;
        assert!(!threshold_trigger_due(0, cooldown_elapsed));
        assert!(!threshold_trigger_due(
            AUTO_CLEANUP_SUSTAINED_TICKS - 1,
            cooldown_elapsed
        ));
        assert!(threshold_trigger_due(
            AUTO_CLEANUP_SUSTAINED_TICKS,
            cooldown_elapsed
        ));
    }

    #[test]
    fn threshold_trigger_requires_cooldown_elapsed() {
        assert!(!threshold_trigger_due(AUTO_CLEANUP_SUSTAINED_TICKS, false));
        assert!(threshold_trigger_due(
            AUTO_CLEANUP_SUSTAINED_TICKS + 5,
            true
        ));
    }

    #[test]
    fn initial_threshold_trigger_has_no_cooldown() {
        assert!(threshold_cooldown_elapsed(None));
        assert!(!threshold_cooldown_elapsed(Some(Duration::from_secs(
            9 * 60 + 59
        ))));
        assert!(threshold_cooldown_elapsed(Some(
            AUTO_CLEANUP_THRESHOLD_COOLDOWN
        )));
    }
}
