//! 任务栏信息窗口（TrafficMonitor 同类）。
//!
//! 优先 `SetParent` 嵌进 `Shell_TrayWnd`；失败再 `TOPMOST`。
//! 绘制：GDI 光栅化到位图，`UpdateLayeredWindow` 原子提交整帧——和 TrafficMonitor 默认走的路径一样。
//! 不用 DirectComposition 合成交换链：那条路径要等 `Present` 翻转，被任务栏重绘打断时会露出半帧。
//! 仅主线程调用。

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, AddFontMemResourceEx, BI_RGB, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC, DeleteObject, FW_MEDIUM,
    GetDC, GetDeviceCaps, GetTextExtentPoint32W, GetTextMetricsW, HDC, HFONT, HGDIOBJ, LOGPIXELSY,
    OUT_DEFAULT_PRECIS, ReleaseDC, ScreenToClient, SelectObject, SetBkMode, SetTextColor,
    TEXTMETRICW, TRANSPARENT, TextOutW,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::System::Variant::{
    VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BSTR, VariantClear,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, TreeScope_Descendants,
    UIA_AutomationIdPropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GWL_EXSTYLE,
    GetParent, GetWindowLongPtrW, GetWindowRect, HMENU, HWND_TOPMOST, IDC_ARROW, LoadCursorW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_DEFERERASE, SWP_NOACTIVATE, SWP_NOCOPYBITS,
    SWP_NOREDRAW, SWP_NOSENDCHANGING, SWP_NOZORDER, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BSTR, PCWSTR, w};

use crate::memory::MemorySection;
use crate::system_metrics::CpuNetworkSnapshot;

use crate::win32::wide_null;

const CLASS_NAME: PCWSTR = w!("MemoryCleanerLite.TaskbarWindow");
const PAD_X: i32 = 4;
const PAD_Y: i32 = 2;
const COL_GAP: i32 = 10;
const LINE_GAP: i32 = 0;
const GAP_FROM_NOTIFY: i32 = 2;
/// 几何抖动阈值：过大时会盖住通知区/天气板边缘。
const GEOMETRY_SLACK_PX: i32 = 2;
/// UIA 结果 TTL；开关天气靠 TaskbarDa 即时失效，不靠这条。
const WIDGETS_UIA_TTL: Duration = Duration::from_millis(500);
/// TaskbarDa 开但 UIA 失败时，右侧天气板预留（96dpi 逻辑像素）。
const WIDGETS_FALLBACK_WIDTH_96: i32 = 180;
const THEME_RECHECK: Duration = Duration::from_secs(5);
const FONT_TTF: &[u8] = include_bytes!("../fonts/Lilex-Regular.ttf");
const FONT_FACE: &str = "Lilex";
const FONT_POINT_SIZE: i32 = 13;
/// 列宽上限样例，避免网速/百分比位数变化改窗宽。
const SLOT_UP: &str = "↑: 999.99 MB/s";
const SLOT_DOWN: &str = "↓: 999.99 MB/s";
const SLOT_CPU: &str = "CPU: 100%";
const SLOT_RAM: &str = "RAM: 100%";

thread_local! {
    static STATE: RefCell<Option<TaskbarWindowState>> = const { RefCell::new(None) };
    static WIDGETS_CACHE: RefCell<WidgetsBoardCache> = const {
        RefCell::new(WidgetsBoardCache {
            checked_at: None,
            notify_left: i32::MIN,
            widgets_shown: false,
            board_left: None,
        })
    };
}

struct WidgetsBoardCache {
    checked_at: Option<Instant>,
    /// 缓存键：通知区左缘 + TaskbarDa。
    notify_left: i32,
    widgets_shown: bool,
    board_left: Option<i32>,
}

static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static FONT_LOADED: OnceLock<bool> = OnceLock::new();

struct TaskbarWindowState {
    hwnd: HWND,
    enabled: bool,
    /// 已 SetParent 到任务栏（TM 路径）。
    embedded: bool,
    up: String,
    down: String,
    cpu: String,
    ram: String,
    width: i32,
    height: i32,
    pos_x: i32,
    pos_y: i32,
    font: HFONT,
    dpi: i32,
    /// `true` = 白字（深色栏）；`false` = 黑字（浅色栏）
    light_text: bool,
    /// 主题缓存时刻，避免每秒读主题抖动导致反色闪。
    theme_checked_at: Instant,
}

fn cell_up(up: &str) -> String {
    format!("↑: {up}")
}
fn cell_down(down: &str) -> String {
    format!("↓: {down}")
}
fn cell_cpu(cpu: &str) -> String {
    format!("CPU: {cpu}")
}
fn cell_ram(ram: &str) -> String {
    format!("RAM: {ram}")
}

pub fn install() -> anyhow::Result<()> {
    ensure_class()?;
    ensure_font_loaded();

    let dpi = taskbar_dpi();
    let font = create_font(dpi)?;
    let up = cell_up("—");
    let down = cell_down("—");
    let cpu = cell_cpu("—");
    let ram = cell_ram("—");
    let (width, height) = measure_grid(font, &up, &down, &cpu, &ram)?;

    let module = unsafe { GetModuleHandleW(None)? };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            CLASS_NAME,
            PCWSTR::null(),
            WS_POPUP | WS_CLIPSIBLINGS,
            0,
            0,
            width,
            height,
            None,
            None::<HMENU>,
            Some(module.into()),
            None,
        )
    }?;

    let embedded = find_taskbar().is_some_and(|taskbar| try_embed(hwnd, taskbar));
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(TaskbarWindowState {
            hwnd,
            enabled: true,
            embedded,
            up,
            down,
            cpu,
            ram,
            width,
            height,
            pos_x: i32::MIN,
            pos_y: i32::MIN,
            font,
            dpi,
            light_text: !system_uses_light_theme().unwrap_or(false),
            theme_checked_at: Instant::now(),
        });
    });

    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            unsafe {
                let _ = ShowWindow(state.hwnd, SW_SHOWNOACTIVATE);
            }
            relocate_and_paint(state, true);
        }
    });

    crate::log_msg(&format!(
        "[taskbar-window] installed ({})",
        if embedded {
            "SetParent embed"
        } else {
            "TOPMOST fallback"
        }
    ));
    Ok(())
}

pub fn set_enabled(enabled: bool) {
    STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return;
        };
        state.enabled = enabled;
        unsafe {
            let _ = ShowWindow(
                state.hwnd,
                if enabled { SW_SHOWNOACTIVATE } else { SW_HIDE },
            );
        }
        if enabled {
            relocate_and_paint(state, true);
        }
    });
}

pub fn update(physical: &MemorySection, cpu_network: &CpuNetworkSnapshot) {
    STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return;
        };
        if !state.enabled {
            return;
        }

        let dpi = taskbar_dpi();
        let mut dpi_changed = false;
        if dpi != state.dpi
            && dpi > 0
            && let Ok(font) = create_font(dpi)
        {
            unsafe {
                let _ = DeleteObject(state.font.into());
            }
            state.font = font;
            state.dpi = dpi;
            dpi_changed = true;
        }

        let up = cell_up(&cpu_network.upload_label());
        let down = cell_down(&cpu_network.download_label());
        let cpu = cell_cpu(&cpu_network.cpu_label());
        let ram = cell_ram(&physical.percent_label());

        let mut theme_changed = false;
        if state.theme_checked_at.elapsed() >= THEME_RECHECK {
            state.theme_checked_at = Instant::now();
            // 读不到主题注册表时保持当前字色。
            if let Some(light) = system_uses_light_theme() {
                let light_text = !light;
                if state.light_text != light_text {
                    state.light_text = light_text;
                    theme_changed = true;
                }
            }
        }

        let text_changed =
            state.up != up || state.down != down || state.cpu != cpu || state.ram != ram;
        if text_changed {
            state.up = up;
            state.down = down;
            state.cpu = cpu;
            state.ram = ram;
            if let Ok((w, h)) =
                measure_grid(state.font, &state.up, &state.down, &state.cpu, &state.ram)
            {
                // 1px 测量抖动会 SetWindowPos，偶发闪一下。
                if state.pos_x == i32::MIN
                    || (state.width - w).abs() > 2
                    || (state.height - h).abs() > 2
                {
                    state.width = w;
                    state.height = h;
                }
            }
        }

        relocate_and_paint(state, text_changed || theme_changed || dpi_changed);
    });
}

pub fn shutdown() {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().take() {
            unsafe {
                let _ = DestroyWindow(state.hwnd);
                let _ = DeleteObject(state.font.into());
            }
        }
    });
}

/// 几何不变且内容未变 → 什么都不做。
fn relocate_and_paint(state: &mut TaskbarWindowState, content_changed: bool) {
    ensure_embedded(state);

    let Some((screen_x, screen_y, width, height)) =
        compute_screen_placement(state.width, state.height)
    else {
        return;
    };

    let dx = state.pos_x - screen_x;
    let dy = state.pos_y - screen_y;
    let dw = state.width - width;
    let dh = state.height - height;
    let geometry_changed = dx.abs() > GEOMETRY_SLACK_PX
        || dy.abs() > GEOMETRY_SLACK_PX
        || dw.abs() > 2
        || dh.abs() > 2;

    if !geometry_changed && !content_changed {
        return;
    }

    if geometry_changed {
        let (x, y, insert_after, flags) = if state.embedded {
            let mut pt = POINT {
                x: screen_x,
                y: screen_y,
            };
            if let Some(taskbar) = find_taskbar() {
                unsafe {
                    let _ = ScreenToClient(taskbar, &mut pt);
                }
            }
            (
                pt.x,
                pt.y,
                None,
                SWP_NOACTIVATE
                    | SWP_NOZORDER
                    | SWP_NOREDRAW
                    | SWP_NOCOPYBITS
                    | SWP_DEFERERASE
                    | SWP_NOSENDCHANGING,
            )
        } else {
            (
                screen_x,
                screen_y,
                Some(HWND_TOPMOST),
                SWP_NOACTIVATE
                    | SWP_NOREDRAW
                    | SWP_NOCOPYBITS
                    | SWP_DEFERERASE
                    | SWP_NOSENDCHANGING,
            )
        };

        let _ = unsafe { SetWindowPos(state.hwnd, insert_after, x, y, width, height, flags) };
        state.pos_x = screen_x;
        state.pos_y = screen_y;
        state.width = width;
        state.height = height;
    }

    present_layered(state);
}

fn ensure_embedded(state: &mut TaskbarWindowState) {
    let Some(taskbar) = find_taskbar() else {
        state.embedded = false;
        return;
    };

    // WS_POPUP + SetParent 后 IsChild 常年 false；误用会每秒 re-SetParent → 闪。
    // 以 GetParent == taskbar 为准。GetParent 失败不等于丢嵌，不在热路径重嵌。
    match unsafe { GetParent(state.hwnd).ok() } {
        Some(parent) if parent == taskbar => {
            state.embedded = true;
        }
        Some(_) if state.embedded => {
            if try_embed(state.hwnd, taskbar) {
                state.embedded = true;
                crate::log_msg("[taskbar-window] SetParent re-embed ok");
            } else {
                state.embedded = false;
                apply_topmost_exstyle(state.hwnd, true);
            }
        }
        _ => {}
    }
}

fn try_embed(hwnd: HWND, taskbar: HWND) -> bool {
    unsafe {
        match SetParent(hwnd, Some(taskbar)) {
            Ok(_) => {
                apply_topmost_exstyle(hwnd, false);
                true
            }
            Err(err) => {
                crate::log_msg(&format!("[taskbar-window] SetParent failed: {err}"));
                apply_topmost_exstyle(hwnd, true);
                false
            }
        }
    }
}

fn apply_topmost_exstyle(hwnd: HWND, topmost: bool) {
    unsafe {
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let bit = WS_EX_TOPMOST.0 as isize;
        if topmost {
            ex |= bit;
        } else {
            ex &= !bit;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

/// 屏幕坐标：贴通知区左侧，并避让右侧天气/小组件板。
fn compute_screen_placement(width: i32, height: i32) -> Option<(i32, i32, i32, i32)> {
    let taskbar = find_taskbar()?;
    let mut bar = RECT::default();
    unsafe { GetWindowRect(taskbar, &mut bar) }.ok()?;
    let bar_w = bar.right - bar.left;
    let bar_h = bar.bottom - bar.top;
    let horizontal = bar_w >= bar_h;

    let height = height.min(bar_h.max(1));
    let width = width.min(bar_w.max(1));

    let notify = find_notify(taskbar).and_then(|hwnd| {
        let mut nr = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut nr) }.ok()?;
        Some(nr)
    });

    let (mut x, mut y) = if horizontal {
        let notify_left = notify.map(|nr| nr.left);
        let right_limit = [
            Some(bar.right),
            notify_left,
            right_side_widgets_left(taskbar, &bar, notify_left),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(bar.right);
        let x = (right_limit - width - GAP_FROM_NOTIFY).max(bar.left + GAP_FROM_NOTIFY);
        let y = bar.top + (bar_h - height).max(0) / 2;
        (x, y)
    } else if let Some(nr) = notify {
        let x = bar.left + (bar_w - width).max(0) / 2;
        let y = (nr.top - height - GAP_FROM_NOTIFY).max(bar.top + GAP_FROM_NOTIFY);
        (x, y)
    } else {
        (
            bar.left + (bar_w - width).max(0) / 2,
            bar.bottom - height - GAP_FROM_NOTIFY,
        )
    };

    x = x.clamp(bar.left, (bar.right - width).max(bar.left));
    y = y.clamp(bar.top, (bar.bottom - height).max(bar.top));
    Some((x, y, width, height))
}

/// 右侧天气/小组件板左缘。XAML 无 HWND，走 UIA。
/// `TaskbarDa` 每次热路径读取（注册表便宜）；开/关立即失效。UIA 矩形 500ms 刷新。
fn right_side_widgets_left(taskbar: HWND, bar: &RECT, notify_left: Option<i32>) -> Option<i32> {
    let notify_left = notify_left.unwrap_or(bar.right);
    let widgets_shown = taskbar_widgets_btn_shown();

    WIDGETS_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        let same_key = cache.notify_left == notify_left && cache.widgets_shown == widgets_shown;
        let fresh = same_key
            && cache
                .checked_at
                .is_some_and(|t| t.elapsed() < WIDGETS_UIA_TTL);
        if fresh {
            return cache.board_left;
        }

        let board_left = if !widgets_shown {
            None
        } else {
            match probe_widgets_board(taskbar, bar, notify_left) {
                WidgetsBoard::Right(left) => Some(left),
                WidgetsBoard::Absent => None,
                // UIA 失败才按 DPI 预留；左侧入口是 Absent，不挤右锚。
                WidgetsBoard::Unavailable => {
                    let dpi = window_dpi(taskbar);
                    let reserve = (WIDGETS_FALLBACK_WIDTH_96 * dpi.max(96) + 48) / 96;
                    Some(notify_left - reserve)
                }
            }
        };

        cache.checked_at = Some(Instant::now());
        cache.notify_left = notify_left;
        cache.widgets_shown = widgets_shown;
        cache.board_left = board_left;
        board_left
    })
}

#[derive(Clone, Copy)]
enum WidgetsBoard {
    /// 右半侧/贴通知区，右锚止于此。
    Right(i32),
    /// 关闭或仅左侧入口。
    Absent,
    /// COM/UIA 不可用。
    Unavailable,
}

fn probe_widgets_board(taskbar: HWND, bar: &RECT, notify_left: i32) -> WidgetsBoard {
    let Ok(board) = crate::win32::com::ComApartment::run(|| {
        let uia: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)? };
        let root = unsafe { uia.ElementFromHandle(taskbar)? };
        let condition = automation_id_condition(&uia, "WidgetsButton")?;
        let Ok(found) = (unsafe { root.FindFirst(TreeScope_Descendants, &condition) }) else {
            return Ok(WidgetsBoard::Absent);
        };
        let rect = unsafe { found.CurrentBoundingRectangle()? };
        if rect.right - rect.left <= 2 || rect.bottom - rect.top <= 2 {
            return Ok(WidgetsBoard::Absent);
        }
        // 左半侧是开始旁入口，右锚不避让。
        let mid = bar.left + (bar.right - bar.left) / 2;
        if rect.left < mid && rect.right < notify_left - 8 {
            return Ok(WidgetsBoard::Absent);
        }
        Ok(WidgetsBoard::Right(rect.left))
    }) else {
        return WidgetsBoard::Unavailable;
    };
    board
}

fn taskbar_widgets_btn_shown() -> bool {
    hkcu_dword(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        "TaskbarDa",
    )
    .is_some_and(|v| v != 0)
}

fn automation_id_condition(
    uia: &IUIAutomation,
    automation_id: &str,
) -> windows::core::Result<IUIAutomationCondition> {
    let mut value = VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 {
                    bstrVal: ManuallyDrop::new(BSTR::from(automation_id)),
                },
            }),
        },
    };
    let condition = unsafe { uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &value) };
    unsafe {
        let _ = VariantClear(&mut value);
    }
    condition
}

fn ensure_font_loaded() {
    let _ = FONT_LOADED.get_or_init(|| {
        let fonts_added = 0u32;
        let ok = unsafe {
            let handle = AddFontMemResourceEx(
                FONT_TTF.as_ptr() as *const _,
                FONT_TTF.len() as u32,
                None,
                &fonts_added,
            );
            !handle.is_invalid()
        };
        if !ok && let Some(path) = font_disk_path() {
            let wide = wide_null(&path);
            unsafe {
                windows::Win32::Graphics::Gdi::AddFontResourceExW(
                    PCWSTR(wide.as_ptr()),
                    windows::Win32::Graphics::Gdi::FR_PRIVATE,
                    None,
                );
            }
        }
        true
    });
}

fn font_disk_path() -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("fonts").join("Lilex-Regular.ttf"));
        candidates.push(dir.join("Lilex-Regular.ttf"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fonts/Lilex-Regular.ttf"));
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

fn ensure_class() -> anyhow::Result<()> {
    if CLASS_REGISTERED.get().is_some() {
        return Ok(());
    }
    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        style: Default::default(),
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        let err = windows::core::Error::from_thread();
        if err.code().0 as u32 != 1410 {
            return Err(anyhow::anyhow!("RegisterClassW failed: {err}"));
        }
    }
    let _ = CLASS_REGISTERED.set(());
    Ok(())
}

fn find_taskbar() -> Option<HWND> {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None).ok() }
}

fn find_notify(taskbar: HWND) -> Option<HWND> {
    unsafe { FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), None).ok() }
}

fn taskbar_dpi() -> i32 {
    window_dpi(find_taskbar().unwrap_or_default())
}

fn window_dpi(hwnd: HWND) -> i32 {
    unsafe {
        let hdc = GetDC(Some(hwnd));
        if hdc.is_invalid() {
            return 96;
        }
        let dpi = GetDeviceCaps(Some(hdc), LOGPIXELSY);
        let _ = ReleaseDC(Some(hwnd), hdc);
        if dpi > 0 { dpi } else { 96 }
    }
}

/// 浅色任务栏主题（黑字）。只信注册表；不采屏幕（BitBlt 会触发 DWM 重合成）。
fn system_uses_light_theme() -> Option<bool> {
    Some(
        hkcu_dword(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "SystemUsesLightTheme",
        )? != 0,
    )
}

fn hkcu_dword(subkey: &str, value_name: &str) -> Option<u32> {
    let subkey = wide_null(subkey);
    let mut key = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
    }
    .is_err()
    {
        return None;
    }
    let key = unsafe { crate::win32::handle::OwnedRegistryKey::from_raw(key) };
    let name = wide_null(value_name);
    let mut data = [0u8; 4];
    let mut data_size = data.len() as u32;
    let mut ty = REG_DWORD;
    if unsafe {
        RegQueryValueExW(
            key.raw(),
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            Some(data.as_mut_ptr()),
            Some(&mut data_size),
        )
    }
    .is_err()
        || ty != REG_DWORD
        || data_size < 4
    {
        return None;
    }
    Some(u32::from_le_bytes(data))
}

fn create_font(dpi: i32) -> anyhow::Result<HFONT> {
    ensure_font_loaded();
    let height = -((FONT_POINT_SIZE * dpi + 48) / 96);
    let face = wide_null(FONT_FACE);
    let font = unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_MEDIUM.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    };
    if font.is_invalid() {
        anyhow::bail!("CreateFontW failed");
    }
    Ok(font)
}

/// 两行排版。行距用 ascent。按可见字面（字顶到次行基线）居中，空的下行不把字顶上去。
struct Rows {
    height: i32,
    y0: i32,
    y1: i32,
}

fn rows(hdc: HDC) -> Rows {
    let mut tm = TEXTMETRICW::default();
    unsafe {
        let _ = GetTextMetricsW(hdc, &mut tm);
    }
    let cell = tm.tmHeight.max(1);
    let ascent = tm.tmAscent.clamp(1, cell);
    let internal = tm.tmInternalLeading.clamp(0, ascent);
    let descent = tm.tmDescent.max(0);
    let step = ascent + LINE_GAP;
    let body = ascent - internal;
    let visible = step + body;
    let pad = PAD_Y.max(descent);
    let height = visible + pad * 2;
    // 实测字墨质心比窗口中线高 2px（↓ 下行把外接框拉低）。下移后对齐任务栏时钟。
    let y0 = pad - internal + 2;
    Rows {
        height,
        y0,
        y1: y0 + step,
    }
}

fn measure_grid(
    font: HFONT,
    up: &str,
    down: &str,
    cpu: &str,
    ram: &str,
) -> anyhow::Result<(i32, i32)> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            anyhow::bail!("GetDC failed");
        }
        let old = SelectObject(screen, font.into());
        let (w_up, _) = text_extent(screen, up);
        let (w_down, _) = text_extent(screen, down);
        let (w_cpu, _) = text_extent(screen, cpu);
        let (w_ram, _) = text_extent(screen, ram);
        let (w_up_slot, _) = text_extent(screen, SLOT_UP);
        let (w_down_slot, _) = text_extent(screen, SLOT_DOWN);
        let (w_cpu_slot, _) = text_extent(screen, SLOT_CPU);
        let (w_ram_slot, _) = text_extent(screen, SLOT_RAM);
        let layout = rows(screen);
        let _ = SelectObject(screen, old);
        let _ = ReleaseDC(None, screen);

        let left_w = w_up.max(w_down).max(w_up_slot).max(w_down_slot);
        let right_w = w_cpu.max(w_ram).max(w_cpu_slot).max(w_ram_slot);
        let width = left_w + COL_GAP + right_w + PAD_X * 2;
        Ok((width.max(48), layout.height.max(20)))
    }
}

/// 指标/槽位字符串很短；返回写入的 UTF-16 单元数。
fn encode_utf16_stack(text: &str, wide: &mut [u16]) -> usize {
    let mut n = 0usize;
    for unit in text.encode_utf16() {
        if n >= wide.len() {
            break;
        }
        wide[n] = unit;
        n += 1;
    }
    n
}

fn text_extent(hdc: HDC, text: &str) -> (i32, i32) {
    let mut wide = [0u16; 48];
    let n = encode_utf16_stack(text, &mut wide);
    let mut size = SIZE::default();
    if n > 0 {
        unsafe {
            let _ = GetTextExtentPoint32W(hdc, &wide[..n], &mut size);
        }
    }
    (size.cx, size.cy)
}

/// 光栅化并 `UpdateLayeredWindow`：同一 DIB，避免中间 `Vec` 与二次位图。
fn present_layered(state: &TaskbarWindowState) {
    let w = state.width.max(1);
    let h = state.height.max(1);
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        if mem.is_invalid() {
            return;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            return;
        };
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(dib.0));
            let _ = DeleteDC(mem);
            return;
        }
        let _ = SelectObject(mem, dib.into());
        let old_font = SelectObject(mem, state.font.into());
        let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
        let light_text = state.light_text;
        if light_text {
            px.fill(0);
            let _ = SetTextColor(mem, COLORREF(0x00FF_FFFF));
        } else {
            px.fill(0x00FF_FFFF);
            let _ = SetTextColor(mem, COLORREF(0x0000_0000));
        }
        let _ = SetBkMode(mem, TRANSPARENT);
        let layout = rows(mem);
        let shift = (h - layout.height) / 2;
        let (w_up, _) = text_extent(mem, &state.up);
        let (w_down, _) = text_extent(mem, &state.down);
        let (w_up_slot, _) = text_extent(mem, SLOT_UP);
        let (w_down_slot, _) = text_extent(mem, SLOT_DOWN);
        let left_w = w_up.max(w_down).max(w_up_slot).max(w_down_slot);
        let x_left = PAD_X;
        let x_right = PAD_X + left_w + COL_GAP;
        let y0 = layout.y0 + shift;
        let y1 = layout.y1 + shift;
        draw_line(mem, &state.up, x_left, y0);
        draw_line(mem, &state.cpu, x_right, y0);
        draw_line(mem, &state.down, x_left, y1);
        draw_line(mem, &state.ram, x_right, y1);
        for pixel in px.iter_mut() {
            let v = *pixel;
            let b = v & 0xFF;
            let g = (v >> 8) & 0xFF;
            let r = (v >> 16) & 0xFF;
            let raw = if light_text {
                r.max(g).max(b)
            } else {
                255 - (r * 30 + g * 59 + b * 11) / 100
            };
            let a = deepen_coverage(raw);
            *pixel = if a < 2 {
                0
            } else if light_text {
                premult_bgra(255, 255, 255, a)
            } else {
                premult_bgra(0, 0, 0, a)
            };
        }
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            state.hwnd,
            None,
            None,
            Some(&size),
            Some(mem),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = SelectObject(mem, old_font);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(mem);
    }
}

fn premult_bgra(r: u32, g: u32, b: u32, a: u32) -> u32 {
    let r = r * a / 255;
    let g = g * a / 255;
    let b = b * a / 255;
    (a << 24) | (r << 16) | (g << 8) | b
}

fn deepen_coverage(a: u32) -> u32 {
    let a = a.min(255);
    if a < 2 {
        return 0;
    }
    (a + (a * (255 - a)) / 365).min(255)
}

fn draw_line(hdc: HDC, text: &str, x: i32, y: i32) {
    if text.is_empty() {
        return;
    }
    let mut wide = [0u16; 48];
    let n = encode_utf16_stack(text, &mut wide);
    if n == 0 {
        return;
    }
    unsafe {
        let _ = TextOutW(hdc, x, y, &wide[..n]);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            // 分层窗口的像素来自 UpdateLayeredWindow，不是这块 GDI 表面；这里只消掉脏区。
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONUP => LRESULT(0),
        WM_RBUTTONDOWN | WM_RBUTTONUP => LRESULT(0),
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_size_positive() {
        ensure_font_loaded();
        let font = create_font(96).expect("font");
        let (w, h) = measure_grid(font, "↑: 1.0 KB/s", "↓: 2.0 KB/s", "CPU: 10%", "RAM: 50%")
            .expect("extent");
        unsafe {
            let screen = GetDC(None);
            assert!(!screen.is_invalid());
            let old = SelectObject(screen, font.into());
            let layout = rows(screen);
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(screen, &mut tm);
            let _ = SelectObject(screen, old);
            let _ = ReleaseDC(None, screen);
            let _ = DeleteObject(font.into());
            let full_cells = tm.tmHeight * 2 + 1 + PAD_Y * 2;
            assert!(layout.height < full_cells);
            let glyph_top = layout.y0 + tm.tmInternalLeading;
            let glyph_bottom = layout.y1 + tm.tmAscent;
            assert_eq!(glyph_top, layout.height - glyph_bottom + 4);
            assert!(glyph_top >= tm.tmDescent);
            assert_eq!(h, layout.height.max(20));
        }
        assert!(w > 20);
        assert!(h > 10);
    }

    #[test]
    fn glyph_ink_vertical_center() {
        ensure_font_loaded();
        let font = create_font(96).expect("font");
        unsafe {
            let screen = GetDC(None);
            assert!(!screen.is_invalid());
            let mem = CreateCompatibleDC(Some(screen));
            let _ = ReleaseDC(None, screen);
            assert!(!mem.is_invalid());
            let old = SelectObject(mem, font.into());
            let layout = rows(mem);
            let w = 420;
            let h = layout.height.max(1);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib =
                CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
            let _ = SelectObject(mem, dib.into());
            let _ = SelectObject(mem, font.into());
            let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
            px.fill(0);
            let _ = SetTextColor(mem, COLORREF(0x00FF_FFFF));
            let _ = SetBkMode(mem, TRANSPARENT);
            draw_line(mem, "↑: 12.3 MB/s", PAD_X, layout.y0);
            draw_line(mem, "CPU: 10%", 200, layout.y0);
            draw_line(mem, "↓: 12.3 MB/s", PAD_X, layout.y1);
            draw_line(mem, "RAM: 50%", 200, layout.y1);
            let mut min_y = h;
            let mut max_y = -1;
            let mut weight = 0i64;
            let mut moment = 0i64;
            for y in 0..h {
                let row = &px[(y * w) as usize..(y * w + w) as usize];
                let mut row_w = 0i64;
                for p in row {
                    let v = (*p & 0xFF) as i64;
                    if v > 8 {
                        row_w += v;
                    }
                }
                if row_w > 0 {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                    weight += row_w;
                    moment += row_w * i64::from(y);
                }
            }
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
            let _ = DeleteDC(mem);
            let _ = DeleteObject(font.into());
            let centroid = moment / weight.max(1);
            let win_mid_sum = i64::from(h - 1);
            assert!(
                (centroid * 2 - win_mid_sum).abs() <= 1,
                "centroid {centroid} h {h}"
            );
        }
    }

    #[test]
    fn cells_match_traffic_monitor_shape() {
        assert_eq!(cell_up("1KB/s"), "↑: 1KB/s");
        assert_eq!(cell_down("2KB/s"), "↓: 2KB/s");
        assert_eq!(cell_cpu("10%"), "CPU: 10%");
        assert_eq!(cell_ram("38%"), "RAM: 38%");
    }
}
