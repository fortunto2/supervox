# Implementation Plan: Audio Import

**Track ID:** audio-import_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [ ] Not Started

## Overview

Add batch file transcription to voxkit STT backends, then wire a `supervox import` CLI command that reads an audio file, transcribes it, saves as a Call, copies the audio, and optionally auto-analyzes.

## Phase 1: Batch File Transcription in voxkit

Add file-level transcription methods to both STT backends.

### Tasks
- [x] Task 1.1: Add `transcribe_file_bytes(&self, bytes: &[u8], filename: &str, mime: &str) -> Result<Transcript, SttError>` to `OpenAiStt` in `crates/voxkit/src/openai_stt.rs`. Sends raw file bytes as multipart upload (no AudioChunk conversion). Returns Transcript with text, segments, duration, language. <!-- sha:cff72da -->
- [x] Task 1.2: Add `transcribe_chunk(ctx: &WhisperContext, audio: &AudioChunk, language: &str) -> Result<Transcript, SttError>` to `WhisperStt` in `crates/voxkit/src/whisper_stt.rs`. Batch-mode transcription of a full AudioChunk (not streaming). Resamples to 16kHz internally if needed. Returns Transcript with text + duration. <!-- sha:cff72da -->
- [x] Task 1.3: Add `read_wav_file(path: &Path) -> Result<AudioChunk, SttError>` helper to `crates/voxkit/src/types.rs`. Uses `hound::WavReader` to read any WAV file into an AudioChunk (mono conversion if stereo, f32 normalization). <!-- sha:cff72da -->
- [x] Task 1.4: Tests for all new functions in their respective modules. Test `transcribe_file_bytes` serialization (unit test for form building), `read_wav_file` with a fixture WAV, `transcribe_chunk` with mock context pattern. <!-- sha:cff72da -->

### Verification
- [x] `cargo test -p voxkit` passes
- [x] `cargo clippy -p voxkit -- -D warnings` clean

## Phase 1: Batch File Transcription in voxkit <!-- checkpoint:cff72da -->

## Phase 2: CLI Import Command

Wire the `supervox import` command that orchestrates file → transcribe → save → analyze.

### Tasks
- [ ] Task 2.1: Add `Import` variant to `Commands` enum in `crates/supervox-tui/src/main.rs` with args: `file: String`, `--no-analyze` flag, `--json` flag, `--language` optional override.
- [ ] Task 2.2: Implement `cmd_import()` async function in `crates/supervox-tui/src/main.rs`:
  - Validate file exists and extension is supported
  - Read file bytes + determine MIME type from extension
  - Create STT backend based on config (OpenAI batch or Whisper)
  - Transcribe: OpenAI uses `transcribe_file_bytes()`, Whisper uses `read_wav_file()` + `transcribe_chunk()`
  - Copy audio file to calls dir with canonical name (`{date}-{id}.{ext}`)
  - Create and save Call (reuse `save_call()`)
  - If `--no-analyze` not set: run `analyze_transcript()` + `save_analysis()` + `update_call_tags()`
  - Print result summary or JSON
- [ ] Task 2.3: Add `mime_for_extension(ext: &str) -> Option<&str>` helper function for mapping file extensions to MIME types (wav, mp3, m4a, flac, ogg, webm).
- [ ] Task 2.4: Add CLI integration tests in `crates/supervox-tui/tests/cli_commands.rs` — test that `supervox import --help` works, test argument parsing for the Import command.

### Verification
- [ ] `cargo build -p supervox-tui` succeeds
- [ ] `supervox import --help` shows correct usage
- [ ] `cargo test -p supervox-tui` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean

## Phase 3: Docs & Cleanup

### Tasks
- [ ] Task 3.1: Update CLAUDE.md with `supervox import` command, supported formats, and usage examples
- [ ] Task 3.2: Remove dead code — unused imports, orphaned files, stale exports

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
Add `supervox import <audio-file>` to transcribe and analyze external audio recordings.

### Key Files
- `crates/voxkit/src/openai_stt.rs` — add `transcribe_file_bytes()` method
- `crates/voxkit/src/whisper_stt.rs` — add batch `transcribe_chunk()` method
- `crates/voxkit/src/types.rs` — add `read_wav_file()` helper
- `crates/supervox-tui/src/main.rs` — add `Import` command + `cmd_import()` handler
- `crates/supervox-tui/tests/cli_commands.rs` — integration tests
- `CLAUDE.md` — document new command

### Decisions Made
- OpenAI batch API receives raw file bytes (no AudioChunk conversion) — supports all formats natively
- Whisper local mode: WAV only — other formats error with clear message suggesting conversion
- Copy (not move) audio file to calls dir — preserves user's original
- Auto-analyze by default — matches live recording flow. `--no-analyze` to skip.
- No new dependencies needed — hound (WAV reading) already in deps, reqwest multipart already used

### Risks
- Large audio files (>1h) may timeout on OpenAI API — mitigated by API's 25MB limit (user gets clear error)
- Stereo WAV needs mono conversion for whisper — handled in `read_wav_file()`

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
