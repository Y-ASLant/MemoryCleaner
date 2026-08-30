//! Auto-cleanup trigger policy for the configurable memory-usage threshold.
//! The OS low-memory notification lives in `win32::memory_notification`.

use std::time::Duration;

/// How often the auto-cleanup monitor re-evaluates its triggers.
pub const AUTO_CLEANUP_POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Consecutive polls above the threshold required before a threshold cleanup
/// fires, so a transient spike does not trigger a cleanup.
pub const AUTO_CLEANUP_SUSTAINED_TICKS: u32 = 2;
/// Minimum spacing between threshold-triggered automatic cleanups.
pub const AUTO_CLEANUP_THRESHOLD_COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// What requested an automatic cleanup; recorded in the debug log.
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

/// Whether the threshold trigger should fire on this poll.
pub fn threshold_trigger_due(above_threshold_ticks: u32, cooldown_elapsed: bool) -> bool {
    above_threshold_ticks >= AUTO_CLEANUP_SUSTAINED_TICKS && cooldown_elapsed
}

/// Whether a threshold cleanup may run, given its most recent automatic run.
///
/// A first cleanup has no prior run to cool down from.
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
