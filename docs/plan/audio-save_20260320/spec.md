# Specification: Audio Recording Persistence

**Track ID:** audio-save_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox records calls and saves transcripts, but the raw audio is discarded after STT processing. For a "voice-powered productivity TUI," this is a fundamental gap — users can't replay calls, verify transcript accuracy, or share recordings.

This track adds incremental WAV audio recording during live capture, stores WAV files alongside call JSON, and provides CLI/TUI playback via the system audio player.

## Acceptance Criteria

- [x] Mic audio is saved as WAV (16-bit PCM mono) during live recording
- [x] WAV file is written incrementally (stream-write, not buffered in memory)
- [x] WAV stored as `{date}-{id}.wav` alongside `{date}-{id}.json` in `~/.supervox/calls/`
- [x] `Call.audio_path` field (optional) links call to its audio file
- [x] Existing calls without audio load correctly (backward compatible)
- [x] `supervox play <call-id>` opens audio in system player
- [x] `supervox calls` shows audio indicator (speaker icon or marker)
- [x] Analysis mode shows audio available status and 'p' to play
- [x] `supervox delete` also removes the associated WAV file
- [x] All new functions have unit tests

## Dependencies

- No new external crates — `hound` already available via voxkit `wav` feature
- Uses existing `AudioPipeline` in `crates/supervox-tui/src/audio.rs`

## Out of Scope

- System audio recording (mic only for v1 — system audio can be added later)
- In-TUI audio playback controls (play/pause/seek) — delegates to system player
- Audio compression (WAV only, no MP3/OGG encoding)
- Audio trimming or editing

## Technical Notes

- `hound::WavWriter` supports incremental writes — create on recording start, write samples per chunk, finalize on stop
- Raw mic audio arrives at ~48kHz (native mic rate). Save at native rate for quality, not resampled 24kHz
- 1-hour call at 48kHz 16-bit mono ≈ 345 MB WAV. Acceptable for local storage; compression is out of scope
- `Call.audio_path` is `Option<String>` with `#[serde(default)]` — backward compatible with existing JSON
- macOS playback via `open <file.wav>` — cross-platform playback can be added later
- `delete_call()` must also clean up `.wav` file — currently only deletes `.json`
