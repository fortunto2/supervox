# Specification: UX Polish & CLI Improvements

**Track ID:** ux-polish_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox has all 3 modes working (Live, Analysis, Agent) with 110 tests passing. The next priority from the retro is Phase 4 — making the TUI feel production-ready for v0.1.0. This track focuses on the highest-impact UX gaps: keyboard help discoverability, call history browsing within the TUI, speaker label coloring in Live mode, Ollama local LLM config, `--json` CLI flag, and error handling polish.

The codebase already has the foundations: `AudioSource::Mic | System` distinction exists but isn't visually differentiated with colors in the TUI; `sgr-agent` genai already supports Ollama via `LlmConfig::auto()`; the `calls` subcommand outputs a table but has no `--json` flag.

## Acceptance Criteria

- [x] `?` key shows help overlay with all keybindings for the current mode; dismiss with any key
- [x] Call history browser accessible via `h` key from Live/Analysis modes — scrollable list with date, duration, mood, first line; Enter opens Analysis for selected call
- [x] Live mode shows speaker labels ("You:" / "Them:") with distinct colors (cyan for You, yellow for Them)
- [x] `llm_backend` config field supports `"ollama"` value; `--local` CLI flag overrides to Ollama; warns if Ollama not reachable
- [x] `supervox calls --json` outputs JSON array; `supervox analyze file.json --json` outputs JSON
- [x] STT/LLM errors show in TUI status bar with graceful degradation (no panics); retry on transient network errors
- [x] All tests pass, clippy clean, fmt clean
- [x] CLAUDE.md updated with new keybindings and config fields

## Dependencies

- sgr-agent genai (Ollama support already present)
- No new external crates needed (ratatui has overlay/popup support built-in)

## Out of Scope

- Audio waveform visualization (low priority, separate track)
- Notification sounds on call end (separate track)
- v0.1.0 release tagging (after this track ships)
- Export to Markdown/Obsidian (v2 feature)

## Technical Notes

- Help overlay: ratatui `Clear` + `Block` rendered on top of current mode. Per-mode keybinding data as static arrays.
- Call history: reuse `storage::list_calls()` which already returns `Vec<Call>` sorted by date desc. New `CallHistoryState` with cursor + scroll.
- Speaker colors: `LiveState.transcript_lines` is currently `Vec<String>`. Needs to become `Vec<TranscriptLine>` with source field for color routing.
- Ollama: `LlmConfig::auto("llama3.2:3b")` already routes to Ollama in sgr-agent. Need config field + health check (`GET http://localhost:11434/api/tags`).
- --json: `serde_json::to_string_pretty()` on existing `Call` / `CallAnalysis` structs (already derive Serialize).
- Error handling: wrap STT WebSocket and LLM calls with timeout + retry. Show errors via new `AppEvent::StatusMessage(String)`.
