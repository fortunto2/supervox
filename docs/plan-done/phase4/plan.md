# SuperVox Phase 4 — UX Polish + Local LLM

**Status:** [ ] Not Started
**Track:** phase4

## Context Handoff

**Intent:** Polish the TUI experience and add local LLM option for privacy. Make SuperVox feel production-ready.

**Key files:** all TUI modes, config, agent

**Depends on:** Phase 3 complete

---

- [ ] Task 1.1: Add keyboard shortcut help — `?` key shows help overlay with all bindings per mode. Dismiss with any key.
- [ ] Task 1.2: Add call history browser — `l` key opens scrollable list of past calls with date, duration, mood icon, first line. Enter key opens Analysis mode for selected call. Arrow keys / j/k to navigate.
- [ ] Task 1.3: Add Ollama local LLM support — `--local` flag or `llm_backend = "ollama"` in config. Use sgr-agent genai with Ollama endpoint. Detect if Ollama is running, warn if not.
- [ ] Task 1.4: Add speaker diarization labels — in Live mode, label transcript lines as "You" vs "Caller" based on audio source (mic = you, system = caller). Different colors in TUI.
- [ ] Task 1.5: Add audio waveform visualization — in Live mode, show last 2s of audio as ASCII waveform bar below transcript panel. Update at 15Hz.
- [ ] Task 1.6: Add notification on call end — play system sound or show terminal bell when analysis is complete. Configurable in config.toml.
- [ ] Task 1.7: Add `--json` flag for CLI subcommands — `supervox calls --json`, `supervox analyze file.json --json`. Enables piping to jq or other tools.
- [ ] Task 1.8: Error handling polish — graceful degradation when STT API fails (show error in status bar, don't crash). Retry logic for transient network errors. Timeout handling.
- [ ] Task 1.9: Write README.md — installation, usage, modes, configuration, examples. Screenshots of TUI.
- [ ] Task 1.10: Run full verification: tests, clippy, fmt. Tag release v0.1.0.
