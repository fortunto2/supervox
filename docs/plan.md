---
type: methodology
status: active
title: "SuperVox — Development Plan"
created: 2026-03-20
tags: [supervox, plan, voxkit, tui, ratatui, rust]
---

# SuperVox — Development Plan

> Privacy-first voice journal. Terminal TUI first, iOS later.

## Status

| Component | Status | Notes |
|-----------|--------|-------|
| voxkit | **DONE** | 148 tests, 10 modules, all backends |
| schemas | TODO | JSON schemas for entries, summaries |
| journal-agent | TODO | sgr-agent tools: summarize, mood, themes, patterns |
| supervox-tui | TODO | ratatui TUI app with mic capture + live transcription |

---

## Phase 1: Workspace + schemas + journal-agent

### 1.1 Workspace Cargo.toml (project root)

```toml
[workspace]
members = ["crates/voxkit", "crates/journal-agent", "crates/supervox-tui"]
resolver = "2"
```

### 1.2 Schemas (`schemas/`)

JSON schema files:

- `journal_entry.json` — id, created_at, audio_path, duration_secs, transcript, summary, mood, mood_confidence, themes, language
- `summary.json` — text, mood, mood_confidence, themes, action_items
- `weekly_insight.json` — period_start, period_end, top_themes, mood_trend, recurring_topics, entry_count

### 1.3 journal-agent crate (`crates/journal-agent/`)

```toml
[dependencies]
sgr-agent = { path = "../../../shared/rust-code/crates/sgr-agent", features = ["agent", "session", "genai"] }
voxkit = { path = "../voxkit" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
```

**Domain types:**
```rust
pub enum Mood { Calm, Anxious, Happy, Sad, Excited, Reflective, Frustrated }
pub struct Summary { text: String, mood: Mood, confidence: f32, themes: Vec<String> }
pub struct JournalEntry { id: String, created_at: DateTime, transcript: String, summary: Option<Summary>, audio_path: Option<String> }
pub struct WeeklyInsight { top_themes: Vec<String>, mood_trend: Vec<(String, Mood)>, entry_count: usize }
```

**Agent tools (sgr-agent ToolDef):**
- `summarize(text) → Summary`
- `extract_mood(text) → { mood, confidence }`
- `extract_themes(text) → Vec<String>`
- `find_patterns(entry_ids) → WeeklyInsight`

**Storage:** `~/.supervox/entries/<date>-<id>.json`

**Tests:** TDD with fixture transcripts. Test each tool independently.

---

## Phase 2: TUI app (`crates/supervox-tui/`)

### 2.1 Dependencies

```toml
[dependencies]
sgr-agent = { path = "../../../shared/rust-code/crates/sgr-agent", features = ["agent", "session", "genai"] }
sgr-agent-tui = { path = "../../../shared/rust-code/crates/sgr-agent-tui" }
voxkit = { path = "../voxkit", features = ["openai", "mic", "wav"] }
journal-agent = { path = "../journal-agent" }
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
```

### 2.2 TUI layout

```
┌─ SuperVox ──────────────────────────────────────┐
│ ┌─ Chat/Transcript ───────────┐ ┌─ Status ────┐ │
│ │ Live transcript stream...   │ │ Mic: ● on   │ │
│ │                             │ │ VAD: speech  │ │
│ │ AI: Mood: reflective (0.85) │ │ Model: flash │ │
│ │ AI: Themes: [work, goals]   │ │ Entries: 42  │ │
│ └─────────────────────────────┘ └──────────────┘ │
│ [r]ecord [s]top [a]nalyze [l]ist [p]atterns [q]  │
└───────────────────────────────────────────────────┘
```

### 2.3 Core flows

**Record flow:**
1. `r` → start mic capture (voxkit MicCapture)
2. VAD detects speech → live audio level in status bar
3. `s` → stop, transcribe (voxkit OpenAiStt)
4. Show transcript in chat panel
5. Auto-save entry to `~/.supervox/entries/`

**Analyze flow:**
1. `a` → run journal-agent on current/last transcript
2. Stream LLM output to chat panel (sgr-agent-tui streaming)
3. Show mood + themes + summary
4. Update entry JSON with analysis

**Browse flow:**
1. `l` → list entries (date, preview, mood)
2. Select entry → show full transcript + summary
3. `p` → weekly patterns across entries

### 2.4 Reuse from sgr-agent-tui

- `ChatPanel` — message display with streaming
- `FocusRing` — keyboard focus management
- `terminal::init_tui_telemetry` — panic handler + alt screen
- Event loop pattern (crossterm events + tokio channels)

### 2.5 CLI mode (non-TUI)

```bash
supervox-tui record --duration 60    # record + transcribe, no TUI
supervox-tui analyze <file.json>     # analyze entry, print result
supervox-tui list                    # list entries
supervox-tui patterns --last 7d      # show patterns
```

Clap subcommands. Same code paths as TUI, just stdout output.

---

## Phase 3: Polish + features

- Audio waveform visualization in TUI
- Playback of recorded audio
- Search entries by text/theme
- Export entries to markdown
- Ollama local LLM support (--local flag)
- Whisper local STT via voxkit silero feature

---

## Phase 4: iOS app (future)

After TUI is stable:
- Swift app using same schemas
- WhisperKit for local STT
- mlx-swift for local LLM
- TestFlight → App Store

---

## Build order for `/build`

1. Create workspace `Cargo.toml`
2. Create `schemas/` JSON files
3. Create `journal-agent` crate — domain types + agent tools (TDD)
4. Create `supervox-tui` crate — TUI app with record + transcribe + analyze
5. Integration test: record → transcribe → analyze → display in TUI
6. `make check` passes

Each step = 1 commit. Tests before code for journal-agent.
