.PHONY: test clippy fmt check run install release

## Run all tests
test:
	cargo test --workspace

## Clippy lint
clippy:
	cargo clippy --workspace -- -D warnings

## Format check
fmt:
	cargo fmt --all -- --check

## All checks
check: test clippy fmt

## Format fix
fmt-fix:
	cargo fmt --all

## Run TUI
run:
	cargo run -p supervox-tui

## Install to ~/.cargo/bin
install:
	cargo build --release -p supervox-tui
	cp "$$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')/release/supervox" ~/.cargo/bin/supervox
	@echo "Installed supervox to ~/.cargo/bin/supervox"

## Create git tag and push (usage: make release V=0.1.0)
release:
	git tag -a "v$(V)" -m "Release v$(V)"
	git push origin "v$(V)"

## Help
help:
	@grep -E '^##' Makefile | sed 's/## //'
