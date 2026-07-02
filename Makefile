.PHONY: build test install-local

build:
	cargo build --release

test:
	cargo test

install-local: build
	mkdir -p $(HOME)/.local/bin
	cp target/release/codex-harness $(HOME)/.local/bin/codex-harness
	chmod 755 $(HOME)/.local/bin/codex-harness
