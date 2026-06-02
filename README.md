# Memory Cleanr

Windows 内存清理工具，基于 Rust + GPUI 构建。


## 功能

- 实时监控物理内存和虚拟内存使用情况
- 一键清理多种内存区域（工作集、系统文件缓存、已修改页面、备用列表、合并页面、注册表缓存等）
- 系统托盘常驻，右键菜单快速操作
- 完整模式 / 极简模式切换
- 关闭时隐藏到通知区域
- 窗口置顶、启动时最小化等选项
- 自定义窗口控制按钮（最小化、最大化/还原、关闭）
- 配置文件持久化（`%APPDATA%\MemoryCleaner\settings.toml`）
- 部分功能需要**管理员权限**运行

## 构建

```bash
# 发布构建
make build
# 或
cargo build --release
```

构建产物位于 `target/release/memory-cleanr.exe`。

## 运行

```bash
cargo run
```

## 技术栈

- [Rust](https://www.rust-lang.org/) 1.96+（Edition 2021）
- [GPUI](https://gpui.rs) 0.2 — Zed 编辑器的 GPU 加速 UI 框架
- [gpui-component](https://longbridge.github.io/gpui-component/zh-CN/docs/components/) 0.5 — GPUI 组件库（Button / Checkbox / Switch / GroupBox / TitleBar 等）
- [windows-rs](https://github.com/microsoft/windows-rs) 0.62 — Win32 API 绑定（内存管理、权限提升、窗体控制）

## GPUI 组件库

项目依赖的 [gpui-component](https://longbridge.github.io/gpui-component/gallery/) 0.5 提供了 **59 个 UI 组件**，按用途分类如下：

### 基础输入
Button（按钮）、Checkbox（复选框）、Radio（单选框）、Switch（开关）、Toggle（切换按钮）、Input（输入框）、NumberInput（数字输入框）、OtpInput（OTP 输入框）、Textarea（多行文本框）、Select（下拉选择）、Combobox（组合框）、DropdownButton（下拉按钮）、Clipboard（剪贴板）

### 数据显示
Badge（徽章）、Tag（标签）、Label（标签文本）、Icon（图标）、Image（图片）、Avatar（头像）、Kbd（键盘按键）、Skeleton（骨架屏）、Spinner（加载动画）、DescriptionList（描述列表）

### 容器与布局
Accordion（手风琴）、Collapsible（折叠面板）、Tabs（标签页）、GroupBox（分组框）、Sheet（侧滑面板）、Sidebar（侧边栏）、Resizable（可调整尺寸面板）、Scrollbar（滚动条）、Separator（分隔符）

### 反馈与导航
Alert（警告）、AlertDialog（警告对话框）、Dialog（对话框）、Tooltip（提示）、Popover（弹出框）、HoverCard（悬浮卡片）、Notification（通知）、Menu（菜单）、Pagination（分页）、Breadcrumb（面包屑）

### 数据展示
List（列表）、Table（表格）、DataTable（数据表格）、Tree（树形控件）、VirtualList（虚拟列表）

### 表单
Form（表单）、Settings（设置页）

### 选择器
Calendar（日历）、DatePicker（日期选择器）、ColorPicker（颜色选择器）

### 进度与数值
Progress（进度条）、Slider（滑块）、Rating（评分）、Stepper（步骤条）

### 媒体与编辑器
Editor（代码编辑器，支持 LSP）、Chart（图表）、ThemeColors（主题色）

完整演示见 [gpui-component Gallery](https://longbridge.github.io/gpui-component/gallery/)。

## 项目结构

```
src/
├── main.rs              # 入口点，权限提升、托盘安装、GPUI 窗口初始化
├── app.rs               # 应用状态管理、内存轮询、优化流程调度
├── memory.rs            # 内存数据结构和查询（GlobalMemoryStatusEx）
├── optimize.rs          # 8 种内存区域的优化逻辑（NtSetSystemInformation）
├── privileges.rs        # Windows 特权提升（SeProfileSingleProcess 等）
├── settings.rs          # TOML 配置文件读写（%APPDATA%\MemoryCleaner\settings.toml）
├── tray.rs              # 系统托盘图标和右键菜单管理
├── win32/               # Windows API 底层封装
│   ├── mod.rs           # 模块聚合
│   ├── nt.rs            # NtSetSystemInformation 等 NT 原语
│   └── window.rs        # 窗口控制（置顶、隐藏到托盘等）
└── ui/                  # UI 渲染模块
    ├── mod.rs           # 模块聚合
    ├── memory_card.rs   # 内存信息卡片组件
    ├── settings_page.rs # 设置面板组件
    └── title_bar.rs     # 标题栏组件
```

## 清理区域

| 区域 | 说明 | 需要管理员 |
|------|------|-----------|
| 工作集 | 清空所有进程工作集 | 是 |
| 系统文件缓存 | 释放系统文件缓存 | 是 |
| 已修改页面 | 刷写已修改页面列表 | 是 |
| 备用列表 | 清空备用列表 | 是 |
| 备用列表(低) | 清空低优先级备用列表 | 是 |
| 合并页面 | 释放合并页面 | 是 |
| 注册表缓存 | 释放注册表缓存 | 否 |
| 已修改文件 | 清理已修改文件缓存 | 是 |

## 配置项

配置文件位于 `%APPDATA%\MemoryCleaner\settings.toml`，首次运行自动创建。

| 配置项 | 类型 | 默认值 | 说明 |
|--------|------|--------|------|
| `always_on_top` | bool | `false` | 窗口始终置顶 |
| `close_to_notification_area` | bool | `true` | 点击关闭按钮时隐藏到托盘而非退出 |
| `show_virtual_memory` | bool | `true` | 在界面显示虚拟内存卡片 |
| `start_minimized` | bool | `false` | 启动时直接最小化到托盘 |
| `memory_areas` | u32 | `61` | 需清理的内存区域位掩码（对应 `MemoryAreas` 各标志位之和） |

## 常见问题

**为什么需要管理员权限？**
大部分内存清理操作（工作集、文件缓存等）需要通过 `NtSetSystemInformation` 调用内核接口，这些接口要求管理员权限。程序启动时会自动检测权限，若不足会触发 UAC 提升请求。

**释放内存会导致系统变慢吗？**
Windows 会自动将常用的页面重新加载到内存中，因此清理后短期内可能因缓存重建而略微变慢，但不会造成长期影响。在内存紧张的机器上，主动清理可以释放更多可用内存。

**可以设置为开机自启吗？**
当前版本未内置开机自启功能。你可以将可执行文件的快捷方式放入 `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup` 目录实现开机自启。

## 许可

MIT
