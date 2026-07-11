.PHONY: build check clean fmt

fmt:
	cargo fmt --all

check:
	cargo clippy --release -- -D warnings

build: check
	cargo build --release

clean:
	cargo clean
	-@if [ "$(OS)" = "Windows_NT" ]; then \
		if [ -d dist ]; then rm -rf dist; fi; \
	else \
		rm -rf dist; \
	fi
