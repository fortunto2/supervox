# CLAUDE.md — SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

1. **Live** — real-time subtitles + translation + rolling summary during calls
2. **Analysis** — post-call summary, action items, follow-up draft
3. **Agent** — chat with call history, generate emails, find info across calls

## Project Structure

```
supervox/
  crates/
    voxkit/              — Voice pipeline: STT, VAD, TTS (Rust, 148 tests) ✓ DONE
    supervox-agent/      — 3-mode agent: live translate, analysis, chat (TODO)
    supervox-tui/        — ratatui TUI with mode switching (TODO)
  schemas/               — JSON schemas: call, analysis, config (TODO)
  docs/
    plan.md              — Development plan (3 modes, 4 phases)
    prd.md               — Product requirements
  Makefile
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Voice pipeline | `voxkit` (STT, VAD, TTS, mic, system audio capture) |
| LLM agent | `sgr-agent` v0.3 (tool calling, sessions, compaction) |
| TUI | `ratatui` + `sgr-agent-tui` (chat panel, streaming) |
| Real-time STT | voxkit realtime (OpenAI WebSocket) |
| Batch STT | voxkit openai (gpt-4o-transcribe) |
| LLM | sgr-agent genai (Gemini Flash / OpenRouter / Ollama) |
| Audio | voxkit mic + system_audio (cpal, ScreenCaptureKit) |
| Storage | JSON in `~/.supervox/` (calls, sessions, config) |

## Dependencies (workspace)

```toml
sgr-agent = { path = "../../shared/rust-code/crates/sgr-agent", features = ["agent", "session", "genai"] }
sgr-agent-tui = { path = "../../shared/rust-code/crates/sgr-agent-tui" }
```

## Tooling

- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Test:** `cargo test --workspace`
- **Run:** `cargo run -p supervox-tui`

## Essential Commands

```bash
make test      # all workspace tests
make check     # test + clippy + fmt
make run       # launch TUI

# TUI modes
supervox live                    # live call assistant
supervox analyze <call.json>     # post-call analysis
supervox agent                   # chat with history
supervox calls                   # list past calls
```

## Agent Tools (supervox-agent)

| Tool | Mode | What |
|------|------|------|
| `translate` | Live | Translate text chunk (any pair, configurable) |
| `rolling_summary` | Live | Condensed meaning every ~5s (not word-for-word) |
| `analyze_call` | Analysis | Full summary + action items + mood + themes |
| `draft_follow_up` | Analysis | Follow-up email in call language |
| `search_calls` | Agent | RAG search across past call transcripts |
| `ask_about_calls` | Agent | Answer questions about call history |

## Live Mode Pipeline

```
mic + system audio → VAD → STT (realtime WS) → transcript
                                               → translate (parallel) → left panel
                                               → rolling_summary (~5s) → right panel
on stop → auto-trigger analysis mode
```

## Config

```toml
# ~/.supervox/config.toml
[general]
my_language = "ru"            # Target language for summaries/translation
[live]
stt_backend = "realtime"      # "realtime" | "openai"
summary_lag_secs = 5
capture = "mic+system"
[analysis]
llm_model = "gemini-2.5-flash"
follow_up_language = "auto"   # "auto" = same as call language
```

Language is configurable, not hardcoded. Default: summaries in Russian, follow-ups in call language.

## Key Principles

- **Terminal-first** — TUI is the primary interface
- **Configurable languages** — not hardcoded to EN→RU
- **Rolling summary > word-for-word** — meaning matters more than exact words
- **Auto-flow** — live → stop → analysis happens automatically
- **CLI-first testing** — every tool works without TUI
- **Schemas-first** — define Call, Analysis before code

## Don't

- Hardcode languages — use config
- Over-engineer — Phase 1 first, polish later
- Duplicate audio logic — use voxkit for everything
- Skip TDD for agent tools

## Do

- TDD for each agent tool with fixture transcripts
- Use sgr-agent-tui for TUI foundation
- Use sgr-agent Session for call persistence
- Use sgr-agent Compactor for long call transcripts
- Store calls as JSON in `~/.supervox/calls/`
