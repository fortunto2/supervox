# Specification: Real-time Live Mode

**Track ID:** realtime-live_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

Upgrade SuperVox Live mode from placeholder audio pipeline to fully streaming real-time experience. Phase 1 built the TUI skeleton with `MicCapture` + VAD but no STT integration — audio is captured but never transcribed. This phase wires voxkit's `OpenAiStreamingStt` (WebSocket) for live transcription, adds parallel translation via `translate` tool, rolling summary on a timer via `rolling_summary` tool, and config file loading from `~/.supervox/config.toml`.

System audio capture (other party's voice) is included as a separate audio source alongside mic, enabling "You:" vs "Them:" transcript labeling.

## Acceptance Criteria

- [ ] Live mode streams real-time transcripts via OpenAI Realtime WebSocket (delta + final events)
- [ ] Each final transcript segment is auto-translated to `my_language` and shown below the original
- [ ] Rolling summary updates every `summary_lag_secs` (default 5s) in the right panel with 3-5 bullet points
- [ ] System audio capture works alongside mic (macOS), transcript lines labeled "You:" / "Them:"
- [ ] Config loaded from `~/.supervox/config.toml` at startup; default config created if missing
- [ ] Audio level meter (VU bar) shown in status bar, updating at ~10Hz
- [ ] Call timer (MM:SS) shown in status bar
- [ ] On stop: auto-transition to Analysis mode with the saved call
- [ ] All existing tests pass + new tests for config loading, audio mixing, event routing

## Dependencies

- voxkit `realtime` feature (already implemented, not wired to supervox-tui)
- voxkit `system_audio` module + `system-audio-tap` macOS helper binary
- `toml` crate for config deserialization
- OpenAI API key (env var `OPENAI_API_KEY`)

## Out of Scope

- Ollama / local LLM support (Phase 4)
- Speaker diarization beyond mic/system source labeling (Phase 4)
- Clipboard integration for Analysis mode (Phase 3)
- Agent mode improvements (Phase 3)
- Audio waveform visualization (Phase 4)

## Technical Notes

- `supervox-tui/Cargo.toml` missing `realtime` feature for voxkit — must add it
- voxkit realtime STT expects 24kHz i16 PCM — mic is 48kHz, need `resample_to_24k()`
- `AudioPipeline` in `crates/supervox-tui/src/audio.rs` needs rewrite: currently only MicCapture with VAD, need raw capture + realtime STT
- Translation and rolling summary are async LLM calls — must not block transcript display
- `system-audio-tap` binary needed for macOS system audio (ScreenCaptureKit) — graceful fallback to mic-only if not available
- Config struct already defined in `supervox-agent/src/types.rs` — reuse it
