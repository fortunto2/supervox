# CLAUDE.md — SuperVox

Privacy-first voice journal TUI. Record → transcribe → AI insights. Terminal-first.

## Project Structure

```
supervox/
  crates/
    voxkit/              — Voice pipeline: STT, VAD, TTS (Rust, 148 tests) ✓ DONE
    journal-agent/       — LLM analysis: mood, themes, patterns (TODO)
    supervox-tui/        — TUI app: ratatui + sgr-agent-tui (TODO)
  schemas/               — Shared JSON schemas (TODO)
  docs/
    prd.md               — Product requirements
    plan.md              — Development plan
  Makefile
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Voice pipeline | Rust — `voxkit` (STT, VAD, TTS, mic, system audio) |
| LLM agent | Rust — `sgr-agent` v0.3 (tool calling, sessions, compaction) |
| TUI framework | Rust — `ratatui` + `sgr-agent-tui` (chat panel, streaming) |
| STT | voxkit OpenAiStt (cloud) or Realtime WS |
| LLM | sgr-agent genai (Gemini / OpenRouter / Ollama) |
| Audio capture | voxkit mic (cpal, cross-platform) |
| Storage | JSON files in `~/.supervox/` (entries, sessions) |

## Dependencies (workspace)

```toml
# External crates from ~/startups/shared/rust-code/
sgr-agent = { path = "../../shared/rust-code/crates/sgr-agent", features = ["agent", "session", "genai"] }
sgr-agent-tui = { path = "../../shared/rust-code/crates/sgr-agent-tui" }
```

## Tooling

- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Test:** `cargo test --workspace`
- **Run TUI:** `cargo run -p supervox-tui`

## voxkit (DONE)

Feature-gated voice pipeline at `crates/voxkit/`:

| Feature | What |
|---------|------|
| default | AudioChunk, Transcript, SttBackend, VadBackend, VadProcessor, RmsVad |
| `openai` | OpenAiStt (gpt-4o-transcribe) |
| `realtime` | OpenAI Realtime WebSocket STT |
| `silero` | SileroVad (ONNX neural VAD) |
| `openai-tts` | OpenAiTts client |
| `player` | TtsPlayer (sentence split + rodio) |
| `mic` | cpal mic capture with VAD |
| `macos-system-audio` | ScreenCaptureKit capture |
| `macos-mic-mode` | Voice Isolation detection |
| `wav` | WAV encoding |

## TUI Architecture (target)

Based on sgr-agent-tui (ratatui):

```
┌─ SuperVox ──────────────────────────────────────┐
│ ┌─ Chat ──────────────────────┐ ┌─ Status ────┐ │
│ │ [recording 00:45]           │ │ Mic: ●      │ │
│ │                             │ │ VAD: speech  │ │
│ │ Transcript:                 │ │ STT: openai  │ │
│ │ "Today I was thinking..."   │ │ LLM: gemini  │ │
│ │                             │ │              │ │
│ │ AI Summary:                 │ │ Entries: 42  │ │
│ │ Mood: reflective (0.85)     │ │ This week: 5 │ │
│ │ Themes: [work, goals]       │ └──────────────┘ │
│ │                             │                   │
│ └─────────────────────────────┘                   │
│ [r]ecord [s]top [a]nalyze [p]atterns [q]uit       │
└───────────────────────────────────────────────────┘
```

Key bindings:
- `r` — start recording (mic → VAD → buffer)
- `s` — stop recording, transcribe
- `a` — analyze transcript (LLM summary + mood + themes)
- `p` — show weekly patterns
- `l` — list recent entries
- `q` — quit

## Key Principles

- **Terminal-first** — TUI is the primary interface, iOS comes later
- **Privacy-first** — transcription can be local (Whisper) or cloud (OpenAI), user chooses
- **Offline-first** — entries stored locally, LLM analysis optional
- **CLI-first testing** — every feature works without TUI too
- **Schemas-first** — define JournalEntry, Summary before code

## Essential Commands

```bash
make test                    # all workspace tests
make check                   # test + clippy + fmt
cargo run -p supervox-tui    # launch TUI
```

## Don't

- Over-engineer — v1 is record → transcribe → summarize → browse
- Add features not in docs/plan.md Phase 1
- Duplicate audio logic — use voxkit for everything
- Add networking beyond STT/LLM API calls

## Do

- TDD for journal-agent tools (mood, themes, patterns)
- Use sgr-agent-tui for TUI foundation (don't reinvent chat panel)
- Use sgr-agent Session for entry persistence
- Store entries as JSON in `~/.supervox/entries/`
