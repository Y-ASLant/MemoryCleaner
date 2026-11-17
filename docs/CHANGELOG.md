# 更新日志

本文件记录 **Memory Cleaner Lite** 的版本更新，格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

与完整版 Memory Cleaner（master）独立；不收录 master 历史。

**编写约定**（详见 `AGENTS.md`）：每个版本只记录相对上一 tag 的最终差异。

## [Unreleased]

## [1.0.0] - 2026-11-17

### 新增

- **托盘设置菜单**：右键切换清理区域、自动清理与阈值、开机自启、通知、调试日志、全局热键开关、任务栏窗口与语言；打开配置目录。左键一键清理。
- **任务栏窗口**：通知区左侧 2×2（↑/↓/CPU/RAM）；`SetParent` 嵌任务栏，失败则 `TOPMOST`；`UpdateLayeredWindow` 整帧提交。`show_taskbar_window`。
- **CPU 与网速**：托盘提示与任务栏条；`sysinfo` 约 1s 采样；环回/隧道不计入网速。
- **调试日志**：可选 `App.log`（exe 旁，单文件 ≤ 2 MiB）。

### 变更

- **产品身份**：Memory Cleaner Lite `1.0.0`——`MemoryCleanerLite.exe`、`%APPDATA%\MemoryCleanerLite`、独立互斥体/自启任务/AUMID，可与完整版并行。发版 tag 可用 `v1.0.0-lite`。
- **托盘常驻**：无 GPUI/主窗口；主线程 Win32 消息泵 + 托盘菜单。
- **工作集清理**：仅系统 `EmptyWorkingSets`（无进程排除列表）。
- **配置**：不读写 master 遗留键（`always_on_top` / `close_to_notification_area` / `cleanup_history` / `excluded_processes` 等），加载忽略、保存丢弃。

### 移除（相对完整版）

- 主窗口、设置对话框、清理历史、进程排除 UI、热键录制 UI、桌面图标缓存刷新。
- 按进程 EmptyWorkingSet 路径与 `win32/process` 模块。

---

# Changelog

**Memory Cleaner Lite** only. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Independent of full Memory Cleaner (master); master history is not kept here.

## [Unreleased]

## [1.0.0] - 2026-11-17

### Added

- **Tray settings menu**: areas, auto-cleanup/threshold, startup, notifications, debug log, hotkey toggle, taskbar strip, language, open config folder. Left-click runs cleanup.
- **Taskbar strip**: 2×2 ↑/↓/CPU/RAM; `SetParent` / `TOPMOST`; atomic `UpdateLayeredWindow`. `show_taskbar_window`.
- **CPU and network**: tooltip + strip via `sysinfo` (~1s); loopback/tunnels excluded from rates.
- **Debug log**: optional `App.log` beside exe (≤ 2 MiB single file).

### Changed

- **Product identity**: Memory Cleaner Lite `1.0.0` — `MemoryCleanerLite.exe`, `%APPDATA%\MemoryCleanerLite`, separate mutex / startup task / AUMID; side-by-side with full edition. Release tag may be `v1.0.0-lite`.
- **Tray-only**: no GPUI/main window; Win32 pump + tray menu.
- **Working set**: system `EmptyWorkingSets` only (no process exclusion list).
- **Settings**: master-only keys ignored on load and dropped on save.

### Removed (vs full edition)

- Main window, settings dialogs, cleanup history, process-exclusion UI, hotkey recorder, icon-cache refresh.
- Per-process EmptyWorkingSet path and `win32/process`.

[Unreleased]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.0-lite...HEAD
[1.0.0]: https://github.com/Y-ASLant/MemoryCleaner/releases/tag/v1.0.0-lite
