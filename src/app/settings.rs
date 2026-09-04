use super::*;

impl MemoryCleanerApp {
    pub fn apply_locale(&mut self, cx: &mut Context<Self>) {
        locale::apply(&self.settings);
        if let Ok((physical, virtual_mem)) = query_sections() {
            self.physical = physical;
            self.virtual_mem = virtual_mem;
        } else {
            self.physical = MemorySection::unavailable(&t!("memory.physical"));
            self.virtual_mem = MemorySection::unavailable(&t!("memory.virtual"));
        }
        self.sync_anim_targets_from_sections();
        if !self.is_optimizing {
            self.optimize_status.clear();
            self.optimize_has_errors = false;
            self.optimize_step.clear();
        }
        if !self.is_refreshing_icon_cache {
            self.icon_cache_status.clear();
        }
        self.sync_tray();
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_memory_area(&mut self, area: MemoryAreas, enabled: bool, cx: &mut Context<Self>) {
        if self.is_optimizing {
            return;
        }

        let mut areas = self.settings.memory_areas();
        if enabled {
            // Standby List variants are mutually exclusive.
            match area {
                MemoryAreas::STANDBY_LIST => {
                    areas.remove(MemoryAreas::STANDBY_LIST_LOW_PRIORITY);
                }
                MemoryAreas::STANDBY_LIST_LOW_PRIORITY => {
                    areas.remove(MemoryAreas::STANDBY_LIST);
                }
                _ => {}
            }
            areas.insert(area);
        } else {
            areas.remove(area);
        }
        self.settings.memory_areas = areas.bits();
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn add_excluded_process_by_name(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.is_optimizing {
            return;
        }
        let normalized = win32::process::normalize_process_name(name);
        if normalized.is_empty() {
            return;
        }
        if self
            .settings
            .excluded_processes
            .iter()
            .any(|existing| existing == &normalized)
        {
            return;
        }
        self.settings.excluded_processes.push(normalized);
        self.settings.excluded_processes.sort();
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn remove_excluded_process(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.is_optimizing {
            return;
        }
        let normalized = win32::process::normalize_process_name(name);
        self.settings
            .excluded_processes
            .retain(|existing| existing != &normalized);
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn open_window_behavior_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::ui::layout::{
            DIALOG_PADDING_HORIZONTAL, DIALOG_PADDING_TOP, WINDOW_BEHAVIOR_DIALOG_WIDTH,
        };
        use crate::ui::settings_page::render_window_behavior_dialog;

        self.cancel_cleanup_hotkey_recording(cx);

        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let weak = weak.clone();
            dialog
                .title(t!("dialog.window_behavior"))
                .w(px(WINDOW_BEHAVIOR_DIALOG_WIDTH))
                .pt(px(DIALOG_PADDING_TOP))
                .pb(px(CONTENT_PADDING))
                .pl(px(DIALOG_PADDING_HORIZONTAL))
                .pr(px(DIALOG_PADDING_HORIZONTAL))
                .overlay_closable(false)
                .content({
                    let weak = weak.clone();
                    move |content, _window, cx| {
                        content.child(render_window_behavior_dialog(weak.clone(), cx))
                    }
                })
        });
    }

    pub fn set_always_on_top(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = win32::window::set_always_on_top(window, enabled) {
            crate::log_msg(&format!(
                "[window] set_always_on_top({enabled}) failed: {error:#}"
            ));
            cx.notify();
            return;
        }
        self.settings.always_on_top = enabled;
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.close_to_notification_area = enabled;
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_run_at_startup(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(error) = win32::startup::set_enabled(enabled) {
            crate::log_msg(&format!(
                "[startup] set_enabled({enabled}) failed: {error:#}"
            ));
            cx.notify();
            return;
        }
        self.settings.run_at_startup = enabled;
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_show_optimization_notifications(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.show_optimization_notifications = enabled;
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_auto_cleanup_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.settings.auto_cleanup_enabled == enabled {
            return;
        }
        self.settings.auto_cleanup_enabled = enabled;
        self.sync_auto_cleanup_monitors(cx);
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_auto_cleanup_threshold(&mut self, value: u32, cx: &mut Context<Self>) {
        let value = value.min(100);
        if self.settings.auto_cleanup_threshold == value {
            return;
        }
        self.settings.auto_cleanup_threshold = value;
        self.sync_threshold_cleanup_monitor(cx);
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn set_cleanup_hotkey_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.cleanup_hotkey_enabled = enabled;
        if !enabled {
            self.cleanup_hotkey_recording = false;
        }
        win32::hotkey::sync(&self.settings);
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn start_cleanup_hotkey_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.cleanup_hotkey_enabled {
            return;
        }
        self.cleanup_hotkey_recording = true;
        window.focus(&self.hotkey_capture_focus, cx);
        cx.notify();
    }

    pub fn handle_cleanup_hotkey_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if !self.cleanup_hotkey_recording {
            return;
        }

        if event.keystroke.key.eq_ignore_ascii_case("escape") {
            self.cleanup_hotkey_recording = false;
            cx.notify();
            return;
        }

        let keystroke = &event.keystroke;
        let Some(chord) = win32::hotkey::HotkeyBinding::format_chord(
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.modifiers.shift,
            keystroke.modifiers.platform,
            &keystroke.key,
        ) else {
            return;
        };

        self.settings.cleanup_hotkey = chord;
        self.cleanup_hotkey_recording = false;
        win32::hotkey::sync(&self.settings);
        self.queue_settings_save(cx);
        cx.notify();
    }

    pub fn cancel_cleanup_hotkey_recording(&mut self, cx: &mut Context<Self>) {
        if self.cleanup_hotkey_recording {
            self.cleanup_hotkey_recording = false;
            cx.notify();
        }
    }

    pub fn set_debug_logging(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.debug_logging = enabled;
        crate::log::set_debug_enabled(enabled);
        if enabled {
            crate::log::write(&t!(
                "log.debug_enabled",
                path = crate::log::log_file_path().display().to_string()
            ));
        }
        self.queue_settings_save(cx);
        cx.notify();
    }
}
