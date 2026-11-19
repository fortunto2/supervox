.PHONY: test clippy fmt check run install release hooks diarizer

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

## Install git pre-commit hooks
hooks:
	git config core.hooksPath .githooks
	@echo "Pre-commit hooks activated (.githooks/pre-commit)"

## Help
help:
	@grep -E '^##' Makefile | sed 's/## //'

## Build the offline diarizer sidecar (FluidAudio CLI, Neural Engine) into ~/.supervox/bin.
## Needed for `diarizer = "fluid"` (default); without it the live CAM++ labels stay.
FLUIDAUDIO_REV ?= 16780f04d2ece24f67d0fc00efccd0296491c1a8
diarizer:
	test -d _work/fa || git clone https://github.com/FluidInference/FluidAudio.git _work/fa
	git -C _work/fa checkout $(FLUIDAUDIO_REV)
	cd _work/fa && swift build -c release --product fluidaudiocli
	mkdir -p ~/.supervox/bin
	cp _work/fa/.build/out/Products/Release/fluidaudiocli ~/.supervox/bin/fluidaudiocli
	@echo "Sidecar installed. First run compiles the CoreML models (~25 s), then 15 s per 48 min of audio."
