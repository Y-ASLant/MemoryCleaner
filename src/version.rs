/// 面向用户的应用名（窗口标题、任务管理器、标题栏）。
pub const APP_NAME: &str = "Memory Cleaner";

/// 不含 `.exe` 的可执行文件基名，用于进程排除匹配。
pub const PROCESS_BASE_NAME: &str = "MemoryCleaner";

/// 来自 `Cargo.toml` 的应用版本。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 设置页版本链接打开的源码仓库。
pub const REPO_URL: &str = "https://github.com/Y-ASLant/MemoryCleaner";
