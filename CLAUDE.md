# CLAUDE.md — SuperVox

Voice-powered productivity TUI. Live call assistant + post-call analysis + agent chat.

## Modes

1. **Live** — real-time subtitles + translation + rolling summary during calls
2. **Analysis** — post-call summary, action items, follow-up draft
3. **Agent** — chat with call history, generate emails, find info across calls
4. **History** — browse past calls with date, duration, transcript preview

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
| Storage | JSON in `~/.supervox/` (calls, analyses, sessions, config) |

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
supervox live                        # live call assistant
supervox analyze <call.json>         # post-call analysis
supervox analyze <call.json> --json  # output CallAnalysis as JSON
supervox agent                       # chat with history
supervox calls                       # list past calls
supervox calls --json                # output calls as JSON
supervox delete <call-id>            # delete a call (with confirmation)
supervox delete <call-id> --force    # delete without confirmation
supervox export <call-id>            # export call as markdown to stdout
supervox export <call-id> -o file.md # export to file
supervox search <query>              # search call transcripts
supervox search <query> --json       # output matches as JSON
supervox insights                    # cross-call patterns + themes + action items
supervox insights --json             # output CallInsights as JSON
supervox stats                       # aggregate call statistics
supervox stats --json                # output CallStats as JSON
supervox tags                        # list all unique tags with counts
supervox tags --json                 # output tag list as JSON
supervox analyze-all                 # batch-analyze all unanalyzed calls
supervox analyze-all --dry-run       # list unanalyzed calls without processing

# Filtering (calls, search, stats, insights)
supervox calls --tag meeting         # filter by tag (repeatable, OR logic)
supervox calls --since 2026-03-01    # from date onward (YYYY-MM-DD)
supervox calls --until 2026-03-15    # up to date
supervox stats --tag budget --since 2026-01-01  # combined filters

# Ollama (local LLM)
supervox --local live                # use Ollama instead of cloud LLM
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
- **Persists analysis** as `{date}-{id}.analysis.json` alongside call file
- On re-open, loads cached analysis instantly (no LLM call)
- Auto-tags `Call.tags` from `CallAnalysis.themes` when analysis completes
- Maps results to AnalysisState: summary, action_items, decisions, open_questions, mood, themes
- 'f' key: generate follow-up email via `draft_follow_up`
- 'c' key: copy analysis to clipboard
- 'C' key: copy follow-up to clipboard
- 'e' key: export call + analysis as markdown to clipboard
- 'h' key: open call history browser
- Loading state shown during LLM call

### Agent Mode
- Loads last 10 calls with analysis summaries + themes as context (richer than transcript preview)
- Streams LLM responses via `Llm::stream_complete`
- System prompt with call history injected
- "Thinking..." state while awaiting response

### History Mode
- Accessible via 'h' from Live (idle) and Analysis modes
- Shows call list: date, duration, first 60 chars of transcript
- ↑/↓/j/k to navigate, Enter to open in Analysis, Esc to go back
- 'd' key: delete selected call (inline y/n confirmation)
- 't' key: open tag filter popup — select/deselect tags to filter call list

### Key types
- `AudioSource::Mic | System` — "You:" (cyan) vs "Them:" (yellow) labels
- `TranscriptLine { source, text, is_translation }` — unified line type for Live mode
- `AudioEvent::Transcript{source, text, is_final}` — delta (dimmed) + final (normal)
- `AudioEvent::Translation{source_id, text}` — shown italic below original
- `AudioEvent::Summary(String)` — replaces right panel content
- `CallInsights { recurring_themes, mood_summary, open_action_items, key_patterns, total_calls, period }` — cross-call analysis
- `CallStats { total_calls, total_duration_secs, analyzed_count, unanalyzed_count, top_themes, calls_this_week, calls_this_month }` — aggregate statistics
- `ThemeCount { theme, count }` — frequency of a recurring theme
- `MoodSummary { positive, neutral, negative, mixed }` — mood distribution

### Help overlay
- `?` key toggles help popup in any mode showing mode-appropriate keybindings
- Any key dismisses the overlay

### Intelligence module (`crates/supervox-tui/src/intelligence.rs`)
- `start_translation_pipeline()` — spawns async task per final transcript
- `start_summary_pipeline()` — timer-based (config `summary_lag_secs`), keeps prior context

### Analysis pipeline (`crates/supervox-tui/src/analysis_pipeline.rs`)
- `analyze_transcript()` — structured LLM output → CallAnalysis
- `draft_follow_up()` — LLM generates follow-up email
- `generate_insights()` — cross-call pattern analysis → CallInsights

### Agent loop (`crates/supervox-tui/src/agent_loop.rs`)
- `build_calls_context()` — loads recent calls as LLM context (enriched with analysis data)
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
llm_backend = "auto"          # "auto" | "ollama"
ollama_model = "llama3.2:3b"  # Model when llm_backend = "ollama"
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
