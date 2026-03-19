.PHONY: test clippy fmt check run

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

## Help
help:
	@grep -E '^##' Makefile | sed 's/## //'
