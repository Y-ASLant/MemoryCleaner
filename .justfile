set windows-shell := ["cmd.exe", "/C"]

# 格式化全部 Rust 代码
format:
    cargo fmt --all

# Clippy 静态检查（警告视为错误）
check:
    cargo clippy --release -- -D warnings

# 运行单元测试与集成测试
test:
    cargo test

# 发布构建前强制先过一遍 Clippy
build: check
    cargo build --release

# 清理构建产物（target 与 dist）
clean:
    cargo clean
    if exist dist rmdir /s /q dist
