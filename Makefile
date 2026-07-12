.PHONY: build check clean fmt

fmt:
	cargo fmt --all

check:
	cargo clippy --release -- -D warnings

build: check
	cargo build --release

clean:
	cargo clean
	-if exist dist rmdir /s /q dist
