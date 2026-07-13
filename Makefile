.PHONY: build check clean format test

format:
	cargo fmt --all

check:
	cargo clippy --release -- -D warnings

test:
	cargo test

build: check
	cargo build --release

clean:
	cargo clean
	-if exist dist rmdir /s /q dist
