# Repository Guidelines

## Project Overview

Memory Cleaner is a **Windows-only** tray-resident memory-optimization tool written in Rust. It monitors physical and system committed memory (and CPU/network via `sysinfo`), and requests memory reclamation through Windows NT memory-management APIs (`NtSetSystemInformation`, `SetSystemFileCacheSize`, etc.). Settings are changed from the **system-tray right-click menu**. Most cleanup operations require administrator privileges. Licensed MIT.

## Architecture & Data Flow

```
main.rs → OS gate → wake-signal → ensure_elevated() → wake-signal retry
       → single-instance mutex + wake event → locale + notification::init
       → hotkey bind → Tray::install (HWND on main thread)
       → Runtime::start (settings, metrics, auto-cleanup)
       → tray::run_loop: PeekMessage + TrayCommand drain until quit
 │
 ├─ runtime.rs (Settings, optimize, auto-cleanup, metrics; no GUI)
 ├─ tray.rs (tray-icon + muda menu; thread_local TrayState on main thread)
 ├─ auto_cleanup.rs (threshold policy / cooldown)
 ├─ log.rs (optional App.log)
 ├─ locale.rs (rust-i18n)
 ├─ memory.rs (GlobalMemoryStatusEx + GetPerformanceInfo; NtQuery FileCache)
 ├─ system_metrics.rs (sysinfo: CPU + network)
 ├─ optimize.rs (MemoryAreas → NT purge steps)
 ├─ settings.rs (%APPDATA%\MemoryCleaner\settings.toml)
 ├─ privileges.rs
 ├─ messages.rs / version.rs
 └─ win32/ (elevation, hotkey, memory_notification, notification, nt, os,
           process, single_instance, startup, volume, handle, com)
```

- **Entry flow:** `main.rs` → minimum-OS gate (`win32::os::is_supported`, build ≥ 9200 = Windows 8) → wake existing instance (before elevation) → `ensure_elevated()` → wake retry → single-instance mutex + event watcher → `locale::apply` → `notification::init` → hotkey sender → `tray::install` on main thread → `Runtime::start` → `tray::run_loop` (commands + Win32 messages).
- **No GPUI / no main window:** Tray icon + menu only. `TrayIcon` / muda items are **not `Send`**; state lives in `thread_local!` on the main thread. Background work posts `TrayCommand`s; only the main loop calls tray APIs.
- **i18n:** `rust-i18n` with `locales/zh-CN.yml` (`_version: 2`, zh-CN + en). `settings.language` is `auto` | `zh-CN` | `en`; `auto` uses `GetUserDefaultUILanguage`. Language menu updates locale and tray labels immediately.
- **Native layer:** `src/win32/` wraps Win32/NT APIs; `src/optimize.rs` runs cleanup steps on a worker thread.
- **Console suppression:** `#![windows_subsystem = "windows"]`; diagnostics via `OutputDebugStringA` / optional `App.log`.
- **Tray command channel:** One `mpsc` carries tray clicks/menu, hotkeys, single-instance wake, low-memory, spin frames, and optimize finished into the main `run_loop`.
- **Automatic cleanup:** Low-memory `MemoryNotificationMonitor` + threshold poll thread; both honor `auto_cleanup_enabled`. Threshold `0` disables threshold triggers.
- **Settings via tray:** Cleanup area checks (standby variants mutually exclusive), auto-cleanup, threshold presets, run at startup, notifications, debug log, hotkey enable, language, open config folder. **Process exclusion list** and **hotkey chord string** are TOML-only (no GUI picker/recorder).
- **Obsolete UI-only fields:** `always_on_top`, `close_to_notification_area` still deserialize for compat; unused at runtime.
- **Hotkey:** Transactional rebind in `win32::hotkey` (`RegisterHotKey` worker thread). Enable/disable from tray; chord edit via `settings.toml`.
- **Startup:** Task Scheduler logon task `/RL HIGHEST`; always tray-only (no “launch hidden window”).
- **COM lifetime:** `ComApartment::run` for toast/startup paths; release COM objects before leaving the apartment.
- **Second instance:** Wake event → `TrayCommand::ActivateWindow` → same path as left-click cleanup (no window).

## Key Directories

| Path | Purpose |
|---|---|
| `src/` | Application source (`main.rs` entry, `runtime.rs` core) |
| `locales/` | rust-i18n YAML (`zh-CN.yml`, zh-CN + en) |
| `docs/` | Changelog and technical notes |
| `.github/workflows/` | Build / tag-release |
| `src/win32/` | Win32/NT bindings |
| `.codegraph/` | Codegraph index (gitignored) |

## Development Commands

```bash
just format   # cargo fmt --all
just check    # cargo clippy --release -- -D warnings
just test     # cargo test
just build    # cargo build --release (clippy first)
cargo run
cargo run --release
just clean
```

**CI:** `.github/workflows/build.yml` on PRs/pushes to `main` and `v*` tags. Tags publish release when version matches `Cargo.toml`; notes from `docs/CHANGELOG.md`.

**Tests:** `cargo test` — unit tests in `src/` + `tests/settings_persistence.rs`.

## Code Conventions

- Rust Edition 2024, latest stable, Windows-only.
- `Result` / `Option`; `anyhow` on optimize paths; settings use concrete errors.
- `unsafe` concentrated in `src/win32/` and `optimize.rs`.
- Settings: atomic TOML write under `%APPDATA%\MemoryCleaner\settings.toml`.
- `MemoryAreas` bitflags in `optimize.rs`.
- Embedded `App.ico` (`winres`) / `App.png` (`include_bytes!` in tray).
- Debug logging: `log_msg` → OutputDebugString; `log::write` → `App.log` when enabled.

## Important Files

| File | Role |
|---|---|
| `src/main.rs` | Entry: elevation, single-instance, tray, message loop |
| `src/runtime.rs` | Settings, optimize, auto-cleanup, metrics command handlers |
| `src/tray.rs` | Tray icon, settings menu, spin, main-thread message pump |
| `src/optimize.rs` | Cleanup orchestration |
| `src/settings.rs` | TOML schema / persistence |
| `src/win32/hotkey.rs` | Global cleanup hotkey |
| `src/win32/memory_notification.rs` | Low-memory monitor |
| `docs/CHANGELOG.md` | Bilingual Unreleased + releases |

## Tray UX

- **Left-click:** run cleanup.
- **Right-click:** full settings menu (areas, auto-cleanup, toggles, language).
- **Tooltip:** physical %, commit %, system working set, CPU, network.
- **Spin:** icon frames during cleanup; `SetSpinFrame` applied on main thread only.

## Runtime / Tooling

- MSVC toolchain; no `rust-toolchain.toml` / clippy.toml / rustfmt.toml.
- No async runtime dependency (std threads + main pump).
- Release: LTO, strip, `opt-level = "z"`, `panic = "abort"`.
- Binary: `MemoryCleaner.exe`.

## Documentation & Changelog

- `docs/CHANGELOG.md` — 中文 above, English below, `---` separator.
- Each release = final diff vs previous tag only.
- Sections: 新增/变更/移除 · Added/Changed/Removed.
- Version bump: `Cargo.toml` + changelog + tag.

## Testing & QA

- Unit/integration: `cargo test`.
- Manual: elevated cleanup, tray menu persistence to settings.toml, hotkey, toast, second-instance wake.
- Diagnostics: DebugView; optional `App.log`.
