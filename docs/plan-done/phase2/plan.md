# SuperVox Phase 2 — Polish + Real-time

**Status:** [ ] Not Started
**Track:** phase2

## Context Handoff

**Intent:** Add real-time STT (WebSocket streaming), live translation overlay, and rolling summary to Live mode. Phase 1 used batch STT — now upgrade to streaming for real-time subtitles.

**Key files:** `crates/supervox-tui/src/modes/live.rs`, `crates/voxkit/src/realtime_stt.rs`, `crates/supervox-agent/src/tools/`

**Depends on:** Phase 1 complete (workspace, agent, TUI foundation)

---

- [ ] Task 1.1: Switch Live mode STT from batch OpenAiStt to streaming voxkit realtime_stt (WebSocket). Wire TranscriptEvent::Delta for live partial transcripts, ::Final for completed turns. Show deltas in transcript panel with dimmed style, finals in bold.
- [ ] Task 1.2: Add live translation — after each Final transcript event, call supervox-agent `translate` tool. Display translation below original text with 🇷🇺 prefix. Run translation async (don't block transcript display).
- [ ] Task 1.3: Add rolling summary panel — every `summary_lag_secs` (default 5s), collect recent Final transcripts, call `rolling_summary` tool. Display 3-5 bullet points in right panel. Keep last 3 summaries visible (scrollable).
- [ ] Task 1.4: Add system audio capture — integrate voxkit `system_audio` (macOS) alongside mic. Capture both sides of the call. Label transcript lines: "You:" vs "Them:" based on audio source.
- [ ] Task 1.5: Add audio level meter in status bar — show VU meter (█░░░░) based on RMS from mic audio chunks. Update at 10Hz.
- [ ] Task 1.6: Add call timer — show elapsed time (HH:MM:SS) in status bar, start on record, stop on stop.
- [ ] Task 1.7: Config file loading — read `~/.supervox/config.toml` at startup. Apply settings: my_language, stt_backend, summary_lag_secs, capture mode. Create default config if not exists.
- [ ] Task 1.8: Run full verification: `cargo test --workspace`, clippy, fmt. Fix issues.
