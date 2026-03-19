.PHONY: test clippy fmt check

## Run all voxkit tests
test:
	cargo test -p voxkit
	cargo test -p voxkit --features "wav"

## Clippy lint
clippy:
	cargo clippy -p voxkit -- -D warnings

## Format check
fmt:
	cargo fmt -p voxkit -- --check

## All checks
check: test clippy fmt

## Format fix
fmt-fix:
	cargo fmt -p voxkit

## Help
help:
	@grep -E '^##' Makefile | sed 's/## //'
