# Implementation Plan: Real-time Live Mode

**Track ID:** realtime-live_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [ ] Not Started

## Overview

Wire voxkit's existing realtime STT (WebSocket) into the TUI Live mode, add parallel translation + rolling summary on a timer, system audio capture, and config loading. All voxkit modules exist — this is integration work, not new audio code.

## Phase 1: Config + Dependencies

Add config file loading and fix dependency features so realtime STT and system audio are available in the TUI crate.

### Tasks

- [x] Task 1.1: Add `realtime` and `macos-system-audio` features to `crates/supervox-tui/Cargo.toml` voxkit dependency. Add `toml` crate (v0.8) for config deserialization. Verify `cargo check -p supervox-tui` passes. <!-- sha:989147f -->
- [x] Task 1.2: Implement `load_config()` and `save_default_config()` in `crates/supervox-agent/src/storage.rs`. Read `~/.supervox/config.toml`, deserialize to `Config` struct. If file missing, write default config. Add `#[derive(Deserialize)]` to `Config` in `types.rs`. TDD: roundtrip test in temp dir, default creation test, partial config (missing fields use defaults). <!-- sha:fd49602 -->
- [x] Task 1.3: Wire config loading at TUI startup in `crates/supervox-tui/src/main.rs`. Pass `Config` to `App::new()`. Store in `App` struct for use by all modes. <!-- sha:c83e65d -->

### Verification

- [x] `cargo check -p supervox-tui` passes with new features
- [x] `cargo test -p supervox-agent` — config roundtrip tests pass
- [x] Default config created in temp dir when missing

## Phase 2: Real-time Audio Pipeline

Rewrite `AudioPipeline` to use streaming STT instead of placeholder, add system audio capture, and route transcript events to TUI.

### Tasks

- [ ] Task 2.1: Rewrite `crates/supervox-tui/src/audio.rs` — replace current `AudioPipeline` with two-stage design: (1) raw audio capture (mic + optional system audio via config `capture` field), (2) `OpenAiStreamingStt::connect()` feeding `TranscriptEvent` back via channel. Use `voxkit::realtime_stt::resample_to_24k()` for sample rate conversion. `AudioEvent` enum: `Transcript { source: AudioSource, text: String, is_final: bool }`, `Level(f32)`, `Stopped { transcript, duration_secs }`. `AudioSource` enum: `Mic`, `System`.
- [ ] Task 2.2: Update `crates/supervox-tui/src/app.rs` `process_audio_event()` — handle new `AudioEvent::Transcript` with `is_final` flag. Delta events: update current line in `LiveState` (dimmed style). Final events: push completed line with source label ("You:" for mic, "Them:" for system). Track elapsed time for call timer display.
- [ ] Task 2.3: Update `crates/supervox-tui/src/modes/live.rs` — add audio level VU meter (`█░░░░`) and call timer (`MM:SS`) to status bar. Show source labels in transcript panel. Style: delta text dimmed, final text normal, translations italic.

### Verification

- [ ] `cargo build -p supervox-tui` compiles with realtime STT
- [ ] Manual test: `cargo run -p supervox-tui -- live` with mic — real-time transcript appears
- [ ] Audio level meter updates in status bar

## Phase 3: Live Intelligence + Auto-flow

Wire translation and rolling summary as async background tasks triggered by transcript events. Add auto-transition to Analysis mode on stop.

### Tasks

- [ ] Task 3.1: Add translation pipeline in `crates/supervox-tui/src/audio.rs` or new `crates/supervox-tui/src/intelligence.rs` — after each `TranscriptEvent::Final`, spawn async task calling `TranslateTool::call()` with `from_lang` = detected language, `to_lang` = config `my_language`. Send result back as `AudioEvent::Translation { source_id, text }`. Update `LiveState::push_translation()`.
- [ ] Task 3.2: Add rolling summary pipeline — spawn a timer task (interval = config `summary_lag_secs`). Each tick: collect recent final transcript chunks since last summary, call `RollingSummaryTool::call()` with prior summary context. Send result as `AudioEvent::Summary(String)`. Update `LiveState::set_summary()`. Keep last 3 summaries visible.
- [ ] Task 3.3: Implement auto-flow on stop — when user presses `s`, save call via `storage::save_call()`, then switch `App::mode` to `Mode::Analysis` with the saved call file path. Trigger `analyze_call` + `draft_follow_up` tools in background, update Analysis mode state when results arrive.
- [ ] Task 3.4: Integration test — verify event routing: mock STT events → check translation and summary are triggered. Test config values are respected (summary_lag_secs, capture mode, my_language).

### Verification

- [ ] Translation appears below each final transcript line
- [ ] Rolling summary updates every N seconds in right panel
- [ ] Stopping live mode auto-transitions to Analysis with results
- [ ] `cargo test --workspace` all pass

## Phase 4: Docs & Cleanup

### Tasks

- [ ] Task 4.1: Update `CLAUDE.md` — document new config file location, realtime STT setup, system audio requirements (`system-audio-tap` binary), new `AudioEvent` types.
- [ ] Task 4.2: Update `README.md` — add config section with example `config.toml`, document `OPENAI_API_KEY` requirement, system audio setup for macOS.
- [ ] Task 4.3: Remove dead code — unused imports in refactored `audio.rs`, any placeholder code from Phase 1 that's been replaced.

### Verification

- [ ] CLAUDE.md reflects current project state
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo test --workspace` all pass

## Final Verification

- [ ] All acceptance criteria from spec met
- [ ] Tests pass
- [ ] Clippy clean
- [ ] Build succeeds
- [ ] Documentation up to date

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
