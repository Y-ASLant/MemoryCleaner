.PHONY: build clean

build:
	cargo build --release

clean:
	cargo clean
	-@if [ "$(OS)" = "Windows_NT" ]; then \
		if [ -d dist ]; then rm -rf dist; fi; \
	else \
		rm -rf dist; \
	fi
