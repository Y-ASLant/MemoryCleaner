use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use anyhow::{Context, Result, bail};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE,
    MSG, PostQuitMessage, PostThreadMessageW, RegisterClassW, TranslateMessage, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_DESTROY, WM_HOTKEY, WNDCLASSW,
};

use crate::settings::Settings;
use crate::tray::{CommandTx, TrayCommand};

const HOTKEY_ID_OPTIMIZE: i32 = 1;
const WM_APP_SHUTDOWN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;

static COMMAND_TX: OnceLock<CommandTx> = OnceLock::new();
static SERVICE: OnceLock<Mutex<HotkeyService>> = OnceLock::new();
static CLASS_REGISTERED: OnceLock<()> = OnceLock::new();

/// 解析后的全局热键组合，供 `RegisterHotKey` 使用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub virtual_key: VIRTUAL_KEY,
}

impl HotkeyBinding {
    pub const DEFAULT_CLEANUP: &'static str = "Ctrl+Alt+C";

    pub fn parse(chord: &str) -> Option<Self> {
        let chord = chord.trim();
        if chord.is_empty() {
            return None;
        }

        let parts: Vec<&str> = chord
            .split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 2 {
            return None;
        }

        let (modifiers, key) = parts.split_at(parts.len() - 1);
        let mut flags = HOT_KEY_MODIFIERS(0);
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => flags |= MOD_CONTROL,
                "alt" => flags |= MOD_ALT,
                "shift" => flags |= MOD_SHIFT,
                "win" | "windows" => flags |= MOD_WIN,
                _ => return None,
            }
        }

        if flags == HOT_KEY_MODIFIERS(0) {
            return None;
        }

        let virtual_key = parse_virtual_key(key[0])?;
        Some(Self {
            modifiers: flags | MOD_NOREPEAT,
            virtual_key,
        })
    }

    /// 由修饰键标志与主键 token 生成设置用组合，如 `Ctrl+Alt+C`。
    pub fn format_chord(
        control: bool,
        alt: bool,
        shift: bool,
        win: bool,
        key: &str,
    ) -> Option<String> {
        if is_modifier_key(key) {
            return None;
        }
        let key = normalize_key_token(key)?;
        if !control && !alt && !shift && !win {
            return None;
        }

        let mut parts = Vec::<String>::new();
        if control {
            parts.push("Ctrl".into());
        }
        if alt {
            parts.push("Alt".into());
        }
        if shift {
            parts.push("Shift".into());
        }
        if win {
            parts.push("Win".into());
        }
        parts.push(key.to_string());
        let chord = parts.join("+");
        Self::parse(&chord).map(|_| chord)
    }
}

fn is_modifier_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "shift"
            | "alt"
            | "control"
            | "ctrl"
            | "win"
            | "windows"
            | "super"
            | "cmd"
            | "meta"
            | "fn"
            | "capslock"
            | "caps lock"
    )
}

fn normalize_key_token(key: &str) -> Option<char> {
    let key = key.trim();
    if key.len() == 1 {
        let ch = key.chars().next()?;
        if ch.is_ascii_alphanumeric() {
            return Some(ch.to_ascii_uppercase());
        }
    }
    None
}

fn parse_virtual_key(key: &str) -> Option<VIRTUAL_KEY> {
    Some(VIRTUAL_KEY(normalize_key_token(key)? as u16))
}

struct HotkeyWorker {
    binding: HotkeyBinding,
    thread_id: u32,
    join_handle: Option<JoinHandle<()>>,
}

impl Drop for HotkeyWorker {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_APP_SHUTDOWN, WPARAM(0), LPARAM(0));
        }
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

struct HotkeyService {
    worker: Option<HotkeyWorker>,
}

impl HotkeyService {
    fn apply(&mut self, settings: &Settings) -> Result<()> {
        if !settings.cleanup_hotkey_enabled {
            self.worker = None;
            crate::log_msg("[hotkey] disabled");
            return Ok(());
        }

        let binding =
            HotkeyBinding::parse(&settings.cleanup_hotkey).context("invalid hotkey chord")?;
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.binding == binding)
        {
            return Ok(());
        }

        let worker = spawn_hotkey_worker(binding)?;
        self.worker = Some(worker);
        crate::log_msg(&format!("[hotkey] registered {}", settings.cleanup_hotkey));
        Ok(())
    }
}

fn spawn_hotkey_worker(binding: HotkeyBinding) -> Result<HotkeyWorker> {
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<u32>>(1);

    let join_handle = std::thread::Builder::new()
        .name("hotkey-listener".into())
        .spawn(move || {
            let thread_id = unsafe { GetCurrentThreadId() };
            let setup = run_hotkey_setup(binding);
            let _ = ready_tx.send(
                setup
                    .as_ref()
                    .map(|_| thread_id)
                    .map_err(|e| anyhow::anyhow!("{e:#}")),
            );

            let Ok(hwnd) = setup else {
                return;
            };

            unsafe {
                message_loop();
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID_OPTIMIZE);
                let _ = DestroyWindow(hwnd);
            }
        })
        .context("failed to spawn hotkey listener thread")?;

    let thread_id = match ready_rx
        .recv()
        .context("hotkey listener exited before registration completed")
        .and_then(|result| result)
    {
        Ok(thread_id) => thread_id,
        Err(error) => {
            let _ = join_handle.join();
            return Err(error);
        }
    };

    Ok(HotkeyWorker {
        binding,
        thread_id,
        join_handle: Some(join_handle),
    })
}

pub fn bind_command_sender(tx: CommandTx) {
    let _ = COMMAND_TX.set(tx);
}

pub fn sync(settings: &Settings) -> Result<()> {
    if settings.cleanup_hotkey_enabled && COMMAND_TX.get().is_none() {
        bail!("hotkey command channel unavailable");
    }

    SERVICE
        .get_or_init(|| Mutex::new(HotkeyService { worker: None }))
        .lock()
        .expect("hotkey service mutex poisoned")
        .apply(settings)
}

fn run_hotkey_setup(binding: HotkeyBinding) -> Result<HWND> {
    unsafe {
        register_hotkey_window_class()?;

        let hwnd = create_message_window()?;
        if let Err(error) = RegisterHotKey(
            Some(hwnd),
            HOTKEY_ID_OPTIMIZE,
            binding.modifiers,
            binding.virtual_key.0 as u32,
        ) {
            let _ = DestroyWindow(hwnd);
            return Err(error).context("RegisterHotKey failed");
        }

        Ok(hwnd)
    }
}

unsafe fn register_hotkey_window_class() -> Result<()> {
    if CLASS_REGISTERED.get().is_some() {
        return Ok(());
    }

    let instance = unsafe { GetModuleHandleW(None).context("GetModuleHandleW failed")? };
    let class_name = windows::core::w!("MemoryCleanerLiteHotkey");

    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(hotkey_wnd_proc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: class_name,
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&wnd_class) };
    if atom == 0 {
        bail!("RegisterClassW failed");
    }

    let _ = CLASS_REGISTERED.set(());
    Ok(())
}

unsafe fn create_message_window() -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None).context("GetModuleHandleW failed")? };
    let class_name = windows::core::w!("MemoryCleanerLiteHotkey");
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            windows::core::w!("MemoryCleanerLiteHotkey"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
    }
    .context("CreateWindowExW failed")?;

    Ok(hwnd)
}

unsafe extern "system" fn hotkey_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY if wparam.0 == HOTKEY_ID_OPTIMIZE as usize => {
            if let Some(tx) = COMMAND_TX.get() {
                let _ = tx.send(TrayCommand::Optimize);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn message_loop() {
    let mut msg = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if result.0 == 0 || result.0 == -1 {
            break;
        }

        if msg.message == WM_APP_SHUTDOWN {
            break;
        }

        let _ = unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn rebinding_preserves_old_registration_on_collision_and_skips_equivalent_chords() {
        let candidates = || {
            ('0'..='9').chain('A'..='Z').map(|key| Settings {
                cleanup_hotkey_enabled: true,
                cleanup_hotkey: format!("Ctrl+Alt+Shift+{key}"),
                ..Default::default()
            })
        };
        let mut service = HotkeyService { worker: None };
        let original = candidates()
            .find(|settings| service.apply(settings).is_ok())
            .expect("an uncommon hotkey must be available for the test");
        let original_binding = HotkeyBinding::parse(&original.cleanup_hotkey).unwrap();

        // 单独的 RAII worker 在整个测试期间占用冲突组合。
        // 两个 worker 均不绑定 COMMAND_TX，也不合成热键/清理命令。
        let (conflicting, _reservation) = candidates()
            .find_map(|settings| {
                let binding = HotkeyBinding::parse(&settings.cleanup_hotkey).unwrap();
                spawn_hotkey_worker(binding)
                    .ok()
                    .map(|worker| (settings, worker))
            })
            .expect("a second uncommon hotkey must be available for the test");

        assert!(service.apply(&conflicting).is_err());
        assert!(
            spawn_hotkey_worker(original_binding).is_err(),
            "the original chord must remain registered after a collision"
        );

        let mut equivalent = original.clone();
        equivalent.cleanup_hotkey = format!(
            "shift + ALT + control + {}",
            original
                .cleanup_hotkey
                .rsplit('+')
                .next()
                .unwrap()
                .to_ascii_lowercase()
        );
        service
            .apply(&equivalent)
            .expect("an equivalent binding must be a no-op");

        equivalent.cleanup_hotkey = "invalid".into();
        assert!(service.apply(&equivalent).is_err());
        assert!(
            spawn_hotkey_worker(original_binding).is_err(),
            "invalid settings must not unregister the original chord"
        );

        equivalent.cleanup_hotkey_enabled = false;
        service.apply(&equivalent).expect("disabling must succeed");
        let _released = spawn_hotkey_worker(original_binding)
            .expect("disabling must release the original chord");
    }

    #[test]
    fn format_chord_builds_register_hotkey_compatible_chords() {
        let chord =
            HotkeyBinding::format_chord(false, true, true, false, "c").expect("valid chord");
        assert_eq!(chord, "Alt+Shift+C");
        assert!(HotkeyBinding::parse(&chord).is_some());
    }

    #[test]
    fn format_chord_rejects_modifier_only_and_unmodified_keys() {
        assert!(HotkeyBinding::format_chord(false, true, false, false, "shift").is_none());
        assert!(HotkeyBinding::format_chord(false, false, false, false, "c").is_none());
    }

    #[test]
    fn parse_default_cleanup_hotkey() {
        let binding = HotkeyBinding::parse("Ctrl+Alt+C").expect("valid chord");
        assert_eq!(binding.modifiers, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT);
        assert_eq!(binding.virtual_key, VIRTUAL_KEY(b'C' as u16));
    }

    #[test]
    fn parse_rejects_empty_and_modifier_only_chords() {
        assert!(HotkeyBinding::parse("").is_none());
        assert!(HotkeyBinding::parse("Ctrl+Shift").is_none());
        assert!(HotkeyBinding::parse("M").is_none());
    }

    #[test]
    fn parse_supports_alt_and_win_modifiers() {
        let binding = HotkeyBinding::parse("Ctrl+Alt+O").expect("valid chord");
        assert_eq!(binding.modifiers, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT);
        assert_eq!(binding.virtual_key, VIRTUAL_KEY(b'O' as u16));

        let binding = HotkeyBinding::parse("Win+Shift+C").expect("valid chord");
        assert_eq!(binding.modifiers, MOD_WIN | MOD_SHIFT | MOD_NOREPEAT);
        assert_eq!(binding.virtual_key, VIRTUAL_KEY(b'C' as u16));
    }
}
