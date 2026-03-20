# Implementation Plan: Live Mode UX — Dual VU, Ducking, Keybindings

**Track ID:** live-ux_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [x] Complete

## Overview

Three ergonomic improvements to live mode: separate VU meters per audio source, mic ducking when system audio is active, and Space/Enter keybindings for one-handed operation.

## Phase 1: Dual Audio Levels

Add per-source audio level events and split VU meter display.

### Tasks

- [x] Task 1.1: Change `AudioEvent::Level(f32)` to `AudioEvent::Level { source: AudioSource, level: f32 }` <!-- sha:c99d5de -->
- [x] Task 1.2: Add system audio level computation in system chunk handler <!-- sha:c99d5de -->
- [x] Task 1.3: Update `LiveState` — replace `audio_level` with `mic_level` + `system_level` <!-- sha:c99d5de -->
- [x] Task 1.4: Update `process_audio_event()` — route level by source <!-- sha:c99d5de -->
- [x] Task 1.5: Dual VU bars in status bar (mic cyan, sys yellow, 6 blocks) <!-- sha:c99d5de -->
- [x] Task 1.6: Update tests for new field names and dual display <!-- sha:c99d5de -->

### Verification

- [x] `cargo test --workspace` passes
- [x] Status bar shows two independent level meters during recording

## Phase 2: Mic Ducking <!-- checkpoint:f664312 -->

Suppress mic STT input when system audio is loud (simple gate, not DSP echo cancellation).

### Tasks

- [x] Task 2.1: Add `ducking_threshold` field to `Config` in `crates/supervox-agent/src/types.rs` — `f32`, default `0.05`, with `default_ducking_threshold()`. Update Config tests. <!-- sha:f664312 -->
- [x] Task 2.2: Add ducking logic to audio pipeline in `crates/supervox-tui/src/audio.rs` — track `system_level: f32` from system chunks. In mic chunk handler: if `system_level > config.ducking_threshold`, skip `stt_tx.send()` (still write to WAV). Add `is_ducked` flag, send `AudioEvent::Ducking(bool)` on state change. <!-- sha:f664312 -->
- [x] Task 2.3: Add `is_ducked` to `LiveState` in `crates/supervox-tui/src/modes/live.rs`. Show ducking indicator in status bar: `🔇` or dimmed mic bar when ducked. <!-- sha:f664312 -->
- [x] Task 2.4: Handle `AudioEvent::Ducking(bool)` in `process_audio_event()` — update `live_state.is_ducked`. <!-- sha:f664312 -->
- [x] Task 2.5: Unit tests for ducking logic — system loud → mic suppressed, system quiet → mic active, threshold boundary. <!-- sha:f664312 -->

### Verification

- [x] Mic STT paused when system audio exceeds threshold
- [x] WAV recording unaffected by ducking (full audio captured)
- [x] Status bar shows ducking state

## Phase 3: Keybindings <!-- checkpoint:1c87fb2 -->

Add Space and Enter as ergonomic alternatives to r/s/b.

### Tasks

- [x] Task 3.1: Update `handle_live_key()` in `crates/supervox-tui/src/app.rs` — add `KeyCode::Char(' ')` as toggle: start recording if idle, stop if recording. Reuse existing `r`/`s` logic (extract to helper functions `start_recording(app)` / `stop_recording(app)`). <!-- sha:e403132 -->
- [x] Task 3.2: Add `KeyCode::Enter` as bookmark alias — same behavior as `b` when recording. <!-- sha:e403132 -->
- [x] Task 3.3: Update help overlay in `crates/supervox-tui/src/help.rs` — show `Space=rec/stop  Enter=mark  b=mark  h=history  ?=help  q=quit`. <!-- sha:e403132 -->
- [x] Task 3.4: Update status bar hints in `crates/supervox-tui/src/modes/live.rs` — change `r=record s=stop` to `Space=rec/stop Enter=mark ?=help q=quit`. <!-- sha:e403132 -->
- [x] Task 3.5: Update CLAUDE.md keybinding docs and config section with `ducking_threshold`. <!-- sha:1c87fb2 -->

### Verification

- [x] Space starts recording from idle, stops when recording
- [x] Enter adds bookmark during recording
- [x] Old keybindings (`r`/`s`/`b`) still work
- [x] Help overlay reflects all bindings
- [x] `cargo test --workspace` green
- [x] `cargo clippy --workspace -- -D warnings` clean

## Final Verification

- [x] All acceptance criteria from spec met
- [x] Tests pass
- [x] Linter clean
- [x] Documentation up to date

## Context Handoff

_Summary for /build to load at session start — keeps context compact._

### Session Intent

Improve live mode UX with dual VU meters, mic ducking, and ergonomic keybindings.

### Key Files

- `crates/supervox-tui/src/audio.rs` — dual level events, ducking logic in pipeline loop
- `crates/supervox-tui/src/modes/live.rs` — dual VU render, ducking indicator, keybinding hints
- `crates/supervox-tui/src/app.rs` — handle new events, Space/Enter keys, extract start/stop helpers
- `crates/supervox-tui/src/help.rs` — update help overlay
- `crates/supervox-agent/src/types.rs` — add ducking_threshold to Config

### Decisions Made

- **Simple gate over DSP AEC:** Full echo cancellation is complex (WebRTC AEC, speex). Gate on STT feed is 90% of the value for 10% of the effort. WAV still records everything.
- **Space as toggle:** More intuitive than separate keys. One-handed operation during calls.
- **Threshold default 0.05:** Conservative — only duck when system audio is clearly present. Avoids false suppression from background noise.
- **Shorter VU bars (6 blocks):** Two bars need to fit in status line alongside STT label and timer.

### Risks

- **Ducking too aggressive:** May miss user speech during system audio. Mitigation: configurable threshold, conservative default.
- **Space conflicts in agent mode:** Agent mode uses text input — Space already types. Mitigation: Space toggle is live-mode only, agent mode unchanged.
- **System audio level noisy:** ScreenCaptureKit may include system sounds (notifications). Mitigation: threshold filters low-energy noise.

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
