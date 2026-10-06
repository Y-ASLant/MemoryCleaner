# MemoryCleaner 开发任务入口（替代原 Makefile）
# 裸 `just` 等价于原来的裸 `make`；`just --list` 查看全部任务
set windows-shell := ["cmd.exe", "/C"]

# 默认任务，与原 Makefile 首目标一致
default: format

format:
    cargo fmt --all

check:
    cargo clippy --release -- -D warnings

test:
    cargo test

# 发布构建前强制先过一遍 Clippy
build: check
    cargo build --release

# cmd 语法，项目仅面向 Windows
clean:
    cargo clean
    if exist dist rmdir /s /q dist
