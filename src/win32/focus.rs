//! Track the last external foreground window so paste can restore focus.
//!
//! Memory Cleanr runs elevated; `SendInput` to medium-IL apps is blocked by UIPI.
//! Restoring the previous HWND and posting `WM_PASTE` is the reliable path.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

static PREV_FOREGROUND_HWND: AtomicIsize = AtomicIsize::new(0);
static OUR_HWND: AtomicIsize = AtomicIsize::new(0);

/// Remember our main window HWND (excluded when saving previous focus).
pub fn set_our_hwnd(hwnd: HWND) {
    OUR_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
}

pub fn clear_our_hwnd() {
    OUR_HWND.store(0, Ordering::Relaxed);
}

/// Save the current foreground window if it is not our own.
pub fn save_current_focus() {
    let hwnd = unsafe { GetForegroundWindow() };
    let val = hwnd.0 as isize;
    if val == 0 {
        return;
    }
    let our = OUR_HWND.load(Ordering::Relaxed);
    if our != 0 && val == our {
        return;
    }
    PREV_FOREGROUND_HWND.store(val, Ordering::Relaxed);
}

/// Restore the previously saved foreground window (best effort).
pub fn restore_previous_foreground() -> bool {
    let prev = PREV_FOREGROUND_HWND.load(Ordering::Relaxed);
    if prev == 0 {
        crate::log_msg("[focus] no previous foreground hwnd saved");
        return false;
    }
    let hwnd = HWND(prev as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            crate::log_msg(&format!("[focus] previous hwnd {prev:#x} invalid"));
            return false;
        }
        let current = GetForegroundWindow();
        if current.0 as isize == prev {
            return true;
        }
        // Nudge the input queue so SetForegroundWindow is more likely to succeed.
        let _ = GetWindowThreadProcessId(hwnd, None);
        let ok = SetForegroundWindow(hwnd).as_bool();
        if !ok {
            crate::log_msg(&format!("[focus] SetForegroundWindow({prev:#x}) failed"));
        }
        ok
    }
}

/// HWND that should receive paste (saved previous, else current foreground).
pub fn paste_target_hwnd() -> Option<HWND> {
    let prev = PREV_FOREGROUND_HWND.load(Ordering::Relaxed);
    if prev != 0 {
        let hwnd = HWND(prev as *mut _);
        if unsafe { IsWindow(Some(hwnd)).as_bool() } {
            return Some(hwnd);
        }
    }
    let fg = unsafe { GetForegroundWindow() };
    let our = OUR_HWND.load(Ordering::Relaxed);
    if fg.0 as isize != 0 && fg.0 as isize != our {
        Some(fg)
    } else {
        None
    }
}
