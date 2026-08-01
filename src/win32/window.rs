use anyhow::{Context, Result};
use gpui::{Bounds, Pixels, Point, Window, point, px, size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST,
    IsIconic, SHOW_WINDOW_CMD, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX,
};

fn show_window(hwnd: HWND, cmd: SHOW_WINDOW_CMD) -> Result<()> {
    unsafe {
        // ShowWindow returns the previous visibility state, not success/failure.
        let _ = ShowWindow(hwnd, cmd);
    }
    Ok(())
}

fn apply_extended_style(hwnd: HWND, update: impl FnOnce(u32) -> u32) -> Result<()> {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, update(style) as _);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    Ok(())
}

pub(crate) fn hwnd_from_window(window: &Window) -> Result<HWND> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|e| anyhow::anyhow!("window handle unavailable: {e}"))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        anyhow::bail!("unsupported platform window handle");
    };

    Ok(HWND(win32.hwnd.get() as _))
}

/// Restore the window from tray-only hidden state.
pub fn show_from_tray(window: &Window) -> Result<()> {
    // Capture the app the user was in before we take focus (needed for paste).
    crate::win32::focus::save_current_focus();
    let hwnd = hwnd_from_window(window)?;
    crate::win32::focus::set_our_hwnd(hwnd);
    apply_extended_style(hwnd, |style| {
        (style & !WS_EX_TOOLWINDOW.0) | WS_EX_APPWINDOW.0
    })?;
    let cmd = unsafe {
        if IsIconic(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        }
    };
    show_window(hwnd, cmd)?;
    Ok(())
}

pub fn set_always_on_top(window: &Window, on_top: bool) -> Result<()> {
    let hwnd = hwnd_from_window(window)?;
    let insert_after = if on_top {
        Some(HWND_TOPMOST)
    } else {
        Some(HWND_NOTOPMOST)
    };

    unsafe {
        SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .context("SetWindowPos failed")?;
    }

    Ok(())
}

/// Temporarily hide a window without destroying it (for paste focus hand-off).
pub fn hide_hwnd(hwnd: HWND) {
    let _ = show_window(hwnd, SW_HIDE);
}

/// Show a previously hidden window again.
pub fn show_hwnd(hwnd: HWND) {
    let _ = show_window(hwnd, SW_SHOW);
}

/// Show without activating (keeps paste target focused while our UI reappears).
pub fn show_hwnd_noactivate(hwnd: HWND) {
    let _ = show_window(hwnd, SW_SHOWNOACTIVATE);
}

/// Remove the maximize/restore button from the window title bar.
pub fn remove_maximize_button(window: &Window) -> Result<()> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let new_style = style & !WS_MAXIMIZEBOX.0;
        SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as _);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    Ok(())
}

/// Outer window rectangle in screen coordinates.
pub fn window_screen_bounds(window: &Window) -> Result<Bounds<Pixels>> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect")?;
        Ok(Bounds::new(
            point(px(rect.left as f32), px(rect.top as f32)),
            size(
                px((rect.right - rect.left) as f32),
                px((rect.bottom - rect.top) as f32),
            ),
        ))
    }
}

/// Capture mouse input for the application window during a cross-window drag.
pub fn capture_mouse(window: &Window) -> Result<()> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        SetCapture(hwnd);
    }
    Ok(())
}

/// Release mouse input captured for a cross-window drag.
pub fn release_mouse_capture() {
    unsafe {
        let _ = ReleaseCapture();
    }
}

/// Move a window without resizing or changing z-order.
pub fn set_window_screen_origin(window: &Window, origin: Point<Pixels>) -> Result<()> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            f32::from(origin.x) as i32,
            f32::from(origin.y) as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .context("SetWindowPos move")?;
    }
    Ok(())
}

/// Hide from the taskbar (tool window); pair with [`set_always_on_top`] for drag ghosts.
pub fn set_tool_window(window: &Window) -> Result<()> {
    let hwnd = hwnd_from_window(window)?;
    apply_extended_style(hwnd, |style| style | WS_EX_TOOLWINDOW.0)
}
