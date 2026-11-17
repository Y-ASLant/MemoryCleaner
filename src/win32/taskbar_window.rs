//! 任务栏信息窗口（TrafficMonitor 同类）。
//!
//! 优先 `SetParent` 嵌进 `Shell_TrayWnd`；失败再 `TOPMOST`。
//! 绘制：TM 不闪路径。D2D 画进合成交换链，`Present` 后 `Commit`。DComp 失败才退回 `UpdateLayeredWindow`。
//! 仅主线程调用。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, AddFontMemResourceEx, BI_RGB, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, BitBlt, CLIP_DEFAULT_PRECIS, CreateCompatibleDC,
    CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, FW_MEDIUM, GetDC, GetDeviceCaps, GetTextExtentPoint32W, GetTextMetricsW, HDC,
    HFONT, HGDIOBJ, LOGPIXELSY, OUT_DEFAULT_PRECIS, ReleaseDC, SRCCOPY, ScreenToClient,
    SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, TextOutW, ValidateRect,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GWL_EXSTYLE,
    GetWindowLongPtrW, GetWindowRect, HMENU, HWND_TOPMOST, IDC_ARROW, IsChild, LoadCursorW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_DEFERERASE, SWP_NOACTIVATE, SWP_NOCOPYBITS,
    SWP_NOREDRAW, SWP_NOSENDCHANGING, SWP_NOZORDER, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use windows::core::{PCWSTR, w};

use crate::memory::MemorySection;
use crate::system_metrics::CpuNetworkSnapshot;
use crate::tray::TrayCommand;
use crate::win32::wide_null;

const CLASS_NAME: PCWSTR = w!("MemoryCleaner.TaskbarWindow");
const PAD_X: i32 = 4;
const PAD_Y: i32 = 2;
const COL_GAP: i32 = 10;
const LINE_GAP: i32 = 0;
const GAP_FROM_NOTIFY: i32 = 6;
/// 任务栏亮度 ≥ 此值 → 黑字，否则白字。
const LIGHT_TASKBAR_LUMA: u32 = 140;
const THEME_RECHECK: Duration = Duration::from_secs(5);
const FONT_TTF: &[u8] = include_bytes!("../fonts/Lilex-Medium.ttf");
const FONT_FACE: &str = "Lilex";
const FONT_POINT_SIZE: i32 = 13;
/// 列宽上限样例，避免网速/百分比位数变化改窗宽。
const SLOT_UP: &str = "↑: 999.99 MB/s";
const SLOT_DOWN: &str = "↓: 999.99 MB/s";
const SLOT_CPU: &str = "CPU: 100%";
const SLOT_RAM: &str = "RAM: 100%";

thread_local! {
    static STATE: RefCell<Option<TaskbarWindowState>> = const { RefCell::new(None) };
    static REPAINTING: Cell<bool> = const { Cell::new(false) };
    static PRESENTING: Cell<bool> = const { Cell::new(false) };
}

static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();
static FONT_LOADED: OnceLock<bool> = OnceLock::new();
static CMD_TX: OnceLock<std::sync::mpsc::Sender<TrayCommand>> = OnceLock::new();

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
    comp: Option<crate::win32::taskbar_comp::Comp>,
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

pub fn install(command_tx: std::sync::mpsc::Sender<TrayCommand>) -> anyhow::Result<()> {
    let _ = CMD_TX.set(command_tx);
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

    let embedded = try_embed(hwnd);
    let comp =
        match crate::win32::taskbar_comp::attach(hwnd, width.max(1) as u32, height.max(1) as u32) {
            Ok(comp) => {
                // 合成视觉自己显示。留着分层样式会盖住它，或和 ULW 抢同一层。
                set_ex_bit(hwnd, WS_EX_LAYERED.0, false);
                set_ex_bit(hwnd, WS_EX_NOREDIRECTIONBITMAP.0, true);
                Some(comp)
            }
            Err(err) => {
                crate::log_msg(&format!("[taskbar-window] DComp init failed: {err:#}"));
                None
            }
        };
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
            light_text: !taskbar_is_light(),
            theme_checked_at: Instant::now(),
            comp,
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
        if dpi != state.dpi
            && dpi > 0
            && let Ok(font) = create_font(dpi)
        {
            unsafe {
                let _ = DeleteObject(state.font.into());
            }
            state.font = font;
            state.dpi = dpi;
        }

        let up = cell_up(&cpu_network.upload_label());
        let down = cell_down(&cpu_network.download_label());
        let cpu = cell_cpu(&cpu_network.cpu_label());
        let ram = cell_ram(&physical.percent_label());

        let mut theme_changed = false;
        if state.theme_checked_at.elapsed() >= THEME_RECHECK {
            state.theme_checked_at = Instant::now();
            let light_text = !taskbar_is_light();
            if state.light_text != light_text {
                state.light_text = light_text;
                theme_changed = true;
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
                // 1px 测量抖动会 ResizeBuffers / SetWindowPos，偶发闪一下。
                if state.pos_x == i32::MIN
                    || (state.width - w).abs() > 2
                    || (state.height - h).abs() > 2
                {
                    state.width = w;
                    state.height = h;
                }
            }
        }

        relocate_and_paint(state, text_changed || theme_changed);
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

    let geometry_changed = (state.pos_x - screen_x).abs() > 8
        || (state.pos_y - screen_y).abs() > 8
        || (state.width - width).abs() > 2
        || (state.height - height).abs() > 2;

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

        unsafe {
            let _ = SetWindowPos(state.hwnd, insert_after, x, y, width, height, flags);
        }
        state.pos_x = screen_x;
        state.pos_y = screen_y;
        state.width = width;
        state.height = height;
    }

    present_frame(state);
}

fn ensure_embedded(state: &mut TaskbarWindowState) {
    let Some(taskbar) = find_taskbar() else {
        return;
    };
    if unsafe { IsChild(taskbar, state.hwnd).as_bool() } {
        state.embedded = true;
        return;
    }
    // 未嵌入：不再每秒 SetParent（失败路径会闪）。仅「曾经嵌入又丢了」才立即重试。
    if !state.embedded {
        return;
    }
    if try_embed(state.hwnd) {
        state.embedded = true;
        crate::log_msg("[taskbar-window] SetParent re-embed ok");
    } else {
        state.embedded = false;
        apply_topmost_exstyle(state.hwnd, true);
    }
}

fn try_embed(hwnd: HWND) -> bool {
    let Some(taskbar) = find_taskbar() else {
        return false;
    };
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
        let bit = isize::try_from(WS_EX_TOPMOST.0).unwrap_or(0);
        if topmost {
            ex |= bit;
        } else {
            ex &= !bit;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

fn set_ex_bit(hwnd: HWND, bit: u32, on: bool) {
    unsafe {
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let bit = isize::try_from(bit).unwrap_or(0);
        if on {
            ex |= bit;
        } else {
            ex &= !bit;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
    }
}

/// 屏幕坐标：托盘通知区左侧。
fn compute_screen_placement(width: i32, height: i32) -> Option<(i32, i32, i32, i32)> {
    let taskbar = find_taskbar()?;
    let mut bar = RECT::default();
    unsafe { GetWindowRect(taskbar, &mut bar) }.ok()?;
    let bar_w = bar.right - bar.left;
    let bar_h = bar.bottom - bar.top;
    let horizontal = bar_w >= bar_h;

    let height = height.min(bar_h.max(1));
    let width = width.min(bar_w.max(1));

    let (mut x, mut y) = if let Some(notify) = find_notify(taskbar) {
        let mut nr = RECT::default();
        if unsafe { GetWindowRect(notify, &mut nr) }.is_ok() {
            if horizontal {
                let mut x = nr.left - width - GAP_FROM_NOTIFY;
                if x < bar.left + GAP_FROM_NOTIFY {
                    x = bar.left + GAP_FROM_NOTIFY;
                }
                let y = bar.top + (bar_h - height).max(0) / 2;
                (x, y)
            } else {
                let x = bar.left + (bar_w - width).max(0) / 2;
                let mut y = nr.top - height - GAP_FROM_NOTIFY;
                if y < bar.top + GAP_FROM_NOTIFY {
                    y = bar.top + GAP_FROM_NOTIFY;
                }
                (x, y)
            }
        } else if horizontal {
            (
                bar.right - width - GAP_FROM_NOTIFY,
                bar.top + (bar_h - height).max(0) / 2,
            )
        } else {
            (
                bar.left + (bar_w - width).max(0) / 2,
                bar.bottom - height - GAP_FROM_NOTIFY,
            )
        }
    } else if horizontal {
        (
            bar.right - width - GAP_FROM_NOTIFY,
            bar.top + (bar_h - height).max(0) / 2,
        )
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
        candidates.push(dir.join("fonts").join("Lilex-Medium.ttf"));
        candidates.push(dir.join("Lilex-Medium.ttf"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fonts/Lilex-Medium.ttf"));
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

/// 浅色任务栏 → true（应用黑字）。Win11 合成栏上窗口 DC 的 GetPixel 常为 0，不可靠。
fn taskbar_is_light() -> bool {
    if let Some(light) = system_uses_light_theme() {
        return light;
    }
    screen_taskbar_luma()
        .map(|luma| luma >= LIGHT_TASKBAR_LUMA)
        .unwrap_or(false)
}

fn system_uses_light_theme() -> Option<bool> {
    let subkey = wide_null(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let mut key = HKEY::default();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
    };
    if open.is_err() {
        return None;
    }
    let key = unsafe { crate::win32::handle::OwnedRegistryKey::from_raw(key) };
    let name = wide_null("SystemUsesLightTheme");
    let mut data = [0u8; 4];
    let mut data_size = data.len() as u32;
    let mut value_type = REG_DWORD;
    let query = unsafe {
        RegQueryValueExW(
            key.raw(),
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut value_type),
            Some(data.as_mut_ptr()),
            Some(&mut data_size),
        )
    };
    if query.is_err() || data_size < 4 {
        return None;
    }
    let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    Some(value != 0)
}

/// 从屏幕 DC `BitBlt` 采样任务栏左侧（避开右侧托盘浮层）。
fn screen_taskbar_luma() -> Option<u32> {
    let taskbar = find_taskbar()?;
    let mut bar = RECT::default();
    unsafe { GetWindowRect(taskbar, &mut bar) }.ok()?;
    let y = bar.top + (bar.bottom - bar.top).max(1) / 2;
    let xs = [
        bar.left + 40,
        bar.left + (bar.right - bar.left).max(1) / 5,
        bar.left + (bar.right - bar.left).max(1) / 3,
    ];

    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        if mem.is_invalid() {
            let _ = ReleaseDC(None, screen);
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 1,
                biHeight: 1,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        if dib.is_err() || bits.is_null() {
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return None;
        }
        let dib = dib.ok()?;
        let old = SelectObject(mem, dib.into());
        let px = &mut *(bits as *mut u32);
        let mut sum = 0u32;
        let mut n = 0u32;
        for x in xs {
            if BitBlt(mem, 0, 0, 1, 1, Some(screen), x, y, SRCCOPY).is_err() {
                continue;
            }
            let v = *px;
            let b = v & 0xFF;
            let g = (v >> 8) & 0xFF;
            let r = (v >> 16) & 0xFF;
            sum += (r * 299 + g * 587 + b * 114) / 1000;
            n += 1;
        }
        let _ = SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);
        sum.checked_div(n)
    }
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

fn text_extent(hdc: HDC, text: &str) -> (i32, i32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    unsafe {
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
    }
    (size.cx, size.cy)
}

fn present_frame(state: &mut TaskbarWindowState) {
    // Present/Commit 会嵌套派发 WM_PAINT。再进 present 会清掉正在显示的缓冲。
    if PRESENTING.replace(true) {
        return;
    }
    let _guard = PresentGuard;
    if state.comp.is_some() {
        let pixels = rasterize(state);
        let w = state.width.max(1) as u32;
        let h = state.height.max(1) as u32;
        if let Some(comp) = state.comp.as_mut()
            && comp.present(&pixels, w, h).is_ok()
        {
            return;
        }
        // 窗口已不是分层窗。这里再 ULW 会空一帧，下一秒才补上，看起来像偶发闪。
        crate::log_msg("[taskbar-window] DComp present failed");
        return;
    }
    present_layered(state);
}

struct PresentGuard;

impl Drop for PresentGuard {
    fn drop(&mut self) {
        PRESENTING.set(false);
    }
}

fn present_layered(state: &TaskbarWindowState) {
    let pixels = rasterize(state);
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
        let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        if dib.is_err() || bits.is_null() {
            let _ = DeleteDC(mem);
            return;
        }
        let dib = dib.unwrap_or_default();
        let _ = SelectObject(mem, dib.into());
        let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
        let n = px.len().min(pixels.len());
        px[..n].copy_from_slice(&pixels[..n]);
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
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(mem);
    }
}

fn rasterize(state: &TaskbarWindowState) -> Vec<u32> {
    let w = state.width.max(1);
    let h = state.height.max(1);
    let mut pixels = vec![0u32; (w * h) as usize];
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return pixels;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let _ = ReleaseDC(None, screen);
        if mem.is_invalid() {
            return pixels;
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
        let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        if dib.is_err() || bits.is_null() {
            let _ = DeleteDC(mem);
            return pixels;
        }
        let dib = dib.unwrap_or_default();
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
        pixels.copy_from_slice(px);
        let _ = SelectObject(mem, old_font);
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(mem);
    }
    pixels
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
    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let _ = TextOutW(hdc, x, y, &wide);
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
            // BeginPaint 会让 DWM 给这个窗口合成一帧空表面。只校验脏区，并重新挂上上一帧。
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            if !REPAINTING.get() && !PRESENTING.get() {
                REPAINTING.set(true);
                STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut()
                        && let Some(comp) = state.comp.as_mut()
                    {
                        let _ = comp.recommit();
                    }
                });
                REPAINTING.set(false);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONUP => {
            if let Some(tx) = CMD_TX.get() {
                let _ = tx.send(TrayCommand::Optimize);
            }
            LRESULT(0)
        }
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
