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
    supervox-agent/      — 3-mode agent: tools + storage + config ✓ DONE
    supervox-tui/        — ratatui TUI with mode switching ✓ All 3 modes DONE
  schemas/               — JSON schemas: call, analysis, config
  docs/
    plan/                — Track-based implementation plans
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
mic → MicCapture::start_raw() → resample_to_24k() → OpenAiStreamingStt (WS)
system audio → SystemAudioCapture::start_raw() → resample_to_24k() → separate STT (WS)
                                                                     ↓
                                                     TranscriptEvent (Delta/Final)
                                                                     ↓
                                                     AudioEvent::Transcript{source, is_final}
                                                                     ↓
                                               ┌─ translate (async per final) → left panel
                                               └─ rolling_summary (timer) → right panel
on stop → save call → auto-switch to Analysis mode → trigger LLM analysis
```

### Analysis Mode
- Auto-triggers `analyze_call` (structured LLM output → CallAnalysis)
- Maps results to AnalysisState: summary, action_items, decisions, open_questions, mood, themes
- 'f' key: generate follow-up email via `draft_follow_up`
- 'c' key: copy analysis to clipboard
- 'C' key: copy follow-up to clipboard
- Loading state shown during LLM call

### Agent Mode
- Loads last 10 calls as context on startup
- Streams LLM responses via `Llm::stream_complete`
- System prompt with call history injected
- "Thinking..." state while awaiting response

### Key types
- `AudioSource::Mic | System` — "You:" vs "Them:" labels
- `AudioEvent::Transcript{source, text, is_final}` — delta (dimmed) + final (normal)
- `AudioEvent::Translation{source_id, text}` — shown italic below original
- `AudioEvent::Summary(String)` — replaces right panel content

### Intelligence module (`crates/supervox-tui/src/intelligence.rs`)
- `start_translation_pipeline()` — spawns async task per final transcript
- `start_summary_pipeline()` — timer-based (config `summary_lag_secs`), keeps prior context

### Analysis pipeline (`crates/supervox-tui/src/analysis_pipeline.rs`)
- `analyze_transcript()` — structured LLM output → CallAnalysis
- `draft_follow_up()` — LLM generates follow-up email

### Agent loop (`crates/supervox-tui/src/agent_loop.rs`)
- `build_calls_context()` — loads recent calls as LLM context
- `run_agent_query()` — streaming LLM query with call history

### AppEvent (`crates/supervox-tui/src/app.rs`)
- `AnalysisReady(CallAnalysis)` / `AnalysisError(String)` — analysis results
- `FollowUpReady(String)` / `FollowUpError(String)` — follow-up results
- `AgentChunk(String)` / `AgentDone` / `AgentError(String)` — agent streaming

## Config

```toml
# ~/.supervox/config.toml
my_language = "ru"            # Target language for summaries/translation
stt_backend = "realtime"      # "realtime" | "openai"
llm_model = "gemini-2.5-flash"
summary_lag_secs = 5
capture = "mic+system"        # "mic" | "mic+system"
```

Config loaded at startup via `storage::load_config()`. Default created if missing.
Requires `OPENAI_API_KEY` env var for realtime STT.
System audio requires `system-audio-tap` binary (macOS ScreenCaptureKit).

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
