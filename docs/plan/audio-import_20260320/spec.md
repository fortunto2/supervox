# Specification: Audio Import

**Track ID:** audio-import_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

Add a `supervox import <audio-file>` CLI command that transcribes external audio files (WAV, MP3, M4A, FLAC, OGG, WebM) using the configured STT backend and saves them as regular Call entries. This lets users bring recordings from Zoom, phone calls, voice memos, or any other source into SuperVox for analysis, search, and action tracking — expanding the product beyond self-recorded calls.

Currently, calls only enter the system via live recording. Import removes that bottleneck: any audio file becomes a searchable, analyzable call.

## Acceptance Criteria

- [x] `supervox import <file>` transcribes an audio file and saves it as a Call
- [x] Supports WAV, MP3, M4A, FLAC, OGG, WebM formats via OpenAI batch STT
- [x] Supports WAV format via local Whisper backend (`--local` flag)
- [x] Audio file is copied to `~/.supervox/calls/` alongside the Call JSON
- [x] Auto-analyzes after transcription by default (can skip with `--no-analyze`)
- [x] `--json` flag outputs the resulting Call as JSON (for scripting)
- [x] Imported calls appear in `supervox calls`, `search`, `stats`, `insights` like any other call
- [x] Duration is correctly computed from the audio file
- [x] Progress feedback shown during transcription and analysis

## Dependencies

- `voxkit` openai_stt: needs `transcribe_file_bytes()` method for raw file upload
- `voxkit` whisper_stt: needs batch `transcribe_chunk()` method (non-streaming)
- `hound` crate (already in deps): for reading WAV duration/samples

## Out of Scope

- Batch import of multiple files (user can loop in shell)
- Automatic format conversion (whisper only supports WAV; user converts other formats)
- Speaker diarization on imported files
- TUI mode for import (CLI only)

## Technical Notes

- OpenAI batch STT API (`gpt-4o-transcribe`) natively accepts MP3, M4A, WAV, FLAC, OGG, WebM — no conversion needed
- For Whisper backend: read WAV via `hound::WavReader` → `AudioChunk` → `WhisperStt::transcribe_chunk()`. Other formats not supported in local mode.
- Reuse `save_call()` + `save_analysis()` from storage module
- Copy (not move) the audio file to calls dir to preserve the original
- MIME type detection from file extension (simple map, no magic bytes needed)
