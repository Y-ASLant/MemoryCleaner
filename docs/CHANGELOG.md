# 更新日志

本文件记录 **Memory Cleaner Lite** 的版本更新，格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

**编写约定**（详见 `AGENTS.md`）：每个版本只记录相对上一 tag 的最终差异。

## [Unreleased]

### 修复

- **任务栏指标条**：避让 Win11 右侧天气/小组件板（UIA `WidgetsButton` + `TaskbarDa` 回退）；收紧贴通知区定位，降低遮挡系统托盘。

## [1.0.0] - 2026-11-17

首个版本。

### 新增

- **托盘设置菜单**：右键切换清理区域、自动清理与阈值、开机自启、通知、调试日志、全局热键开关、任务栏窗口与语言；打开配置目录。左键一键清理。
- **任务栏窗口**：通知区左侧 2×2（↑/↓/CPU/RAM）；`SetParent` 嵌任务栏，失败则 `TOPMOST`；`UpdateLayeredWindow` 整帧提交。`show_taskbar_window`。
- **CPU 与网速**：托盘提示与任务栏条；`sysinfo` 约 1s 采样；环回/隧道不计入网速。
- **调试日志**：可选 `App.log`（exe 旁，单文件 ≤ 2 MiB）。
- **产品身份**：`MemoryCleanerLite.exe`、`%APPDATA%\MemoryCleanerLite`、独立互斥体/自启任务/AUMID。发版 tag 可用 `v1.0.0-lite`。

---

# Changelog

**Memory Cleaner Lite**. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- **Taskbar strip**: avoid Win11 right-side weather/widgets board (UIA `WidgetsButton` + `TaskbarDa` fallback); tighter notify-edge placement to reduce tray occlusion.

## [1.0.0] - 2026-11-17

Initial release.

### Added

- **Tray settings menu**: areas, auto-cleanup/threshold, startup, notifications, debug log, hotkey toggle, taskbar strip, language, open config folder. Left-click runs cleanup.
- **Taskbar strip**: 2×2 ↑/↓/CPU/RAM; `SetParent` / `TOPMOST`; atomic `UpdateLayeredWindow`. `show_taskbar_window`.
- **CPU and network**: tooltip + strip via `sysinfo` (~1s); loopback/tunnels excluded from rates.
- **Debug log**: optional `App.log` beside exe (≤ 2 MiB single file).
- **Product identity**: `MemoryCleanerLite.exe`, `%APPDATA%\MemoryCleanerLite`, separate mutex / startup task / AUMID. Release tag may be `v1.0.0-lite`.

[Unreleased]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.0-lite...HEAD
[1.0.0]: https://github.com/Y-ASLant/MemoryCleaner/releases/tag/v1.0.0-lite
