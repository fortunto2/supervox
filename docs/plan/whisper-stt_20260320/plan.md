# Implementation Plan: Local Whisper STT Backend

**Track ID:** whisper-stt_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [ ] Not Started

## Overview

Add a `StreamingSttBackend` trait to voxkit, implement local Whisper backend using whisper-rs + Silero VAD, and wire config-based backend selection in supervox-tui. Three phases: trait abstraction, Whisper implementation, TUI wiring.

## Phase 1: Streaming STT Trait + Refactor <!-- checkpoint:8083c4e -->

Extract the streaming STT interface into a trait so audio.rs can select backends dynamically.

### Tasks

- [x] Task 1.1: Add `StreamingSttBackend` trait to `crates/voxkit/src/stt.rs` — `async fn connect(config) -> Result<(Sender<SttInput>, Receiver<TranscriptEvent>), SttStreamError>`. Move `SttInput` from `realtime_stt.rs` to `stt.rs` (shared type). Add trait test. <!-- sha:dc328d3 -->
- [x] Task 1.2: Implement `StreamingSttBackend` for `OpenAiStreamingStt` in `crates/voxkit/src/realtime_stt.rs` — wrap existing `connect()` in trait impl. No behavior change. <!-- sha:06faab9 -->
- [x] Task 1.3: Refactor `crates/supervox-tui/src/audio.rs` — replace direct `OpenAiStreamingStt::connect()` calls with `StreamingSttBackend::connect()`. Add `create_stt_backend(config) -> Box<dyn StreamingSttBackend>` factory function. Use `config.stt_backend` to select. <!-- sha:06faab9 -->
- [x] Task 1.4: Add `whisper_model` field to `Config` in `crates/supervox-agent/src/types.rs` — default `"base"`, with `default_whisper_model()` function. Update Config tests. <!-- sha:1c0f0be -->

### Verification

- [x] `cargo test --workspace` passes — no behavior change for existing OpenAI path
- [x] `stt_backend = "realtime"` still works identically

## Phase 2: Whisper Backend

Implement local Whisper STT in voxkit using whisper-rs + Silero VAD.

### Tasks

- [ ] Task 2.1: Add `whisper` feature to `crates/voxkit/Cargo.toml` — depends on `whisper-rs` + enables `silero` feature. Feature-gate new module.
- [ ] Task 2.2: Create `crates/voxkit/src/whisper_stt.rs` — `WhisperStt` struct with model loading, `transcribe_segment(audio: &[f32], sample_rate: u32) -> Result<String, SttError>` method. Use whisper-rs `WhisperContext` + `WhisperParams`. Test with synthetic audio fixture.
- [ ] Task 2.3: Add model download utility in `whisper_stt.rs` — `ensure_model(model_size: &str, models_dir: &Path) -> Result<PathBuf>`. Downloads GGML model from Hugging Face if not present. Supports tiny/base/small/medium sizes.
- [ ] Task 2.4: Implement `StreamingSttBackend` for `WhisperStt` — spawn task that: receives `SttInput::Audio` → feeds Silero VAD → on speech end → run whisper transcribe → emit `TranscriptEvent::Final`. Handle `SttInput::Close` for shutdown.
- [ ] Task 2.5: Add integration test in `crates/voxkit/tests/` — test WhisperStt with a short WAV fixture (create 1-second silence WAV). Verify pipeline connects, processes audio, emits events.

### Verification

- [ ] `cargo test --workspace --features whisper` passes
- [ ] WhisperStt can be constructed and connected without panics
- [ ] Model download function works (test with tiny model)

## Phase 3: Config Wiring + TUI Integration

Wire backend selection into the TUI pipeline and update status display.

### Tasks

- [ ] Task 3.1: Update `AudioPipeline::start()` in `crates/supervox-tui/src/audio.rs` — use `create_stt_backend()` factory. When `stt_backend = "whisper"`, don't require `OPENAI_API_KEY`. Pass `whisper_model` and models dir to WhisperStt.
- [ ] Task 3.2: Update live mode status bar in `crates/supervox-tui/src/modes/live.rs` — show "STT: whisper (base)" or "STT: realtime" based on active backend.
- [ ] Task 3.3: Add voxkit `whisper` feature to `crates/supervox-agent/Cargo.toml` and `crates/supervox-tui/Cargo.toml` — enable by default so `cargo run` includes Whisper support.
- [ ] Task 3.4: Update CLAUDE.md config section — document `stt_backend = "whisper"`, `whisper_model` config, and `supervox --local live` behavior (uses both local LLM and local STT).

### Verification

- [ ] `stt_backend = "whisper"` in config.toml → live mode uses local Whisper
- [ ] `stt_backend = "realtime"` → unchanged OpenAI behavior
- [ ] Status bar shows correct backend name
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace -- -D warnings` clean

## Phase 4: Docs & Cleanup

### Tasks

- [ ] Task 4.1: Update CLAUDE.md with new architecture (StreamingSttBackend trait, whisper feature flag, model storage)
- [ ] Task 4.2: Remove dead code — unused imports, stale comments, orphaned functions from refactor
- [ ] Task 4.3: Update config.toml example in CLAUDE.md and README

### Verification

- [ ] CLAUDE.md reflects current project state
- [ ] Linter clean, tests pass

## Final Verification

- [ ] All acceptance criteria from spec met
- [ ] Tests pass
- [ ] Linter clean
- [ ] Build succeeds
- [ ] Documentation up to date

## Context Handoff

_Summary for /build to load at session start — keeps context compact._

### Session Intent

Add local Whisper STT as a privacy-first alternative to OpenAI Realtime, with config-based backend selection.

### Key Files

- `crates/voxkit/src/stt.rs` — add StreamingSttBackend trait, move SttInput here
- `crates/voxkit/src/realtime_stt.rs` — implement trait for OpenAiStreamingStt
- `crates/voxkit/src/whisper_stt.rs` — NEW: WhisperStt + model download + VAD pipeline
- `crates/voxkit/src/lib.rs` — add whisper_stt module (feature-gated)
- `crates/voxkit/Cargo.toml` — add whisper feature + whisper-rs dep
- `crates/supervox-agent/src/types.rs` — add whisper_model to Config
- `crates/supervox-tui/src/audio.rs` — refactor to use trait, add factory function
- `crates/supervox-tui/src/modes/live.rs` — update status bar display

### Decisions Made

- **whisper-rs over candle-whisper:** whisper-rs wraps whisper.cpp which has mature Metal acceleration on Apple Silicon. Candle is pure Rust but heavier and less optimized for macOS.
- **Silero VAD for speech segmentation:** Already in voxkit (ort dep). Replaces OpenAI's server-side VAD for local pipeline.
- **Final-only events in v1:** Local Whisper runs batch on speech segments. No streaming deltas — keeps implementation simple. Can add chunked partial results later.
- **Model auto-download:** Store in `~/.supervox/models/`. Download from Hugging Face GGML releases on first use. Default to base model (~148MB, good balance of speed/accuracy).
- **Feature flag approach:** `whisper` feature gates all Whisper code. Default-enabled in supervox-tui Cargo.toml so release builds include it.

### Risks

- **whisper-rs compilation:** Requires CMake + C++ compiler for whisper.cpp. May fail on some systems. Mitigation: feature-gated, so can be disabled.
- **Model download size:** ~148MB for base model. First-use latency. Mitigation: show progress in status bar, support smaller models (tiny = ~75MB).
- **VAD accuracy:** Silero VAD needs tuning for different environments. Mitigation: use conservative defaults, expose config params later.
- **Latency:** Batch Whisper on speech segments may have higher latency than streaming OpenAI. Mitigation: use smaller model + Metal acceleration. Accept tradeoff for privacy.

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
