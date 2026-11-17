//! 无窗口运行时：设置、清理、自动清理与指标采样。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rust_i18n::t;

use crate::auto_cleanup::{
    AUTO_CLEANUP_POLL_INTERVAL, AutoCleanupSource, threshold_cooldown_elapsed,
    threshold_trigger_due,
};
use crate::locale;
use crate::memory::{MemorySection, MemoryStatus, SystemWorkingSet};
use crate::messages::{build_cleanup_result_message, format_cleanup_effect};
use crate::optimize::{self, MemoryAreas};
use crate::settings::Settings;
use crate::system_metrics::{CpuNetworkSnapshot, SystemMetricsSampler};
use crate::tray::{self, TrayCommand};
use crate::win32;

const METRICS_INTERVAL: Duration = Duration::from_secs(1);

pub struct Runtime {
    settings: Settings,
    command_tx: Sender<TrayCommand>,
    metrics_sampler: SystemMetricsSampler,
    physical: MemorySection,
    virtual_mem: MemorySection,
    system_working_set: Option<SystemWorkingSet>,
    cpu_network: CpuNetworkSnapshot,
    is_optimizing: bool,
    last_auto_cleanup: Option<Instant>,
    low_memory_monitor: Option<win32::memory_notification::MemoryNotificationMonitor>,
    stop_background: Arc<AtomicBool>,
}

impl Runtime {
    pub fn start(settings: Settings, command_tx: Sender<TrayCommand>) -> Arc<Mutex<Self>> {
        let physical = MemorySection::unavailable(&t!("memory.physical"));
        let virtual_mem = MemorySection::unavailable(&t!("memory.virtual"));
        crate::log::set_debug_enabled(settings.debug_logging);

        let metrics_tx = command_tx.clone();
        let runtime = Arc::new(Mutex::new(Self {
            settings,
            command_tx,
            metrics_sampler: SystemMetricsSampler::new(),
            physical,
            virtual_mem,
            system_working_set: None,
            cpu_network: CpuNetworkSnapshot::unavailable(),
            is_optimizing: false,
            last_auto_cleanup: None,
            low_memory_monitor: None,
            stop_background: Arc::new(AtomicBool::new(false)),
        }));

        {
            let mut guard = lock(&runtime);
            guard.refresh_stats();
            guard.sync_hotkey();
            guard.sync_low_memory_monitor();
            guard.sync_tray();
        }

        spawn_metrics_loop(metrics_tx);
        spawn_threshold_loop(Arc::clone(&runtime));
        runtime
    }

    pub fn handle_command(runtime: &Arc<Mutex<Self>>, command: TrayCommand) {
        match command {
            TrayCommand::Quit => {
                lock(runtime).shutdown();
                tray::request_quit();
            }
            TrayCommand::SetSpinFrame {
                quarters,
                generation,
            } => tray::apply_spin_frame(quarters, generation),
            TrayCommand::OptimizeFinished => {
                lock(runtime).on_optimize_finished();
            }
            other => lock(runtime).dispatch(other),
        }
    }

    fn dispatch(&mut self, command: TrayCommand) {
        match command {
            TrayCommand::Optimize => self.run_optimize(),
            TrayCommand::RefreshTooltip => {
                self.refresh_stats();
                self.sync_tray_tooltip();
                self.sync_taskbar_window();
            }
            TrayCommand::LowMemory => {
                self.run_auto_cleanup(AutoCleanupSource::LowMemoryNotification);
            }
            TrayCommand::MenuAction(action) => self.handle_menu_action(&action),
            TrayCommand::Quit
            | TrayCommand::SetSpinFrame { .. }
            | TrayCommand::OptimizeFinished => {}
        }
    }

    fn handle_menu_action(&mut self, action: &str) {
        match action {
            "optimize" => self.run_optimize(),
            "quit" => {
                self.shutdown();
                tray::request_quit();
            }
            "run_at_startup" => {
                let next = !self.settings.run_at_startup;
                self.apply_run_at_startup(next);
            }
            "auto_cleanup" => {
                self.settings.auto_cleanup_enabled = !self.settings.auto_cleanup_enabled;
                self.persist_settings();
                self.sync_low_memory_monitor();
            }
            "notifications" => {
                self.settings.show_optimization_notifications =
                    !self.settings.show_optimization_notifications;
                self.persist_settings();
            }
            "debug_logging" => {
                self.settings.debug_logging = !self.settings.debug_logging;
                crate::log::set_debug_enabled(self.settings.debug_logging);
                self.persist_settings();
            }
            "cleanup_hotkey" => {
                self.settings.cleanup_hotkey_enabled = !self.settings.cleanup_hotkey_enabled;
                self.persist_settings();
                self.sync_hotkey();
            }
            "taskbar_window" => {
                self.settings.show_taskbar_window = !self.settings.show_taskbar_window;
                self.persist_settings();
                win32::taskbar_window::set_enabled(self.settings.show_taskbar_window);
                if self.settings.show_taskbar_window {
                    self.sync_taskbar_window();
                }
            }
            "open_config" => open_config_dir(),
            "lang_auto" => self.set_language("auto"),
            "lang_zh" => self.set_language("zh-CN"),
            "lang_en" => self.set_language("en"),
            "threshold_0" => self.set_threshold(0),
            "threshold_70" => self.set_threshold(70),
            "threshold_80" => self.set_threshold(80),
            "threshold_90" => self.set_threshold(90),
            "threshold_95" => self.set_threshold(95),
            other if other.starts_with("area_") => self.toggle_area(other),
            _ => {}
        }
        self.sync_tray();
    }

    fn toggle_area(&mut self, action: &str) {
        let Some(area) = area_from_action(action) else {
            return;
        };
        if self.is_optimizing {
            return;
        }
        let mut areas = self.settings.memory_areas();
        if areas.contains(area) {
            areas.remove(area);
        } else {
            match area {
                MemoryAreas::STANDBY_LIST => areas.remove(MemoryAreas::STANDBY_LIST_LOW_PRIORITY),
                MemoryAreas::STANDBY_LIST_LOW_PRIORITY => areas.remove(MemoryAreas::STANDBY_LIST),
                _ => {}
            }
            areas.insert(area);
        }
        self.settings.memory_areas = areas.bits();
        self.persist_settings();
    }

    fn set_threshold(&mut self, threshold: u32) {
        self.settings.auto_cleanup_threshold = threshold;
        if threshold > 0 {
            self.settings.auto_cleanup_enabled = true;
        }
        self.persist_settings();
        self.sync_low_memory_monitor();
    }

    fn set_language(&mut self, language: &str) {
        self.settings.language = language.into();
        locale::apply(&self.settings);
        self.refresh_stats();
        self.persist_settings();
    }

    fn apply_run_at_startup(&mut self, enabled: bool) {
        match win32::startup::set_enabled(enabled) {
            Ok(()) => {
                self.settings.run_at_startup = enabled;
                self.persist_settings();
            }
            Err(error) => crate::log_msg(&format!("[startup] update failed: {error:#}")),
        }
    }

    fn sync_hotkey(&self) {
        if let Err(error) = win32::hotkey::sync(&self.settings) {
            crate::log_msg(&format!("[hotkey] sync failed: {error:#}"));
        }
    }

    fn sync_low_memory_monitor(&mut self) {
        self.low_memory_monitor.take();
        if !self.settings.auto_cleanup_enabled {
            return;
        }
        match win32::memory_notification::MemoryNotificationMonitor::start(self.command_tx.clone())
        {
            Ok(monitor) => self.low_memory_monitor = Some(monitor),
            Err(error) => crate::log_msg(&format!(
                "[memory-notification] monitor start failed: {error:#}"
            )),
        }
    }

    fn persist_settings(&self) {
        self.settings.save();
    }

    fn shutdown(&mut self) {
        self.stop_background.store(true, Ordering::Relaxed);
        self.low_memory_monitor.take();
        win32::taskbar_window::shutdown();
        self.persist_settings();
    }

    fn refresh_stats(&mut self) {
        self.system_working_set = SystemWorkingSet::query().ok();
        match sections_from_status() {
            Ok((physical, virtual_mem)) => {
                self.physical = physical;
                self.virtual_mem = virtual_mem;
            }
            Err(_) => {
                self.physical = MemorySection::unavailable(&t!("memory.physical"));
                self.virtual_mem = MemorySection::unavailable(&t!("memory.virtual"));
            }
        }
        let _ = self.metrics_sampler.refresh();
        self.cpu_network = self.metrics_sampler.snapshot();
    }

    fn sync_tray_tooltip(&self) {
        tray::sync_tooltip(
            &self.physical,
            &self.virtual_mem,
            self.system_working_set.as_ref(),
            &self.cpu_network,
        );
    }

    fn sync_taskbar_window(&self) {
        if !self.settings.show_taskbar_window {
            return;
        }
        win32::taskbar_window::update(&self.physical, &self.cpu_network);
    }

    fn sync_tray(&self) {
        tray::sync_menu(&self.settings);
        self.sync_tray_tooltip();
        self.sync_taskbar_window();
    }

    fn run_auto_cleanup(&mut self, source: AutoCleanupSource) {
        if !self.settings.auto_cleanup_enabled
            || self.is_optimizing
            || self.settings.memory_areas().is_empty()
        {
            return;
        }
        crate::log::write(&format!("[auto-cleanup] trigger: {}", source.log_label()));
        self.last_auto_cleanup = Some(Instant::now());
        self.run_optimize();
    }

    fn run_optimize(&mut self) {
        if self.is_optimizing {
            return;
        }
        let areas = self.settings.memory_areas();
        let steps = match optimize::step_plan(areas) {
            Ok(steps) if !steps.is_empty() => steps,
            _ => {
                crate::log_msg(&t!("tooltip.select_areas"));
                return;
            }
        };

        let notify = self.settings.show_optimization_notifications;
        let command_tx = self.command_tx.clone();
        self.is_optimizing = true;
        tray::start_spin();

        let _ = thread::Builder::new()
            .name("optimize".into())
            .spawn(move || {
                if notify {
                    let _ = win32::notification::show(
                        &t!("notification.optimize_start_title"),
                        &t!("notification.optimize_start_body"),
                    );
                }

                let memory_before = MemoryStatus::query().ok();
                let mut completed = Vec::new();
                let mut errors = Vec::new();

                for step in steps {
                    let name = step.label;
                    match (step.run)() {
                        Ok(()) => {
                            completed.push(name.clone());
                            crate::log::write(&format!("[optimize] {name} succeeded"));
                        }
                        Err(error) => {
                            crate::log::write(&format!("[optimize] {name} failed: {error:#}"));
                            errors.push(name);
                        }
                    }
                }

                let memory_after = MemoryStatus::query().ok();
                let memory_sample_available = memory_before.is_some() && memory_after.is_some();
                let (avail_before, memory_load_before) = memory_before
                    .map(|s| (s.avail_phys, s.memory_load))
                    .unwrap_or_default();
                let (avail_after, memory_load_after) = memory_after
                    .map(|s| (s.avail_phys, s.memory_load))
                    .unwrap_or_default();

                let effect_detail = if memory_sample_available {
                    format_cleanup_effect(
                        avail_before,
                        avail_after,
                        memory_load_before,
                        memory_load_after,
                    )
                } else {
                    t!("memory.unavailable").to_string()
                };
                let completed_refs: Vec<&str> = completed.iter().map(String::as_str).collect();
                let errors_refs: Vec<&str> = errors.iter().map(String::as_str).collect();
                let status =
                    build_cleanup_result_message(&completed_refs, &errors_refs, &effect_detail);
                crate::log::write(&format!("[optimize] result: {status}"));

                if notify {
                    let _ = win32::notification::show(&t!("notification.optimize_title"), &status);
                }

                tray::stop_spin();
                let _ = command_tx.send(TrayCommand::OptimizeFinished);
            });
    }

    fn on_optimize_finished(&mut self) {
        self.is_optimizing = false;
        self.refresh_stats();
        self.sync_tray_tooltip();
        self.sync_taskbar_window();
    }

    fn poll_threshold(&mut self, above_threshold_ticks: &mut u32) {
        if !self.settings.auto_cleanup_enabled || self.is_optimizing {
            return;
        }
        let threshold = self.settings.auto_cleanup_threshold;
        if threshold == 0 {
            *above_threshold_ticks = 0;
            return;
        }
        let memory_load = MemoryStatus::query()
            .map(|status| status.memory_load)
            .unwrap_or(0);
        if memory_load >= threshold {
            *above_threshold_ticks = above_threshold_ticks.saturating_add(1);
        } else {
            *above_threshold_ticks = 0;
        }
        if threshold_trigger_due(
            *above_threshold_ticks,
            threshold_cooldown_elapsed(self.last_auto_cleanup.map(|t| t.elapsed())),
        ) {
            *above_threshold_ticks = 0;
            self.run_auto_cleanup(AutoCleanupSource::Threshold);
        }
    }
}

fn lock(runtime: &Arc<Mutex<Runtime>>) -> std::sync::MutexGuard<'_, Runtime> {
    runtime.lock().unwrap_or_else(|p| p.into_inner())
}

fn sections_from_status() -> anyhow::Result<(MemorySection, MemorySection)> {
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

fn area_from_action(action: &str) -> Option<MemoryAreas> {
    Some(match action {
        "area_working_set" => MemoryAreas::WORKING_SET,
        "area_system_file_cache" => MemoryAreas::SYSTEM_FILE_CACHE,
        "area_modified_page_list" => MemoryAreas::MODIFIED_PAGE_LIST,
        "area_standby_list" => MemoryAreas::STANDBY_LIST,
        "area_standby_list_low" => MemoryAreas::STANDBY_LIST_LOW_PRIORITY,
        "area_combined_page_list" => MemoryAreas::COMBINED_PAGE_LIST,
        "area_modified_file_cache" => MemoryAreas::MODIFIED_FILE_CACHE,
        _ => return None,
    })
}

fn open_config_dir() {
    let dir = Settings::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.to_string_lossy().into_owned();
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

fn spawn_metrics_loop(command_tx: Sender<TrayCommand>) {
    let _ = thread::Builder::new()
        .name("metrics".into())
        .spawn(move || {
            // 首包尽快：基线后短等再采一次真实差分。
            thread::sleep(Duration::from_millis(200));
            if command_tx.send(TrayCommand::RefreshTooltip).is_err() {
                return;
            }
            loop {
                thread::sleep(METRICS_INTERVAL);
                if command_tx.send(TrayCommand::RefreshTooltip).is_err() {
                    break;
                }
            }
        });
}

fn spawn_threshold_loop(runtime: Arc<Mutex<Runtime>>) {
    let stop = lock(&runtime).stop_background.clone();
    let _ = thread::Builder::new()
        .name("threshold-cleanup".into())
        .spawn(move || {
            let mut above_threshold_ticks = 0u32;
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(AUTO_CLEANUP_POLL_INTERVAL);
                if let Ok(mut app) = runtime.lock() {
                    app.poll_threshold(&mut above_threshold_ticks);
                }
            }
        });
}
