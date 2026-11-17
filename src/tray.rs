use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use image::RgbaImage;
use rust_i18n::t;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::memory::{MemorySection, SystemWorkingSet};
use crate::optimize::MemoryAreas;
use crate::settings::Settings;
use crate::system_metrics::CpuNetworkSnapshot;

const TRAY_ICON_SIZE: u32 = 32;
const SPIN_STEP_MS: u64 = 96;

// TrayIcon / muda 非 Send：仅主线程访问。
thread_local! {
    static TRAY: RefCell<Option<TrayState>> = const { RefCell::new(None) };
}

static CMD_TX: OnceLock<Sender<TrayCommand>> = OnceLock::new();
static SPIN_GENERATION: AtomicU32 = AtomicU32::new(0);
static SPIN_ACTIVE: AtomicBool = AtomicBool::new(false);
static MAIN_THREAD_ID: AtomicU32 = AtomicU32::new(0);

const AREA_SPECS: &[(&str, MemoryAreas)] = &[
    ("area_working_set", MemoryAreas::WORKING_SET),
    ("area_system_file_cache", MemoryAreas::SYSTEM_FILE_CACHE),
    ("area_modified_page_list", MemoryAreas::MODIFIED_PAGE_LIST),
    ("area_standby_list", MemoryAreas::STANDBY_LIST),
    (
        "area_standby_list_low",
        MemoryAreas::STANDBY_LIST_LOW_PRIORITY,
    ),
    ("area_combined_page_list", MemoryAreas::COMBINED_PAGE_LIST),
    ("area_modified_file_cache", MemoryAreas::MODIFIED_FILE_CACHE),
];

const THRESHOLD_SPECS: &[(u32, &str)] = &[
    (0, "threshold_0"),
    (70, "threshold_70"),
    (80, "threshold_80"),
    (90, "threshold_90"),
    (95, "threshold_95"),
];

#[derive(Debug)]
pub enum TrayCommand {
    RefreshTooltip,
    Optimize,
    LowMemory,
    MenuAction(String),
    SetSpinFrame { quarters: u32, generation: u32 },
    OptimizeFinished,
    Quit,
}

struct TrayState {
    icon: TrayIcon,
    tooltip: Option<String>,
    icon_frames: [tray_icon::Icon; 4],
    items: MenuItems,
}

struct MenuItems {
    optimize: MenuItem,
    quit: MenuItem,
    run_at_startup: CheckMenuItem,
    auto_cleanup: CheckMenuItem,
    notifications: CheckMenuItem,
    debug_logging: CheckMenuItem,
    cleanup_hotkey: CheckMenuItem,
    taskbar_window: CheckMenuItem,
    open_config: MenuItem,
    lang_auto: CheckMenuItem,
    lang_zh: CheckMenuItem,
    lang_en: CheckMenuItem,
    thresholds: [CheckMenuItem; 5],
    areas: [CheckMenuItem; 7],
    sub_areas: Submenu,
    sub_auto: Submenu,
    sub_threshold: Submenu,
    sub_settings: Submenu,
    sub_language: Submenu,
}

fn with_tray_mut(f: impl FnOnce(&mut TrayState)) {
    TRAY.with(|cell| {
        if let Some(tray) = cell.borrow_mut().as_mut() {
            f(tray);
        }
    });
}

pub fn install(command_tx: Sender<TrayCommand>, settings: &Settings) -> anyhow::Result<()> {
    let _ = CMD_TX.set(command_tx.clone());
    MAIN_THREAD_ID.store(
        unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
        Ordering::Relaxed,
    );

    let source = load_icon_source();
    let frames_rgba = build_icon_frames(&source);
    let icon_frames = [
        rgba_to_icon(&frames_rgba[0])?,
        rgba_to_icon(&frames_rgba[1])?,
        rgba_to_icon(&frames_rgba[2])?,
        rgba_to_icon(&frames_rgba[3])?,
    ];
    let items = build_menu_items(settings);
    let menu = build_root_menu(&items);

    let icon = TrayIconBuilder::new()
        .with_tooltip(crate::version::APP_NAME)
        .with_icon(icon_frames[0].clone())
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?;

    install_event_handlers(command_tx);

    TRAY.with(|cell| {
        *cell.borrow_mut() = Some(TrayState {
            icon,
            tooltip: None,
            icon_frames,
            items,
        });
    });
    sync_menu(settings);
    Ok(())
}

fn check_item(id: &str, text: impl AsRef<str>, checked: bool) -> CheckMenuItem {
    CheckMenuItem::with_id(id, text, true, checked, None)
}

fn build_menu_items(settings: &Settings) -> MenuItems {
    let areas_bits = settings.memory_areas();
    let areas = std::array::from_fn(|i| {
        let (id, area) = AREA_SPECS[i];
        check_item(id, t!(area.label_key()), areas_bits.contains(area))
    });

    let threshold = settings.auto_cleanup_threshold;
    let thresholds = std::array::from_fn(|i| {
        let (value, id) = THRESHOLD_SPECS[i];
        let label = if value == 0 {
            t!("tray.threshold_off").to_string()
        } else {
            format!("{value}%")
        };
        check_item(id, label, threshold == value)
    });

    MenuItems {
        optimize: MenuItem::with_id("optimize", t!("tray.optimize"), true, None),
        quit: MenuItem::with_id("quit", t!("tray.quit"), true, None),
        run_at_startup: check_item(
            "run_at_startup",
            t!("settings.run_at_startup"),
            settings.run_at_startup,
        ),
        auto_cleanup: check_item(
            "auto_cleanup",
            t!("settings.auto_cleanup"),
            settings.auto_cleanup_enabled,
        ),
        notifications: check_item(
            "notifications",
            t!("tray.notifications"),
            settings.show_optimization_notifications,
        ),
        debug_logging: check_item(
            "debug_logging",
            t!("settings.debug_logging"),
            settings.debug_logging,
        ),
        cleanup_hotkey: check_item(
            "cleanup_hotkey",
            t!("settings.cleanup_hotkey"),
            settings.cleanup_hotkey_enabled,
        ),
        taskbar_window: check_item(
            "taskbar_window",
            t!("settings.taskbar_window"),
            settings.show_taskbar_window,
        ),
        open_config: MenuItem::with_id("open_config", t!("tray.open_config"), true, None),
        lang_auto: check_item(
            "lang_auto",
            t!("settings.language_auto"),
            settings.language == "auto",
        ),
        lang_zh: check_item(
            "lang_zh",
            t!("settings.language_zh"),
            settings.language == "zh-CN",
        ),
        lang_en: check_item(
            "lang_en",
            t!("settings.language_en"),
            settings.language == "en",
        ),
        thresholds,
        areas,
        sub_areas: Submenu::with_id("sub_areas", t!("tray.menu_areas"), true),
        sub_auto: Submenu::with_id("sub_auto", t!("tray.menu_auto_cleanup"), true),
        sub_threshold: Submenu::with_id("sub_threshold", t!("tray.menu_threshold"), true),
        sub_settings: Submenu::with_id("sub_settings", t!("tray.menu_settings"), true),
        sub_language: Submenu::with_id("sub_language", t!("settings.language"), true),
    }
}

fn build_root_menu(items: &MenuItems) -> Menu {
    let menu = Menu::new();
    let _ = menu.append(&items.optimize);
    let _ = menu.append(&PredefinedMenuItem::separator());

    for area in &items.areas {
        let _ = items.sub_areas.append(area);
    }
    let _ = menu.append(&items.sub_areas);

    for th in &items.thresholds {
        let _ = items.sub_threshold.append(th);
    }
    let _ = items.sub_auto.append(&items.auto_cleanup);
    let _ = items.sub_auto.append(&items.sub_threshold);
    let _ = menu.append(&items.sub_auto);

    let _ = items.sub_language.append(&items.lang_auto);
    let _ = items.sub_language.append(&items.lang_zh);
    let _ = items.sub_language.append(&items.lang_en);
    let _ = items.sub_settings.append(&items.run_at_startup);
    let _ = items.sub_settings.append(&items.notifications);
    let _ = items.sub_settings.append(&items.debug_logging);
    let _ = items.sub_settings.append(&items.cleanup_hotkey);
    let _ = items.sub_settings.append(&items.taskbar_window);
    let _ = items.sub_settings.append(&items.sub_language);
    let _ = items.sub_settings.append(&items.open_config);
    let _ = menu.append(&items.sub_settings);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&items.quit);
    menu
}

pub fn sync_menu(settings: &Settings) {
    with_tray_mut(|tray| {
        let items = &tray.items;
        items.optimize.set_text(t!("tray.optimize"));
        items.quit.set_text(t!("tray.quit"));
        items.sub_areas.set_text(t!("tray.menu_areas"));
        items.sub_auto.set_text(t!("tray.menu_auto_cleanup"));
        items.sub_threshold.set_text(t!("tray.menu_threshold"));
        items.sub_settings.set_text(t!("tray.menu_settings"));
        items.sub_language.set_text(t!("settings.language"));
        items.open_config.set_text(t!("tray.open_config"));

        items.run_at_startup.set_text(t!("settings.run_at_startup"));
        items.run_at_startup.set_checked(settings.run_at_startup);

        items.auto_cleanup.set_text(t!("settings.auto_cleanup"));
        items
            .auto_cleanup
            .set_checked(settings.auto_cleanup_enabled);

        items.notifications.set_text(t!("tray.notifications"));
        items
            .notifications
            .set_checked(settings.show_optimization_notifications);

        items.debug_logging.set_text(t!("settings.debug_logging"));
        items.debug_logging.set_checked(settings.debug_logging);

        items.cleanup_hotkey.set_text(t!("settings.cleanup_hotkey"));
        items
            .cleanup_hotkey
            .set_checked(settings.cleanup_hotkey_enabled);

        items.taskbar_window.set_text(t!("settings.taskbar_window"));
        items
            .taskbar_window
            .set_checked(settings.show_taskbar_window);

        items.lang_auto.set_text(t!("settings.language_auto"));
        items.lang_zh.set_text(t!("settings.language_zh"));
        items.lang_en.set_text(t!("settings.language_en"));
        items.lang_auto.set_checked(settings.language == "auto");
        items.lang_zh.set_checked(settings.language == "zh-CN");
        items.lang_en.set_checked(settings.language == "en");

        let th = settings.auto_cleanup_threshold;
        for (i, item) in items.thresholds.iter().enumerate() {
            let (value, _) = THRESHOLD_SPECS[i];
            if value == 0 {
                item.set_text(t!("tray.threshold_off"));
            } else {
                item.set_text(format!("{value}%"));
            }
            item.set_checked(th == value);
        }

        let areas = settings.memory_areas();
        for (i, item) in items.areas.iter().enumerate() {
            let (_, area) = AREA_SPECS[i];
            item.set_text(t!(area.label_key()));
            item.set_checked(areas.contains(area));
        }
    });
}

pub fn sync_tooltip(
    physical: &MemorySection,
    virtual_mem: &MemorySection,
    system_working_set: Option<&SystemWorkingSet>,
    cpu_network: &CpuNetworkSnapshot,
) {
    let tooltip = format_memory_tooltip(physical, virtual_mem, system_working_set, cpu_network);
    with_tray_mut(|tray| {
        if tray.tooltip.as_ref() != Some(&tooltip) && tray.icon.set_tooltip(Some(&tooltip)).is_ok()
        {
            tray.tooltip = Some(tooltip);
        }
    });
}

fn load_icon_source() -> RgbaImage {
    const PNG: &[u8] = include_bytes!("../App.png");
    image::load_from_memory(PNG).expect("App.png").to_rgba8()
}

fn build_icon_frames(source: &RgbaImage) -> [RgbaImage; 4] {
    let mut base = image::imageops::resize(
        source,
        TRAY_ICON_SIZE,
        TRAY_ICON_SIZE,
        image::imageops::FilterType::CatmullRom,
    );
    // tray-icon 用 CreateIcon 的 1-bit AND mask：半透明边会变成黑角。
    harden_icon_alpha(&mut base);
    let mut r90 = image::imageops::rotate90(&base);
    let mut r180 = image::imageops::rotate180(&base);
    let mut r270 = image::imageops::rotate270(&base);
    harden_icon_alpha(&mut r90);
    harden_icon_alpha(&mut r180);
    harden_icon_alpha(&mut r270);
    [base, r90, r180, r270]
}

/// 透明/不透明二值化，去掉抗锯齿半透明像素。
fn harden_icon_alpha(img: &mut RgbaImage) {
    for pixel in img.pixels_mut() {
        if pixel.0[3] < 128 {
            pixel.0 = [0, 0, 0, 0];
        } else {
            pixel.0[3] = 255;
        }
    }
}

fn rgba_to_icon(frame: &RgbaImage) -> anyhow::Result<tray_icon::Icon> {
    let (w, h) = frame.dimensions();
    Ok(tray_icon::Icon::from_rgba(frame.as_raw().to_vec(), w, h)?)
}

pub fn apply_spin_frame(quarters: u32, generation: u32) {
    if SPIN_GENERATION.load(Ordering::Relaxed) != generation {
        return;
    }
    with_tray_mut(|tray| {
        let frame = &tray.icon_frames[(quarters % 4) as usize];
        let _ = tray.icon.set_icon(Some(frame.clone()));
    });
}

pub fn stop_spin() {
    let generation = SPIN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    SPIN_ACTIVE.store(false, Ordering::Relaxed);
    if let Some(tx) = CMD_TX.get() {
        let _ = tx.send(TrayCommand::SetSpinFrame {
            quarters: 0,
            generation,
        });
    }
}

pub fn start_spin() {
    if SPIN_ACTIVE.swap(true, Ordering::Relaxed) {
        return;
    }
    let generation = SPIN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    if thread::Builder::new()
        .name("tray-icon-spin".into())
        .spawn(move || {
            let mut quarters = 0u32;
            loop {
                if SPIN_GENERATION.load(Ordering::Relaxed) != generation {
                    break;
                }
                if let Some(tx) = CMD_TX.get() {
                    let _ = tx.send(TrayCommand::SetSpinFrame {
                        quarters,
                        generation,
                    });
                }
                quarters = quarters.wrapping_add(1);
                thread::sleep(Duration::from_millis(SPIN_STEP_MS));
            }
        })
        .is_err()
    {
        SPIN_ACTIVE.store(false, Ordering::Relaxed);
    }
}

pub fn format_memory_tooltip(
    physical: &MemorySection,
    virtual_mem: &MemorySection,
    system_working_set: Option<&SystemWorkingSet>,
    cpu_network: &CpuNetworkSnapshot,
) -> String {
    let mut lines = vec![t!("tray.tooltip", percent = physical.percent_label()).to_string()];
    lines.push(
        t!(
            "tray.tooltip_virtual",
            percent = virtual_mem.percent_label()
        )
        .to_string(),
    );
    lines.push(
        t!(
            "tray.tooltip_system_working_set",
            current = system_working_set
                .map(|value| crate::memory::MemoryStatus::format_bytes(value.current))
                .unwrap_or_else(|| "—".into()),
        )
        .to_string(),
    );
    lines.push(t!("tray.tooltip_cpu", percent = cpu_network.cpu_label()).to_string());
    lines.push(
        t!(
            "tray.tooltip_network",
            down = cpu_network.download_label(),
            up = cpu_network.upload_label()
        )
        .to_string(),
    );
    lines.join("\n")
}

fn install_event_handlers(tx: Sender<TrayCommand>) {
    TrayIconEvent::set_event_handler(Some({
        let tx = tx.clone();
        move |event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let _ = tx.send(TrayCommand::Optimize);
            }
            TrayIconEvent::Enter { .. } => {
                let _ = tx.send(TrayCommand::RefreshTooltip);
            }
            _ => {}
        }
    }));

    MenuEvent::set_event_handler(Some({
        move |event: MenuEvent| {
            let _ = tx.send(TrayCommand::MenuAction(event.id().0.clone()));
        }
    }));
}

pub fn request_quit() {
    let tid = MAIN_THREAD_ID.load(Ordering::Relaxed);
    if tid == 0 {
        return;
    }
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
            tid,
            windows::Win32::UI::WindowsAndMessaging::WM_QUIT,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        );
    }
}

/// 主线程：泵消息并处理命令（托盘 API 不可跨线程）。
pub fn run_loop<F>(command_rx: std::sync::mpsc::Receiver<TrayCommand>, mut on_command: F)
where
    F: FnMut(TrayCommand),
{
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
    };

    loop {
        // 菜单模态期间 metrics 会堆积；合并 RefreshTooltip，松手后只刷一次。
        let mut refresh = false;
        while let Ok(command) = command_rx.try_recv() {
            match command {
                TrayCommand::RefreshTooltip => refresh = true,
                TrayCommand::Quit => {
                    on_command(command);
                    return;
                }
                other => on_command(other),
            }
        }
        if refresh {
            on_command(TrayCommand::RefreshTooltip);
        }

        let mut had_message = false;
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
                had_message = true;
                if msg.message == WM_QUIT {
                    return;
                }
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }

        if !had_message {
            thread::sleep(Duration::from_millis(16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::with_locale;

    #[test]
    fn tooltip_lists_memory_cpu_and_network() {
        with_locale("en", || {
            let physical = MemorySection {
                title: "Physical".into(),
                total: 8 << 30,
                used: 4 << 30,
                avail: 4 << 30,
                used_percent: 50.0,
            };
            let virtual_mem = MemorySection {
                title: "Commit".into(),
                total: 16 << 30,
                used: 8 << 30,
                avail: 8 << 30,
                used_percent: 50.0,
            };
            let sws = SystemWorkingSet {
                current: 1 << 20,
                peak: 2 << 20,
            };
            let cpu = CpuNetworkSnapshot {
                cpu_percent: Some(12.0),
                download_bps: Some(1024.0),
                upload_bps: Some(512.0),
            };
            let tip = format_memory_tooltip(&physical, &virtual_mem, Some(&sws), &cpu);
            assert!(tip.contains("50%"));
            assert!(tip.contains("12%"));
        });
    }
}
