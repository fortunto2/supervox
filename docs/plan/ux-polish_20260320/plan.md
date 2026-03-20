# Implementation Plan: UX Polish & CLI Improvements

**Track ID:** ux-polish_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [ ] Not Started

## Overview

Four phases: (1) TUI polish — help overlay + speaker colors, (2) call history browser, (3) CLI improvements — --json flag + Ollama config + error handling, (4) docs & cleanup. Each phase independently verifiable.

## Phase 1: TUI Polish — Help Overlay & Speaker Colors <!-- checkpoint:460da7d -->

Add keyboard help overlay and visual speaker differentiation in Live mode.

### Tasks

- [x] Task 1.1: Add `TranscriptLine` struct to `crates/supervox-tui/src/modes/live.rs` — fields: `source: AudioSource`, `text: String`, `is_translation: bool`. Replace `transcript_lines: Vec<String>` and `translation_lines: Vec<String>` with `lines: Vec<TranscriptLine>` in `LiveState`. Update `process_audio_event()` in `crates/supervox-tui/src/app.rs` to populate source field. <!-- sha:7fe7053 -->
- [x] Task 1.2: Color-code speaker labels in Live mode render — in `crates/supervox-tui/src/modes/live.rs` render function, prefix lines with "You: " (cyan) for `AudioSource::Mic` and "Them: " (yellow) for `AudioSource::System`. Translations remain italic cyan but inherit source prefix. <!-- sha:7fe7053 -->
- [x] Task 1.3: Add help overlay system — create `crates/supervox-tui/src/help.rs` with `HelpOverlay` struct. Define per-mode keybinding arrays as `&[(&str, &str)]` (key, description). Render as centered popup using ratatui `Clear` + `Block` + `Paragraph`. Add `show_help: bool` to `App`. <!-- sha:460da7d -->
- [x] Task 1.4: Wire `?` key to toggle help overlay in `crates/supervox-tui/src/app.rs` key handler. Any key dismisses when overlay is shown. Show mode-appropriate bindings (Live/Analysis/Agent). <!-- sha:460da7d -->

### Verification

- [x] `?` shows overlay in each mode with correct bindings; any key dismisses
- [x] Live mode transcript shows "You:" cyan and "Them:" yellow prefixes
- [x] Existing tests pass (`cargo test -p supervox-tui`)

## Phase 2: Call History Browser <!-- checkpoint:64aabc1 -->

Add in-TUI call browsing accessible from Live and Analysis modes.

### Tasks

- [x] Task 2.1: Add `CallHistoryState` to `crates/supervox-tui/src/modes/live.rs` (or new file `crates/supervox-tui/src/modes/history.rs`) — fields: `calls: Vec<Call>`, `cursor: usize`, `scroll_offset: usize`. Methods: `move_up()`, `move_down()`, `selected() -> Option<&Call>`. <!-- sha:64aabc1 -->
- [x] Task 2.2: Render call history list — date, duration (formatted mm:ss), mood emoji (from CallAnalysis if available), first 60 chars of transcript. Highlight selected row. Scrollable with Up/Down/j/k. <!-- sha:64aabc1 -->
- [x] Task 2.3: Wire `h` key in Live (idle) and Analysis modes to open history browser. Load calls via `storage::list_calls()`. Enter on selected call switches to Analysis mode for that call. Esc returns to previous mode. Add `Mode::History { return_to: Box<Mode> }` variant. <!-- sha:64aabc1 -->
- [x] Task 2.4: Add tests for `CallHistoryState` — cursor bounds, empty list, navigation wrapping. <!-- sha:64aabc1 -->

### Verification

- [x] `h` key opens history with past calls listed
- [x] Arrow keys / j/k navigate, Enter opens Analysis for selected call
- [x] Esc returns to previous mode
- [x] Tests pass

## Phase 3: CLI Improvements — JSON Output & Ollama Config

Add `--json` flag for machine-readable output and Ollama as configurable LLM backend.

### Tasks

- [ ] Task 3.1: Add `--json` flag to `Calls` subcommand in `crates/supervox-tui/src/main.rs`. When set, output `serde_json::to_string_pretty(&calls)` instead of table format.
- [ ] Task 3.2: Add `--json` flag to `Analyze` subcommand. Run analysis pipeline (non-TUI), output `CallAnalysis` as JSON to stdout. Requires extracting analysis logic to work without TUI event loop.
- [ ] Task 3.3: Add `llm_backend` field to `Config` in `crates/supervox-agent/src/types.rs` — values: `"auto"` (default, current behavior), `"ollama"`. Add `ollama_model` field (default `"llama3.2:3b"`). Update `schemas/config.json`.
- [ ] Task 3.4: Add `--local` CLI flag in `crates/supervox-tui/src/main.rs` that overrides `llm_backend` to `"ollama"`. When Ollama backend selected, use `LlmConfig::auto(ollama_model)` with Ollama endpoint. Add basic health check (HTTP GET to `localhost:11434`) with warning if unreachable.
- [ ] Task 3.5: Add error resilience — wrap LLM calls in `crates/supervox-tui/src/analysis_pipeline.rs` and `crates/supervox-tui/src/agent_loop.rs` with timeout (30s). On failure, send `AppEvent::StatusError(String)` instead of panic. Show error in status bar (red text) for 5 seconds, then clear. Add `status_message: Option<(String, Instant)>` to `App`.

### Verification

- [ ] `supervox calls --json` outputs valid JSON array
- [ ] `supervox analyze file.json --json` outputs CallAnalysis JSON
- [ ] `llm_backend = "ollama"` in config uses Ollama endpoint
- [ ] `--local` flag overrides to Ollama with health check warning
- [ ] LLM timeout shows error in status bar, no panic

## Phase 4: Docs & Cleanup

### Tasks

- [ ] Task 4.1: Update CLAUDE.md with new keybindings (`?` help, `h` history, `j/k` nav), new config fields (`llm_backend`, `ollama_model`), new CLI flags (`--json`, `--local`), and History mode description.
- [ ] Task 4.2: Update README.md with new features — help overlay, call history browser, Ollama support, JSON output examples.
- [ ] Task 4.3: Remove dead code — unused imports, orphaned files, stale exports. Run `cargo clippy --workspace -- -D warnings` and fix any warnings.

### Verification

- [ ] CLAUDE.md reflects current project state
- [ ] README.md documents all new features
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo test --workspace` passes
- [ ] `cargo fmt --all -- --check` clean

## Final Verification

- [ ] All acceptance criteria from spec met
- [ ] Tests pass
- [ ] Clippy clean
- [ ] Build succeeds
- [ ] Documentation up to date

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
