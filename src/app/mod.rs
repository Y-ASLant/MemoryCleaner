use rust_i18n::t;

use std::time::{Duration, Instant};

use anyhow::Result;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::{Root, TitleBar, WindowExt};
use gpui_kit::*;
use smol::Timer;

use crate::anim::{AnimatedValue, SampledAnimatedValue, TimedAnimatedValue, ease_out_cubic};
use crate::auto_cleanup::{
    AUTO_CLEANUP_POLL_INTERVAL, AutoCleanupSource, threshold_cooldown_elapsed,
    threshold_trigger_due,
};
use crate::locale;
use crate::memory::{MemorySection, MemoryStatus};
use crate::messages::{build_cleanup_result_message, format_freed_message};
use crate::optimize::{self, MemoryAreas};
use crate::settings::Settings;
use crate::tray::{TrayCommand, dispatch_command};
use crate::ui::layout::{SECTION_GAP, settings_reveal_height};
use crate::win32;

mod optimization;
mod render;
mod settings;
mod window;

pub use window::{
    AppEntityHolder, open_main_window, window_height, window_min_size, window_options, window_size,
};

const SETTINGS_SAVE_DEBOUNCE: Duration = Duration::from_millis(300);
const OPTIMIZE_RESULT_DISPLAY: Duration = Duration::from_secs(5);
const MEMORY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_INTERPOLATION_DURATION_SECS: f32 = MEMORY_REFRESH_INTERVAL.as_secs_f32();
const fn memory_polling_enabled(window_visible: bool) -> bool {
    window_visible
}
const fn threshold_polling_enabled(auto_cleanup_enabled: bool, threshold: u32) -> bool {
    auto_cleanup_enabled && threshold > 0
}

const WINDOW_WIDTH: f32 = 520.;
const WINDOW_MIN_WIDTH: f32 = 520.;
pub const CONTENT_PADDING: f32 = 6.;
const SETTINGS_PANEL_VISIBLE_EPSILON: f32 = 0.01;
const SETTINGS_EXPAND_DURATION_SECS: f32 = 0.22;

fn query_sections() -> Result<(MemorySection, MemorySection)> {
    let status = MemoryStatus::query()?;

    let physical = MemorySection {
        title: t!("memory.physical").to_string(),
        total: status.total_phys,
        used: status.used_phys(),
        avail: status.avail_phys,
        used_percent: status.memory_load as f32,
    };

    let virt_used = status
        .total_page_file
        .saturating_sub(status.avail_page_file);
    let virt_percent = if status.total_page_file > 0 {
        (virt_used as f64 / status.total_page_file as f64 * 100.0).round() as u32
    } else {
        0
    };
    let virtual_mem = MemorySection {
        title: t!("memory.virtual").to_string(),
        total: status.total_page_file,
        used: virt_used,
        avail: status.avail_page_file,
        used_percent: virt_percent as f32,
    };

    Ok((physical, virtual_mem))
}

pub struct MemoryCleanerApp {
    pub window: Option<AnyWindowHandle>,
    pub settings: Settings,
    pub physical: MemorySection,
    pub virtual_mem: MemorySection,
    settings_save_gen: u32,
    memory_refresh_task: Option<Task<()>>,
    window_opening: bool,
    pub is_optimizing: bool,
    pub is_refreshing_icon_cache: bool,
    pub optimize_step: String,
    pub optimize_status: String,
    pub optimize_has_errors: bool,
    pub icon_cache_status: String,
    pub settings_expanded: bool,
    window_shown: bool,
    /// When the last automatic cleanup ran; anchors the threshold cooldown.
    last_auto_cleanup: Option<Instant>,
    threshold_cleanup_task: Option<Task<()>>,
    command_tx: std::sync::mpsc::Sender<TrayCommand>,
    low_memory_monitor: Option<win32::memory_notification::MemoryNotificationMonitor>,
    pub cleanup_hotkey_recording: bool,
    pub(crate) cleanup_hotkey_failed: bool,
    pub(crate) startup_setting_pending: bool,
    pub(crate) startup_setting_failed: bool,
    pub(crate) hotkey_capture_focus: FocusHandle,
    anim_physical: SampledAnimatedValue,
    anim_virtual: SampledAnimatedValue,
    anim_optimize: AnimatedValue,
    anim_used_phys: SampledAnimatedValue,
    anim_avail_phys: SampledAnimatedValue,
    anim_used_virt: SampledAnimatedValue,
    anim_avail_virt: SampledAnimatedValue,
    anim_settings_expand: TimedAnimatedValue,
    anim_dirty: bool,
    /// Wall-clock of the previous interpolator tick (`None` when settled).
    last_anim_tick: Option<Instant>,
    /// Last known window content height (updated on every resize).
    current_window_height: f32,
}

impl MemoryCleanerApp {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Settings,
        command_tx: std::sync::mpsc::Sender<TrayCommand>,
        tray_rx: std::sync::mpsc::Receiver<TrayCommand>,
        launch_hidden: bool,
    ) -> Self {
        crate::log::set_debug_enabled(settings.debug_logging);
        if settings.debug_logging {
            crate::log::write(&t!(
                "log.debug_enabled",
                path = crate::log::log_file_path().display().to_string()
            ));
        }

        let (physical, virtual_mem) = query_sections().unwrap_or_else(|e| {
            crate::log_msg(&format!("[memory] initial query failed: {e}"));
            (
                MemorySection::unavailable(&t!("memory.physical")),
                MemorySection::unavailable(&t!("memory.virtual")),
            )
        });

        let phys_percent = physical.used_percent;
        let phys_used = physical.used as f32;
        let phys_avail = physical.avail as f32;
        let virt_percent = virtual_mem.used_percent;
        let virt_used = virtual_mem.used as f32;
        let virt_avail = virtual_mem.avail as f32;

        let mut app = Self {
            window: None,
            settings,
            physical,
            virtual_mem,
            settings_save_gen: 0,
            memory_refresh_task: None,
            window_opening: false,
            is_optimizing: false,
            is_refreshing_icon_cache: false,
            optimize_step: String::new(),
            optimize_status: String::new(),
            optimize_has_errors: false,
            icon_cache_status: String::new(),
            settings_expanded: false,
            window_shown: !launch_hidden,
            last_auto_cleanup: None,
            threshold_cleanup_task: None,
            command_tx,
            low_memory_monitor: None,
            cleanup_hotkey_recording: false,
            cleanup_hotkey_failed: false,
            startup_setting_pending: false,
            startup_setting_failed: false,
            hotkey_capture_focus: cx.focus_handle(),
            anim_physical: SampledAnimatedValue::new(
                phys_percent,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_virtual: SampledAnimatedValue::new(
                virt_percent,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_optimize: AnimatedValue::new(0.0),
            anim_used_phys: SampledAnimatedValue::new(
                phys_used,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_avail_phys: SampledAnimatedValue::new(
                phys_avail,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_used_virt: SampledAnimatedValue::new(
                virt_used,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_avail_virt: SampledAnimatedValue::new(
                virt_avail,
                MEMORY_INTERPOLATION_DURATION_SECS,
            ),
            anim_settings_expand: TimedAnimatedValue::new(0.0, SETTINGS_EXPAND_DURATION_SECS),
            anim_dirty: false,
            last_anim_tick: None,
            current_window_height: window_height(false),
        };

        cx.set_global(AppEntityHolder(cx.entity()));
        app.attach_window(window, cx, launch_hidden);
        app.sync_cleanup_hotkey();
        app.set_run_at_startup(app.settings.run_at_startup, cx);
        app.start_background_tasks(cx, tray_rx);

        app
    }
}

#[cfg(test)]
mod tests {
    use super::{memory_polling_enabled, threshold_polling_enabled};

    #[test]
    fn memory_polling_follows_window_visibility() {
        assert!(memory_polling_enabled(true));
        assert!(!memory_polling_enabled(false));
    }

    #[test]
    fn threshold_polling_requires_enabled_nonzero_threshold() {
        assert!(threshold_polling_enabled(true, 80));
        assert!(!threshold_polling_enabled(true, 0));
        assert!(!threshold_polling_enabled(false, 80));
    }
}
