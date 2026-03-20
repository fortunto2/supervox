# Specification: Local Whisper STT Backend

**Track ID:** whisper-stt_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

Add local Whisper speech-to-text as an alternative STT backend for SuperVox live mode. Currently, all STT goes through OpenAI's Realtime API (cloud). This feature adds an offline, privacy-first option using whisper.cpp (via `whisper-rs`) with Metal acceleration on Apple Silicon.

The key architectural change is introducing a `StreamingSttBackend` trait that abstracts the streaming STT connection pattern (`connect() → (Sender, Receiver)`), then implementing it for both OpenAI Realtime (existing) and local Whisper (new). The Whisper backend uses voxkit's existing Silero VAD to detect speech segments, then runs batch Whisper transcription on each segment.

## Acceptance Criteria

- [ ] `stt_backend = "whisper"` in config.toml selects local Whisper STT
- [ ] `stt_backend = "realtime"` (default) preserves current OpenAI behavior exactly
- [ ] Live mode works fully offline with Whisper backend (no network calls for STT)
- [ ] Whisper uses Metal acceleration on Apple Silicon (via whisper.cpp)
- [ ] Model auto-downloads on first use (`ggml-base.bin` default, configurable)
- [ ] VAD (Silero) segments speech before sending to Whisper (no continuous decoding)
- [ ] TranscriptEvent::Final emitted per speech segment (deltas optional/stretch)
- [ ] `supervox live` status bar shows active STT backend name
- [ ] Tests cover WhisperStt construction, VAD→Whisper pipeline, config selection

## Dependencies

- `whisper-rs` crate (whisper.cpp Rust bindings, Metal support)
- `ort` already in voxkit deps (for Silero VAD) — reuse
- Whisper GGML model file (~148MB for base, ~75MB for small)

## Out of Scope

- Deepgram backend (separate track)
- Speaker diarization (separate track)
- Audio waveform visualization (separate track)
- Streaming deltas for Whisper (v1 emits Final only per speech segment)
- Custom model training or fine-tuning
- Model management UI in TUI

## Technical Notes

- **Architecture:** New `StreamingSttBackend` trait in voxkit abstracts the `connect() → (Sender<SttInput>, Receiver<TranscriptEvent>)` pattern. Both OpenAI Realtime and Whisper implement it. `audio.rs` selects backend based on `config.stt_backend`.
- **Whisper pipeline:** `mic → Silero VAD (speech segments) → whisper-rs batch transcribe → TranscriptEvent::Final`. Client-side VAD replaces server-side VAD from OpenAI Realtime.
- **Model storage:** `~/.supervox/models/ggml-{size}.bin`. Auto-download from Hugging Face on first use.
- **Feature flag:** `whisper` feature in voxkit/Cargo.toml, gating `whisper-rs` + `silero` deps.
- **No OPENAI_API_KEY needed** when `stt_backend = "whisper"` — removes hard dependency.
- **Config additions:** `whisper_model = "base"` (size: tiny/base/small/medium).
