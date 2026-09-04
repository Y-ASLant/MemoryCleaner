use super::*;

pub fn window_size(expanded: bool) -> Size<Pixels> {
    size(px(WINDOW_WIDTH), px(window_height(expanded)))
}

/// Target window height for the given expanded state.
pub fn window_height(expanded: bool) -> f32 {
    if expanded {
        crate::ui::layout::expanded_window_height(CONTENT_PADDING)
    } else {
        crate::ui::layout::collapsed_window_height(CONTENT_PADDING)
    }
}
pub fn window_min_size() -> Size<Pixels> {
    size(
        px(WINDOW_MIN_WIDTH),
        px(crate::ui::layout::collapsed_window_height(CONTENT_PADDING)),
    )
}

pub fn window_options(expanded: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitleBar::title_bar_options()),
        window_bounds: Some(WindowBounds::centered(window_size(expanded), cx)),
        is_resizable: false,
        window_min_size: Some(window_min_size()),
        ..Default::default()
    }
}

pub struct AppEntityHolder(pub Entity<MemoryCleanerApp>);
impl Global for AppEntityHolder {}

pub fn open_main_window(
    cx: &mut AsyncApp,
    settings: Settings,
    command_tx: std::sync::mpsc::Sender<TrayCommand>,
    tray_rx: std::sync::mpsc::Receiver<TrayCommand>,
    launch_hidden: bool,
) -> Result<()> {
    let options = cx.update(|app| window_options(false, app));
    cx.open_window(options, |window, cx| {
        window.set_window_title(crate::version::APP_NAME);

        let app_entity = cx.new(|cx| {
            MemoryCleanerApp::new(window, cx, settings, command_tx, tray_rx, launch_hidden)
        });
        let _ = win32::window::remove_maximize_button(window);
        crate::ui::theme::init_light_theme(window, cx);

        let root = cx.new(|cx| Root::new(app_entity.clone(), window, cx));

        if launch_hidden {
            app_entity.update(cx, |app, _| {
                app.destroy_window_to_tray(window, "startup");
                app.sync_tray();
            });
        } else {
            window.activate_window();
        }

        root
    })?;
    Ok(())
}

impl MemoryCleanerApp {
    pub(super) fn attach_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        launch_hidden: bool,
    ) {
        let expansion = if self.settings_expanded { 1.0 } else { 0.0 };
        self.anim_settings_expand.snap_to(expansion);
        self.current_window_height = self.animated_window_height();
        self.window = Some(window.window_handle());
        self.window_shown = !launch_hidden;

        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, gpui_app| {
            crate::log_msg("[close] on_window_should_close");
            let should_close = weak
                .update(gpui_app, |this, _| {
                    this.request_close("should_close", window)
                })
                .unwrap_or(true);

            if should_close {
                gpui_app.quit();
            }
            should_close
        });

        if self.settings.always_on_top
            && let Err(error) = win32::window::set_always_on_top(window, true)
        {
            crate::log_msg(&format!(
                "[window] set_always_on_top(true) failed: {error:#}"
            ));
        }

        if !launch_hidden {
            self.start_memory_refresh(cx);
        }
    }

    fn pause_memory_refresh(&mut self) {
        self.memory_refresh_task.take();
    }

    fn start_memory_refresh(&mut self, cx: &mut Context<Self>) {
        self.pause_memory_refresh();
        if !self.window_visible() {
            return;
        }

        self.memory_refresh_task = Some(cx.spawn(async move |this, cx| {
            loop {
                Timer::after(MEMORY_REFRESH_INTERVAL).await;
                let Ok(()) = this.update(cx, |app, cx| {
                    if !memory_polling_enabled(app.window_visible()) {
                        return;
                    }
                    if app.refresh_memory() {
                        cx.notify();
                        app.sync_tray();
                    }
                }) else {
                    break;
                };
            }
        }));
    }

    fn open_window(&mut self, cx: &mut Context<Self>) {
        if self.window.is_some() || self.window_opening {
            return;
        }

        self.window_opening = true;
        cx.spawn(async move |this, cx| {
            let entity = match this.upgrade() {
                Some(entity) => entity,
                None => return,
            };

            // Read settings_expanded at window creation time, not at spawn time.
            // This prevents a stale height if settings change during the async gap.
            let expanded = entity.update(cx, |app, _| app.settings_expanded);
            let options = cx.update(|app| window_options(expanded, app));
            let opened = cx.open_window(options, |window, cx| {
                entity.update(cx, |app, cx| {
                    app.attach_window(window, cx, false);
                    app.window_opening = false;
                });
                window.set_window_title(crate::version::APP_NAME);
                let _ = win32::window::remove_maximize_button(window);
                crate::ui::theme::init_light_theme(window, cx);
                window.activate_window();
                cx.new(|cx| Root::new(entity.clone(), window, cx))
            });

            if opened.is_err() {
                entity.update(cx, |app, _| {
                    app.window_opening = false;
                    app.window_shown = false;
                });
            } else {
                entity.update(cx, |app, _| app.sync_tray());
            }
        })
        .detach();
    }
    pub(super) fn window_visible(&self) -> bool {
        self.window.is_some() && self.window_shown
    }

    pub(crate) fn sync_tray(&self) {
        crate::tray::sync_display(&self.physical, &self.virtual_mem, self.window_visible());
    }

    pub(crate) fn queue_settings_save(&mut self, cx: &mut Context<Self>) {
        self.settings_save_gen = self.settings_save_gen.wrapping_add(1);
        let generation = self.settings_save_gen;

        cx.spawn(async move |this, cx| {
            Timer::after(SETTINGS_SAVE_DEBOUNCE).await;
            let _ = this.update(cx, |app, _| {
                if app.settings_save_gen == generation {
                    app.settings.save();
                }
            });
        })
        .detach();
    }

    pub(super) fn sync_anim_targets_from_sections(&mut self) {
        self.anim_physical.set_target(self.physical.used_percent);
        self.anim_virtual.set_target(self.virtual_mem.used_percent);
        self.anim_used_phys.set_target(self.physical.used as f32);
        self.anim_avail_phys.set_target(self.physical.avail as f32);
        self.anim_used_virt.set_target(self.virtual_mem.used as f32);
        self.anim_avail_virt
            .set_target(self.virtual_mem.avail as f32);
        self.anim_dirty = true;
    }

    pub fn refresh_memory(&mut self) -> bool {
        let Ok((physical, virtual_mem)) = query_sections() else {
            if self.physical.is_unavailable() && self.virtual_mem.is_unavailable() {
                return false;
            }
            self.physical = MemorySection::unavailable(&t!("memory.physical"));
            self.virtual_mem = MemorySection::unavailable(&t!("memory.virtual"));
            self.sync_anim_targets_from_sections();
            return true;
        };

        let changed = self.physical != physical || self.virtual_mem != virtual_mem;
        if changed {
            self.physical = physical;
            self.virtual_mem = virtual_mem;
            self.sync_anim_targets_from_sections();
        }
        changed
    }

    pub fn animated_used_phys(&self) -> u64 {
        self.anim_used_phys.current as u64
    }
    pub fn animated_avail_phys(&self) -> u64 {
        self.anim_avail_phys.current as u64
    }
    pub fn animated_used_virt(&self) -> u64 {
        self.anim_used_virt.current as u64
    }
    pub fn animated_avail_virt(&self) -> u64 {
        self.anim_avail_virt.current as u64
    }
    pub fn animated_optimize_percent(&self) -> f32 {
        self.anim_optimize.current
    }
    pub fn settings_expand_progress(&self) -> f32 {
        self.anim_settings_expand.current.clamp(0.0, 1.0)
    }
    pub fn settings_panel_visible(&self) -> bool {
        self.settings_expanded || self.settings_expand_progress() > SETTINGS_PANEL_VISIBLE_EPSILON
    }
    fn animated_window_height(&self) -> f32 {
        window_height(false) + settings_reveal_height() * self.settings_expand_progress()
    }
    fn resize_window_height(&mut self, window: &mut Window, height: f32) {
        let rounded = height.round();
        if (self.current_window_height - rounded).abs() >= 0.5 {
            window.resize(size(px(WINDOW_WIDTH), px(rounded)));
            self.current_window_height = rounded;
        }
    }

    /// Set optimize progress and kick the animation loop.
    pub(super) fn set_optimize_percent(&mut self, value: f32) {
        self.anim_optimize.target = value;
        self.anim_dirty = true;
    }

    pub fn activate_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.window {
            match handle.update(cx, |_, window, _| -> Result<()> {
                crate::log_msg("[window] activate_window");
                win32::window::show_from_tray(window)?;
                window.activate_window();
                Ok(())
            }) {
                Ok(Ok(())) => {
                    self.window_shown = true;
                    self.pause_memory_refresh();
                    self.start_memory_refresh(cx);
                    self.sync_tray();
                    return;
                }
                Ok(Err(e)) => crate::log_msg(&format!("[window] show_from_tray failed: {e:#}")),
                Err(_) => crate::log_msg("[window] activate_window handle update failed"),
            }
            self.release_window_handle(cx, "activate_failed");
        }
        self.open_window(cx);
    }

    /// Destroy the GPUI window referenced by `self.window`, then clear tracking state.
    /// Safe to call when no handle is held (still resets `window_shown` and pauses loops).
    fn release_window_handle(&mut self, cx: &mut Context<Self>, source: &str) {
        if let Some(handle) = self.window.take() {
            match handle.update(cx, |_, window, _| window.remove_window()) {
                Ok(()) => crate::log_msg(&format!("[window] release_window ok source={source}")),
                Err(_) => {
                    crate::log_msg(&format!("[window] release_window failed source={source}"))
                }
            }
        } else {
            crate::log_msg(&format!(
                "[window] release_window no handle source={source}"
            ));
        }
        self.window_shown = false;
        self.pause_memory_refresh();
    }

    /// Remove the GPUI window and drop our handle. `activate_window` recreates it via
    /// `open_window()`.
    fn destroy_window_to_tray(&mut self, window: &mut Window, source: &str) {
        window.remove_window();
        self.window = None;
        self.window_shown = false;
        self.pause_memory_refresh();
        crate::log_msg(&format!("[close] hide_to_tray destroy ok source={source}"));
    }

    /// Handle a close request. Returns `true` when the app should quit entirely.
    pub fn request_close(&mut self, source: &str, window: &mut Window) -> bool {
        crate::log_msg(&format!(
            "[close] request_close source={source} close_to_tray={}",
            self.settings.close_to_notification_area
        ));
        self.settings.save();
        if self.settings.close_to_notification_area {
            self.destroy_window_to_tray(window, source);
            self.sync_tray();
            false
        } else {
            true
        }
    }

    pub fn hide_to_tray(&mut self, cx: &mut Context<Self>) {
        self.release_window_handle(cx, "tray_menu");
        self.sync_tray();
    }

    pub fn toggle_settings_expanded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_expanded = !self.settings_expanded;
        let target = if self.settings_expanded { 1.0 } else { 0.0 };
        self.anim_settings_expand.set_target(target);
        self.anim_dirty = true;
        self.resize_window_height(window, self.animated_window_height());
        cx.notify();
    }

    /// Tick all active animations each frame (render-driven, vsync-paced).
    /// Returns `true` if any animation is still running (caller schedules next frame).
    pub(super) fn tick_animations(&mut self, window: &mut Window) -> bool {
        // Sampled memory values move linearly until the next poll. Event and
        // layout animations retain their own easing and completion behavior.
        if self.anim_dirty {
            let now = Instant::now();
            let dt = self
                .last_anim_tick
                .map(|t| now.saturating_duration_since(t).as_secs_f32())
                .unwrap_or(1.0 / 60.0);
            let still = self.anim_physical.tick_dt(dt)
                | self.anim_virtual.tick_dt(dt)
                | self.anim_optimize.tick_dt(dt)
                | self.anim_used_phys.tick_dt(dt)
                | self.anim_avail_phys.tick_dt(dt)
                | self.anim_used_virt.tick_dt(dt)
                | self.anim_avail_virt.tick_dt(dt)
                | self.anim_settings_expand.tick_dt(dt);
            self.anim_dirty = still;
            self.last_anim_tick = still.then_some(now);
        } else {
            self.last_anim_tick = None;
        }

        // When settled, expand progress is exactly 0/1, so this equals the
        // collapsed/expanded height; while animating it tracks the live progress.
        self.resize_window_height(window, self.animated_window_height());

        self.anim_dirty
    }
}
