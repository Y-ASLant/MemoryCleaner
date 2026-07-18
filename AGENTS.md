# Repository Guidelines

## Project Overview

Memory Cleanr is a **Windows-only** GUI memory-optimization tool written in Rust with the **GPUI** framework (from the Zed editor). It frees physical and virtual memory by calling Windows NT memory-management APIs (`NtSetSystemInformation`, `SetSystemFileCacheSize`, etc.), runs as a system-tray resident app, and requires administrator privileges for most operations. Licensed MIT.

## Architecture & Data Flow

```
main.rs → ensure_elevated() → notification::init
 │
 ├─ --startup → Tray Host 会话（常驻）
 │    runtime/tray_host.rs → Win32 MessageLoop + tray + hotkey + IPC 服务端
 │    service/optimize_runner.rs（托盘菜单/热键触发清理）
 │
 └─ 普通启动 → ensure_tray_host_running() → GUI 单例 → GPUI 会话
      runtime/gui_app.rs → app.rs（主窗口、设置、清理进度 UI）
      win32/ipc.rs（Named Pipe：RegisterGui / Spin / SettingsChanged）
 │
 ├─ app.rs (GUI 状态、内存刷新、优化进度、窗口隐藏/恢复)
 ├─ runtime/tray_host.rs (Tray Host 状态、托盘同步、后台清理)
 ├─ service/memory.rs (共享内存查询/刷新 helpers)
 ├─ service/optimize_runner.rs (GUI 与 Tray 共用的清理编排)
 ├─ log.rs (optional App.log file output, timestamp-based retention)
 ├─ locale.rs (rust-i18n locale apply, list separator, lang-id mapping)
 ├─ memory.rs (GlobalMemoryStatusEx → MemoryStatus)
 ├─ optimize.rs (MemoryAreas bitflags → NT cache-purge steps)
 ├─ settings.rs (TOML persistence at %APPDATA%\MemoryCleaner\settings.toml)
 ├─ privileges.rs (SeProfileSingleProcessPrivilege, SeIncreaseQuotaPrivilege)
 ├─ tray.rs (tray-icon crate, App.png embedded via include_bytes!)
 ├─ icon_cache.rs (Explorer icon cache purge)
 ├─ version.rs (version constant)
 ├─ ui/ (GPUI components: layout, memory_card, settings_page, theme, title_bar)
 └─ win32/ (hotkey, ipc, message_loop, optimize_lock, notification, nt, os, process, single_instance, startup, volume, window)
```

- **双进程模型：** 同一可执行文件，Tray Host（`--startup`）负责托盘、全局热键、IPC 服务端与无 GUI 时的清理；GUI 进程按需拉起，通过 Named Pipe 向 Tray Host 注册窗口句柄、同步设置变更、驱动托盘图标旋转。两进程各持独立 Mutex 单例（`single_instance.rs`）。
- **Entry flow:** `main.rs` → elevation → `locale::apply` → `notification::init` →（startup）`ensure_tray_singleton` + `run_tray_session`；或（GUI）`ensure_tray_host_running` + `ensure_gui_singleton` + `run_gui_session` → GPUI `QuitMode::Explicit` → `open_main_window`。
- **跨进程清理互斥：** `win32/optimize_lock.rs` 命名 Mutex，防止 GUI 与 Tray Host 同时执行 NT 清理。
- **IPC 协议（GUI → Tray）：** 长度前缀帧 + 单字节 tag：`RegisterGui`、`UnregisterGui`、`SpinStart`、`SpinStop`、`SettingsChanged`。Tray 就绪通过 `MemoryCleanr_TrayReady_v1` 事件握手。
- **Tray 命令通道：** Tray Host 内 `mpsc` 承载托盘菜单/热键命令；转发线程 `PostMessage(WM_APP_TRAY_CMD)` 汇入 Win32 消息循环（`message_loop.rs`），避免在非托盘线程调用 `set_icon`。
- **内存状态：** GUI 与 Tray Host 各自轮询 `GlobalMemoryStatusEx`（GUI 1 s、Tray 500 ms）；共享逻辑在 `service/memory.rs::refresh_sections`。
- **i18n:** `rust-i18n` with `locales/zh-CN.yml` (single file, `_version: 2`, zh-CN + en). `settings.language` is `auto` | `zh-CN` | `en`; `auto` uses `GetUserDefaultUILanguage` via `win32::os::system_ui_locale()`. Language changes call `MemoryCleanerApp::apply_locale()` to refresh memory labels and tray menu text immediately.
- **Async runtime:** `smol` 用于 GPUI 侧异步（内存轮询、清理进度、Toast）；Tray Host 以 Win32 消息循环 + 普通线程为主。
- **UI stack:** GPUI + `gpui-component` (Button, Checkbox, Switch, GroupBox, ProgressCircle, Kbd).
- **Native layer:** `src/win32/` wraps low-level Windows APIs; `src/optimize.rs` orchestrates the cleanup steps.
- **Console suppression:** `main.rs` uses `#![windows_subsystem = "windows"]`; diagnostics go to `OutputDebugStringA` (viewable via DebugView). Optional file logging via `src/log.rs` when `debug_logging` is enabled.
- **Window lifecycle:** Closing with `close_to_notification_area` hides the GPUI window and exits the GUI process; Tray Host keeps running. `activate_or_spawn_gui()` reopens or spawns GUI. Memory polling in GUI pauses when the window closes.

## Key Directories

| Path | Purpose |
|---|---|
| `src/` | Application source (binary crate, main.rs entry point) |
| `src/runtime/` | Tray Host / GUI 会话入口（`tray_host.rs`, `gui_app.rs`） |
| `src/service/` | 共享业务逻辑（内存查询 helpers、`optimize_runner`） |
| `src/ui/` | GPUI UI components (layout, memory_card, settings_page, theme, title_bar) |
| `locales/` | rust-i18n translation YAML (`zh-CN.yml`, zh-CN + en strings) |
| `docs/` | Project docs (`CHANGELOG.md`, technical comparisons) |
| `src/win32/` | Win32/NT API bindings (hotkey, ipc, message_loop, optimize_lock, notification, nt, os, process, single_instance, startup, volume, window) |
| `vendor/proc-macro-error2/` | Vendored patch for Rust 1.97+ compatibility (see below) |
| `.codegraph/` | Codegraph index (gitignored) |

## Development Commands

```bash
# Format
make fmt # cargo fmt

# Lint (clippy with -D warnings — warnings are errors)
make check # cargo clippy -- -D warnings

# Test
make test # cargo test

# Build (release, runs clippy first)
make build # cargo build --release

# Run (debug)
cargo run

# Run (release behavior — console suppressed)
cargo run --release

# Clean
make clean # cargo clean
```

**Tests:** `make test` / `cargo test` — 52 unit tests in `src/` plus 2 integration tests in `tests/settings_persistence.rs`.

## Code Conventions & Common Patterns

- **Language:** Rust, Edition 2024 (requires Rust 1.96+).
- **Platform:** Windows-only. All modules assume `target_os = "windows"`.
- **Error handling:** Functions return `Result<T, E>` or use `Option` for fallible lookups. `anyhow` is used in optimize/icon_cache paths; settings and most UI code use concrete errors.
- **Unsafe / FFI:** `unsafe` is concentrated in `src/win32/` (NT API calls, privilege token manipulation, hotkey message loop) and `src/optimize.rs` (NtSetSystemInformation). Each unsafe block is narrowly scoped.
- **Naming:** Standard Rust conventions — `snake_case` functions/variables, `PascalCase` types, `SCREAMING_SNAKE_CASE` constants. Win32 wrappers match the original API names.
- **State management:** `MemoryCleanerApp` in `app.rs` owns all application state (settings, memory stats, optimization progress, hotkey recording). UI reads from this state via GPUI's `Render` trait.
- **Settings persistence:** TOML file at `%APPDATA%\MemoryCleaner\settings.toml`, written atomically (temp file + rename), debounced 300 ms.
- **Bitflags:** `MemoryAreas` in `optimize.rs` uses the `bitflags` crate to represent configurable cleaning regions.
- **Embedded assets:** `App.ico` compiled into the binary via `winres` (`build.rs`); `App.png` embedded via `include_bytes!` in `tray.rs`.
- **Debug logging:** `log_msg()` always writes to `OutputDebugString` (and stderr in debug builds). `log::write()` additionally appends to `App.log` beside the executable when `settings.debug_logging` is true. Before each write, `log.rs` purges lines whose `[unix_secs.millis]` prefix is older than 7 days (`LOG_RETENTION_SECS`).
- **Platform UI chrome:** `win32::os::is_windows_11_or_later()` uses `RtlGetVersion` (build ≥ 22000 = Win11). `ui::theme::init_light_theme` sets gpui-component `radius` / `radius_lg` to 0 and disables `shadow` on Win10 so buttons, cards, and dialogs render with square corners. Custom UI must use `cx.theme().radius`, not hardcoded `rounded(px(...))`.

## Important Files

| File | Role |
|---|---|
| `src/main.rs` | Entry point — elevation, single-instance, notification init, tray/hotkey setup, GPUI launch |
| `src/app.rs` | Core application state, memory refresh loop, optimization, window hide/restore, hotkey recording |
| `src/tray.rs` | Tray icon install, cleanup spin animation, tooltip/menu sync, command dispatch |
| `src/win32/hotkey.rs` | `RegisterHotKey` in dedicated thread; sends `TrayCommand::Optimize` |
| `src/win32/notification.rs` | Windows Toast + Start Menu shortcut for AppUserModelID |
| `src/log.rs` | Optional `App.log` file output with timestamp-based line retention |
| `src/ui/theme.rs` | Light theme init + Win10 square-corner chrome |
| `src/locale.rs` | rust-i18n locale apply, list separator, lang-id mapping |
| `src/win32/os.rs` | Windows build detection (Win10 vs Win11), system UI locale |
| `src/optimize.rs` | Memory cleanup orchestration (8 cleaning regions) |
| `src/settings.rs` | TOML settings schema and persistence |
| `src/win32/nt.rs` | Raw NT API bindings (`NtSetSystemInformation`, `NtCreateFile`, structs, enums) |
| `src/win32/volume.rs` | Mount Manager volume enumeration and modified-file-cache flush |
| `src/win32/startup.rs` | Run-at-startup registry toggle (`HKCU\...\Run`) |
| `docs/CHANGELOG.md` | Version changelog (final diff vs previous release only) |
| `Cargo.toml` | Dependencies, features, release profile (LTO, strip, abort-on-panic) |
| `build.rs` | Icon embedding via `winres` |
| `Makefile` | fmt / check / build / clean targets |

## UI Layout Notes

- **Window size:** fixed width 520px; collapsed height ~294px, expanded ~630px (`src/app.rs` + `src/ui/layout.rs`).
- **Collapsed view:** memory cards + cleanup button.
- **Expanded view:** adds cleanup-area checkboxes panel (`settings_page::render_settings_content`).
- **Window behavior dialog** (always on top, close-to-tray, run at startup, debug logging, optimization notifications, cleanup hotkey + recording, language): opened from title-bar gear icon; `overlay_closable(false)` — clicking the backdrop does not close it.
- **Optimization feedback:** progress and result text render inside the cleanup button; result clears after 5 seconds (`OPTIMIZE_RESULT_DISPLAY`).
- **Memory refresh:** `MEMORY_REFRESH_INTERVAL` = 1 s while main window is visible; paused when hidden to tray (`pause_memory_refresh` / `start_memory_refresh`).
- **Platform chrome:** Win10 (build &lt; 22000) uses square corners via theme tokens; Win11 keeps gpui-component defaults.

## Unimplemented Settings (Reserved)

These fields exist in `settings.toml` for forward compatibility but have no runtime logic yet:

- `auto_optimization_interval` / `auto_optimization_memory_usage` — scheduled or threshold-triggered auto cleanup
- `tray_icon_*` — reserved tray icon settings (no runtime logic)

Implemented since earlier docs (do **not** list as unimplemented):

- `show_optimization_notifications` — Windows Toast on optimize start/complete
- `cleanup_hotkey_enabled` / `cleanup_hotkey` — global hotkey via `RegisterHotKey`
- `run_at_startup` — silent launch to tray after sign-in via `win32::startup`

## Documentation & Changelog

- **Changelog file:** `docs/CHANGELOG.md` — bilingual (中文 above, English below, separated by `---`; no mixed-language bullets). Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
- **Scope rule:** Each release entry records **only the final diff** between that version and the **immediately previous tagged release** (e.g. `git diff v1.0.1..HEAD`). Do **not** list intermediate commits, in-progress refactors, or step-by-step bug fixes from development.
- **What to include:** User-visible features, behavior changes, new/removed settings, dependency or build-profile changes, and noteworthy docs. Describe the **shipped outcome**, not the debugging journey.
- **What to omit:** Separate “fix” bullets for issues discovered and resolved before release; RAII refactors, API path corrections, or review feedback that never shipped independently.
- **Sections:** Chinese block uses `### 新增` / `### 变更` / `### 移除`; English block uses `### Added` / `### Changed` / `### Removed`. Mirror the same bullets in both blocks. Older releases may stay as one-line summaries pointing to git history.
- **Version bump:** When preparing a release, update `Cargo.toml` `version`, add the `docs/CHANGELOG.md` section, and tag (e.g. `v1.0.2`). Compare link at file bottom uses `Y-ASLant/MemoryCleanr` on GitHub.
- **Technical docs:** Deeper implementation notes (e.g. API comparisons) live under `docs/` and are referenced from the changelog when relevant; they are not a substitute for the changelog entry.

## Tray Icon Spin During Cleanup

While `run_optimize` is in progress, `tray::start_spin()` posts `TrayCommand::SetSpinFrame` ticks every 120ms; the GPUI thread applies them via `set_icon`. Do not call `TrayIcon::set_icon` from background threads — `tray-icon` requires the Win32 tray window thread.

## Runtime / Tooling Preferences

- **Toolchain:** Rust 1.96+ with MSVC (Windows Build Tools or Visual Studio required).
- **No rust-toolchain.toml, .cargo/config.toml, clippy.toml, or rustfmt.toml** — defaults only.
- **Async:** `smol` (not tokio).
- **Vendored patch:** `proc-macro-error2` 2.0.1 is vendored under `vendor/` to fix `E0365` on Rust 1.97+ (changes `extern crate proc_macro` to `pub extern crate proc_macro`). Remove when upstream releases a fix.
- **Release profile:** Aggressive optimization — LTO enabled, symbols stripped, `opt-level = "s"` (size), single codegen unit, `panic = "abort"`.
- **Package manager:** Cargo only. No npm, no other package managers.
- **Binary name:** `MemoryCleanr.exe` (see `[[bin]]` name in `Cargo.toml`).

## Testing & QA

- **Unit tests:** `cargo test` — memory formatting, cleanup messages, settings TOML, tray tooltip, hotkey chord parse/format, optimize step plan, layout metrics, icon-cache outcomes, notification XML escape, volume flush helpers.
- **Integration tests:** `tests/settings_persistence.rs` — settings save/load and atomic write in isolated `%APPDATA%`.
- **Manual QA:** Win32 memory cleanup, tray, GPUI dialogs, Explorer restart, global hotkey, Windows Toast (admin required for most cleanup).
- **Diagnostics:** DebugView for `OutputDebugString`; optional `App.log` when debug logging is enabled.
