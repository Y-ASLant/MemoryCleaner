# 更新日志

本文件记录 Memory Cleaner 的版本更新，格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

**编写约定**（详见 `AGENTS.md` → Documentation & Changelog）：每个版本只记录相对上一 tag 的**最终差异**，不记录开发过程中的中间修改或逐步修复。


## [Unreleased]

### 变更

- **全局热键**：新组合键注册失败时保留原绑定和设置，并在设置界面显示错误；重复应用相同组合键不再重新注册。
- **通知资源生命周期**：通知与快捷方式操作配对初始化和释放 COM，通知使用局部 WinRT 激活工厂，避免重复调用累积 COM 初始化状态。
- **开机自启**：启动同步和设置更新改为后台执行；更新期间禁用开关，失败时保留原设置并提示错误；`schtasks` 子进程等待上限为 10 秒，超时后请求终止该子进程。
- **工作集清理结果**：进程枚举、打开进程和清空工作集的异常失败现在会返回错误；所有目标均因权限不足而失败时不再报告成功。
- **管理员提升**：UAC 重新启动改用 `ShellExecuteExW` 返回的确切子进程句柄确认创建成功，不再用同名进程判断。
- **托盘显示同步**：内存轮询仍更新 Tooltip，但仅在语言或窗口可见性变化时写入原生菜单文本。

### 移除

- **注册表缓存清理**：移除不释放内存、只通过 `RegFlushKey` 强制注册表配置单元写盘的清理区域；旧设置中的对应位会在规范化时丢弃。

## [1.0.7] - 2026-09-04

相对 [1.0.6] 的最终变更如下。

### 变更

- **界面边距与卡片间距统一**：主窗口外围内边距、卡片间距、卡片标题与内容间隙及底部清理按钮间隙统一对齐为 6 px；修复展开状态下清理区域卡片与底部按钮之间的多余高度计算偏差，使折叠与展开状态下卡片到底部清理按钮的间距保持严格一致（均为 6 px）。
- **UI 依赖迁移至 gpui-kit**：将原有的 `gpui` 与 `gpui-component` git 依赖统一迁移至 crates.io 发布的 `gpui-kit` 0.6；最低支持的 Rust 工具链升级至 1.97.1+（Edition 2024），并移除本地 `proc-macro-error2` patch 补丁。
- **模块解耦与 Win32 资源 RAII**：将庞大的 `app.rs` 与 `settings_page.rs` 按职责拆分为子模块；新增 `src/win32/handle.rs` 提供 Win32 内核对象与注册表键的 RAII 封装，统一资源生命周期管理以防止句柄泄露；拆分独立 `elevation` 模块处理管理员提权检测与重新拉起。
- **项目文档**：更新中英文 README 与开发者规范以反映模块化架构及 `gpui-kit` 依赖变更。


## [1.0.6] - 2026-08-31

相对 [1.0.5] 的最终变更如下。

### 变更

- **内存监控动画**：物理内存与虚拟内存卡片改为在每秒采样之间进行线性插值；新样本从当前显示位置连续重定向，圆环颜色与动画百分比同步，窗口隐藏时立即取消刷新任务。
- **自动清理生命周期**：Windows 低内存监控现在严格跟随自动清理开关启停；阈值为 `0` 时不创建轮询任务，修改阈值会重置持续压力计数，重新启用时可正确检测当前低内存状态。
- **调试日志**：日志保留扫描由每次写入改为至多每小时一次；合法时间戳日志继续保留 7 天，异常格式内容限制为最新 256 行且不超过 64 KiB，文件读写错误会输出到调试流。
- **发布流程**：GitHub Actions 改为仅在推送 `v*` 标签时构建和发布；标签必须与 `Cargo.toml` 版本一致，GitHub Release 说明直接提取 `docs/CHANGELOG.md` 中该版本的中英文区域。
- **项目文档**：中英文 README 补充自动清理、开机启动、进程排除、单实例唤醒、设置字段及相关模块说明。


## [1.0.5] - 2026-08-30

相对 [1.0.4] 的最终变更如下。

### 新增

- **自动清理策略**：自动清理支持 Windows 低内存通知和持续内存占用阈值两种触发方式；可在窗口行为设置中选择阈值。
- **GitHub Actions**：新增仅由 `v*` 标签触发的 Windows 构建与发布流程；标签须匹配版本号，随后自动执行格式检查、Clippy、测试、release 构建，并创建附带 `MemoryCleaner.exe` 的 GitHub Release。

### 变更

- **阈值清理**：连续两次检测到内存占用超过阈值后即可首次清理；后续阈值清理保留 10 分钟冷却时间，避免持续压力下重复清理。
- **已有实例唤醒**：再次启动程序时会通知已有实例显示主窗口；同一权限级别下无需额外 UAC 提示。
- **开机启动**：改用以最高权限运行的登录计划任务，并在每次同步时刷新任务配置，确保程序更新或移动后仍使用当前可执行文件。
- **启动可靠性**：计划任务查询会区分任务不存在与系统错误；单实例互斥体和唤醒监听器的创建失败会中止启动并记录错误。
- **当前策略说明**：自动清理设置会明确显示当前启用的触发条件；阈值关闭时，说明仅由 Windows 低物理内存通知触发。
- **选择器布局**：自动清理的阈值下拉选择器宽度与语言选择器统一，减少弹出菜单与触发控件的宽度差异。

### 移除

- **定时清理**：移除定时间隔设置与触发逻辑；旧配置中的间隔字段会被安全忽略并在下次保存时移除。


## [1.0.4] - 2026-07-29

相对 [1.0.3] 的最终变更如下。

### 新增

- **自动清理**：窗口行为设置中新增「自动清理」开关；启用后，程序通过 Windows 低物理内存通知触发清理，并在内存压力恢复前不会重复触发，避免轮询和频繁清理。
- **展开/折叠动画**：清理区域设置面板展开与折叠时使用固定时长布局动画，并支持中途反向切换。

### 变更

- **虚拟内存显示**：主界面固定显示物理内存与虚拟内存两张卡片，移除隐藏虚拟内存的分支逻辑。
- **动画调度**：连续动画改为通过 GPUI `window.request_animation_frame()` 请求下一帧，减少展开/折叠时的卡顿与逐帧感。
- **布局高度计算**：展开窗口高度由设置面板的显式布局高度推导，移除硬编码展开高度，避免展开或折叠后内容被裁切。
- **动画基础设施**：`src/anim.rs` 新增固定时长插值器，用于布局过渡；原有指数平滑动画继续用于内存数值、环形图和清理进度。
- **Toast 快捷方式校验**：Windows Toast 初始化时会检查开始菜单快捷方式是否指向当前程序；路径不匹配时自动重建，避免移动程序后通知入口失效。
- **文档配置表**：README 移除未实现的预留配置说明，保留当前实际支持的设置项。

### 移除

- **未实现的预留配置项**：移除 `show_virtual_memory`、托盘图标自定义预留字段，以及旧的自动优化间隔/阈值预留字段；已保存的未知 TOML 字段仍会被兼容忽略。

## [1.0.3] - 2026-07-20

相对 [1.0.2] 的最终变更如下。

### 新增

- **平滑动画**：内存使用率环形图、清理进度条、内存数值文字（已用/可用字节）在数据刷新时平滑过渡，而非直接跳变。动画采用指数衰减插值，~300ms 到达目标值的 95%。
- **动画模块**：新增 `src/anim.rs`，`AnimatedValue` 插值器可供全 crate 复用。
- **动画智能暂停**：窗口隐藏到托盘时动画循环完全停止（零 CPU），窗口恢复后自动重启。

### 修复

- **托盘菜单状态**：关闭窗口后右键托盘菜单正确显示「显示窗口」而非「隐藏窗口」；从托盘恢复窗口后正确显示「隐藏窗口」。
- **窗口错误路径**：`activate_window` 和 `open_window` 失败时正确重置 `window_shown` 状态。
- **优化完成后托盘同步**：内存清理完成后立即同步托盘提示文本，不再等待下次鼠标悬停。
## [1.0.2] - 2026-07-19

相对 [1.0.1] 的最终变更如下。

### 新增

- **开机自启**：设置中可开启「登录 Windows 后静默启动到系统托盘」，不显示主窗口（`src/win32/startup.rs`）。
- **进程排除选择器**：列表项显示实例数与内存占用；无法读取内存时显示占位符。
- **文档**：`README_EN.md`（英文说明）、`docs/API_COMPARISON_MEMREDUCT.md`（与 Mem Reduct 清理 API 对比）。

### 变更

- **已修改文件缓存**：由遍历 `A:`–`Z:` 固定磁盘盘符，改为通过 Mount Manager 枚举 `\??\Volume{GUID}` 并刷写；新增 `src/win32/volume.rs` 统一管理枚举、刷写与结果汇总；至少一个卷刷写成功即视为该步骤成功。
- **清理进度文案**：已修改文件步骤由显示盘符（如 `C:`）改为显示 `Volume{GUID}`。
- **进程排除交互**：从进程列表选择后直接加入排除列表，移除中间「待确认」状态。
- **Release 构建**：`opt-level` 由 `z` 调整为 `s`。
- **应用图标**：更新 `App.ico` / `App.png`。
- **名称修正**：项目名称由 `Memory Cleanr` 修正为 `Memory Cleaner`，同步更新二进制名、包名、窗口标题、托盘快捷方式、互斥量等所有引用。

## [1.0.1] - 2026-07-16

相对 [1.0.0] 的主要变更：进程排除、全局清理热键（默认 Ctrl+Alt+C）与热键录制、优化完成 Toast、界面国际化、托盘清理动画、图标缓存刷新、调试日志、Windows 10 方形圆角主题等。详见 git history。

## [1.0.0] - 2026-07-11

首个公开发布：8 种内存清理区域、GPUI 界面、系统托盘、管理员提升、设置持久化。

---

# Changelog

Records Memory Cleaner releases. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Writing rules** (see `AGENTS.md` → Documentation & Changelog): each release entry covers **only the final diff** vs the previous tag — not intermediate commits or step-by-step fixes during development.



## [Unreleased]

### Changed

- **Global hotkey** — Failed registrations retain the previous binding and settings and display an error. Reapplying the same shortcut no longer re-registers it.
- **Notification resource lifecycle** — Notification and shortcut operations balance COM initialization and teardown; notifications use scoped WinRT activation factories to prevent repeated calls from accumulating COM initialization state.
- **Run at startup** — Launch-time synchronization and setting updates run in the background. The switch is disabled while pending, and failed updates retain the previous setting and display an error. Each `schtasks` child wait is limited to 10 seconds, after which termination is requested.
- **Working Set cleanup results** — Unexpected process-enumeration, process-open, and working-set-emptying failures now return errors. Cleanup no longer reports success when every target failed with access denied.
- **Administrator elevation** — UAC relaunch now confirms creation through the exact child-process handle returned by `ShellExecuteExW`, rather than treating any same-name process as successful.
- **Tray display synchronization** — Memory polling still refreshes the tooltip, while native menu text is updated only when the locale or window visibility changes.

### Removed

- **Registry Cache cleanup** — Removed the cleanup region that did not release memory and only forced registry hives to disk through `RegFlushKey`; normalization discards its legacy settings bit.

## [1.0.7] - 2026-09-04

Final changes since [1.0.6].

### Changed

- **Unified UI padding and card spacing** — Window content padding, inter-card gaps, card title-to-content spacing, and the cleanup-button gap are now strictly aligned to a unified 6 px grid. Corrected the expanded-height calculation to remove excess bottom gap, ensuring the space between the bottom card and the cleanup button remains exactly 6 px in both collapsed and expanded states.
- **Migrated UI dependencies to gpui-kit** — Replaced direct git dependencies for `gpui` and `gpui-component` with the crates.io release `gpui-kit` 0.6. The minimum supported Rust toolchain is updated to 1.97.1+ (Edition 2024), and the local `proc-macro-error2` patch is removed.
- **Modular architecture and Win32 RAII handles** — Split the monolithic `app.rs` and `settings_page.rs` into dedicated submodules by responsibility. Added `src/win32/handle.rs` with RAII wrappers for Win32 kernel handles and registry keys to guarantee leak-free resource cleanup across NT, process, and volume subsystems, and isolated UAC elevation handling into `src/win32/elevation.rs`.
- **Project documentation** — Updated Chinese and English READMEs and repository guidelines to document the modularized codebase and `gpui-kit` dependency.


## [1.0.6] - 2026-08-31

Final changes since [1.0.5].

### Changed

- **Memory-monitor animation** — Physical- and virtual-memory cards now interpolate linearly across each one-second sample interval. New samples retarget from the displayed value, ring colors track the animated percentage, and hiding the window cancels the refresh task immediately.
- **Automatic-cleanup lifecycle** — The Windows low-memory monitor now strictly follows the Automatic Cleanup switch. A zero threshold creates no polling task, threshold changes reset sustained-pressure counting, and re-enabling the feature correctly observes an active low-memory condition.
- **Debug logging** — Retention scanning now runs at most hourly instead of on every write. Valid timestamped entries retain the seven-day policy, malformed content is capped at the newest 256 lines and 64 KiB, and file I/O failures are reported to the debug stream.
- **Release workflow** — GitHub Actions now builds and publishes only for pushed `v*` tags, which must match the version in `Cargo.toml`; GitHub Release notes are extracted directly from that version's Chinese and English sections in `docs/CHANGELOG.md`.
- **Project documentation** — The Chinese and English READMEs now cover automatic cleanup, startup, process exclusions, existing-instance activation, settings fields, and related modules.


## [1.0.5] - 2026-08-30

Final changes since [1.0.4].

### Added

- **Automatic-cleanup policies** — Automatic cleanup now supports Windows low-memory notifications and sustained memory-usage thresholds. The threshold is configurable in Window Behavior settings.
- **GitHub Actions** — Added a Windows build-and-release workflow triggered only by `v*` tags. The tag must match the package version; it checks formatting, runs Clippy and tests, builds the release binary, and creates a GitHub Release with `MemoryCleaner.exe`.

### Changed

- **Threshold cleanup** — The first cleanup runs after two consecutive above-threshold checks; subsequent threshold cleanups retain a 10-minute cooldown to avoid repeated cleanup under sustained pressure.
- **Existing-instance activation** — Launching the application again signals the existing instance to show its main window without an additional UAC prompt at the same integrity level.
- **Startup** — Logon startup now uses a highest-privilege scheduled task and refreshes its configuration on every sync, so updates or moved installations continue to launch the current executable.
- **Startup reliability** — Scheduled-task queries now distinguish a missing task from system errors; failures to create the single-instance mutex or wake listener abort startup and are logged.
- **Current-policy explanation** — Automatic Cleanup now explicitly states the active triggers. When the threshold is off, it says that only Windows low-physical-memory notifications can trigger cleanup.
- **Selector layout** — The automatic-cleanup threshold selector now matches the language selector width, reducing the width mismatch between the trigger control and its popup menu.

### Removed

- **Scheduled cleanup** — Removed the scheduled-interval setting and trigger. Legacy interval fields in saved configuration are safely ignored and removed on the next save.
## [1.0.4] - 2026-07-29

Final changes since [1.0.3].

### Added

- **Automatic cleanup** — Added an "Automatic Cleanup" switch in Window Behavior settings. When enabled, cleanup is triggered by Windows low-physical-memory notifications and is rearmed only after memory pressure recovers, avoiding polling and repeated cleanup loops.
- **Expand/collapse animation** — The cleanup settings panel now uses a fixed-duration layout transition when expanding or collapsing, and supports reversing mid-animation.

### Changed

- **Virtual memory display** — The main window now always shows both physical-memory and virtual-memory cards, removing the hidden-virtual-memory rendering branch.
- **Animation scheduling** — Continuous animations now request frames through GPUI `window.request_animation_frame()`, reducing stutter and frame-stepping during expand/collapse.
- **Layout height calculation** — Expanded window height is derived from explicit settings-panel layout metrics instead of a hardcoded expanded height, preventing content clipping after expand/collapse.
- **Animation foundation** — `src/anim.rs` now includes a fixed-duration interpolator for layout transitions; the existing exponential smoothing remains for memory values, usage rings, and cleanup progress.
- **Toast shortcut validation** — Windows Toast initialization now verifies that the Start Menu shortcut points to the current executable and recreates it when the target path is stale.
- **Documentation settings table** — README settings tables now list only currently supported settings and omit unused reserved options.

### Removed

- **Unused reserved settings** — Removed `show_virtual_memory`, reserved tray-icon customization fields, and legacy automatic-optimization interval/threshold placeholders; unknown TOML fields from older configs remain safely ignored.

## [1.0.3] - 2026-07-20

Final changes since [1.0.2].

### Added

- **Smooth animations** — Memory usage rings, cleanup progress bar, and memory text values (used/avail bytes) now transition smoothly between data refreshes instead of jumping. Uses exponential-decay interpolation, reaching 95% of target in ~300 ms.
- **Animation module** — New `src/anim.rs` with reusable `AnimatedValue` interpolator for the entire crate.
- **Smart animation pause** — Animation loop fully stops (zero CPU) when the window is hidden to tray; automatically restarts on restore.

### Fixed

- **Tray menu state** — After closing the window, the tray context menu correctly shows "Show Window" instead of "Hide Window"; after restoring from tray, it correctly shows "Hide Window".
- **Window error paths** — `activate_window` and `open_window` failure paths now correctly reset `window_shown` state.
- **Post-optimization tray sync** — Tray tooltip text updates immediately after cleanup completes, instead of waiting for the next mouse hover.
## [1.0.2] - 2026-07-19

Final changes since [1.0.1].

### Added

- **Run at startup** — New setting to launch silently into the system tray after Windows sign-in, without showing the main window (`src/win32/startup.rs`).
- **Process exclusion picker** — List entries show instance count and memory usage; a placeholder when memory cannot be read.
- **Documentation** — `README_EN.md` (English readme) and `docs/API_COMPARISON_MEMREDUCT.md` (cleanup API comparison with Mem Reduct).

### Changed

- **Modified file cache** — Volume discovery switched from iterating fixed drive letters `A:`–`Z:` to Mount Manager enumeration of `\??\Volume{GUID}` with flush via `NtCreateFile` / `NtFlushBuffersFile`; new `src/win32/volume.rs` centralizes enumeration, flush, and reporting; the step succeeds when at least one volume flushes successfully.
- **Cleanup progress text** — Modified-file step now shows `Volume{GUID}` instead of drive letters (e.g. `C:`).
- **Process exclusion UX** — Selecting a process from the list adds it to the exclusion list immediately; removed the intermediate pending-confirmation state.
- **Release build** — `opt-level` changed from `z` to `s`.
- **App icons** — Updated `App.ico` and `App.png`.
- **Name correction** — Project name corrected from `Memory Cleanr` to `Memory Cleaner`; binary name, package name, window title, tray shortcut, mutex, and all other references updated accordingly.

## [1.0.1] - 2026-07-16

Since [1.0.0]: process exclusion, global cleanup hotkey (default Ctrl+Alt+C) with recording, post-optimization toast, UI i18n, tray spin animation during cleanup, icon cache refresh, debug logging, Windows 10 square-corner theme, and more. See git history.

## [1.0.0] - 2026-07-11

Initial public release: 8 memory cleanup regions, GPUI UI, system tray, administrator elevation, settings persistence.

[Unreleased]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.7...HEAD
[1.0.7]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.6...v1.0.7
[1.0.6]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.5...v1.0.6
[1.0.5]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.4...v1.0.5
[1.0.4]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/Y-ASLant/MemoryCleaner/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/Y-ASLant/MemoryCleaner/releases/tag/v1.0.1
[1.0.0]: https://github.com/Y-ASLant/MemoryCleaner/releases/tag/v1.0.0
