---
type: methodology
status: active
title: "SuperVox — Development Plan"
created: 2026-03-20
tags: [supervox, plan, voxkit, tui, ratatui, rust]
---

# SuperVox — Development Plan

> Voice-powered productivity TUI. Live call assistant + voice journal + post-call agent.

## Modes

SuperVox works in **3 modes**, selected at startup or switched on the fly:

### Mode 1: Live Call Assistant (primary)

Real-time help during calls when you struggle with English.

```
┌─ SuperVox ─ LIVE ─ call with John (00:12:34) ──────────┐
│ ┌─ Transcript + Translation ────────┐ ┌─ Summary ────┐ │
│ │ 🇬🇧 "We need to align on the     │ │ Rolling:     │ │
│ │    deliverables for Q3..."        │ │ • Discussing │ │
│ │ 🇷🇺 "Нужно согласовать            │ │   Q3 план    │ │
│ │    результаты на Q3..."           │ │ • Он хочет   │ │
│ │                                   │ │   дедлайн    │ │
│ │ 🇬🇧 "Can you commit to Friday?"   │ │   в пятницу  │ │
│ │ 🇷🇺 "Можешь пообещать к пятнице?" │ │ • Бюджет не  │ │
│ │                                   │ │   обсуждали  │ │
│ └───────────────────────────────────┘ └──────────────┘ │
│ Status: Mic ● | STT: realtime | Lang: EN→RU           │
└────────────────────────────────────────────────────────┘
```

**What it does:**
- Captures mic + system audio (your voice + the other person)
- Real-time STT (OpenAI Realtime WebSocket or gpt-4o-transcribe)
- Live translation to your language (configurable: EN→RU, any pair)
- **Rolling summary with ~5s lag** — condensed meaning of what's being said, not word-for-word
- Saves full transcript + translation for post-call analysis

**Why rolling summary matters:** When you're struggling with English, you might miss 3 sentences while parsing one. The rolling summary panel shows "what they mean" in your language, updated every few seconds. Even with lag, this is lifesaving.

### Mode 2: Post-Call Analysis

After a call ends (or on any saved transcript):

```
┌─ SuperVox ─ ANALYSIS ─ call with John (saved) ────────┐
│ ┌─ Summary ─────────────────────────────────────────┐  │
│ │ ## Итоги                                          │  │
│ │ Обсудили Q3 дедлайны. Джон хочет к пятнице.       │  │
│ │ Бюджет пока не согласован — ждёт аппрув от CEO.   │  │
│ │                                                    │  │
│ │ ## Мои действия                                    │  │
│ │ - [ ] Отправить estimate до четверга               │  │
│ │ - [ ] Написать follow-up email                     │  │
│ │ - [ ] Забукать встречу с CEO на бюджет             │  │
│ │                                                    │  │
│ │ ## Follow-up draft                                 │  │
│ │ "Hi John, thanks for the call. Here's my estimate…"│  │
│ └────────────────────────────────────────────────────┘  │
│ [a]gent chat | [e]xport | [c]opy follow-up | [q]uit    │
└────────────────────────────────────────────────────────┘
```

**What it does:**
- Full summary of the call (in your language)
- Action items extracted automatically
- Follow-up email draft (in the call's language — English)
- Key decisions, open questions, deadlines
- Mood/tone analysis (were they happy? frustrated? rushing?)

### Mode 3: Agent Chat

Interactive conversation with your call history:

```
> Что мы договорились с Джоном по бюджету?
AI: На звонке 20 марта Джон сказал что бюджет ждёт аппрув CEO.
    Вы не обсуждали конкретные суммы. На прошлом звонке (15 марта)
    он упоминал $50K как ориентир.

> Напиши фолоап на английском
AI: "Hi John, following up on our call today. I'll have the Q3
    estimate ready by Thursday as discussed. Could you also check
    with your CEO on the budget approval? Happy to schedule a
    separate call if needed. Best, Rustam"
```

**What it does:**
- RAG over all your call transcripts
- Answer questions about past calls
- Generate follow-ups, emails, summaries on demand
- Cross-call analysis (what changed between calls, recurring topics)

---

## Configuration

```toml
# ~/.supervox/config.toml
[general]
my_language = "ru"           # Summary/translation target language
output_language = "ru"       # UI and analysis output language

[live]
stt_backend = "realtime"     # "realtime" (WebSocket) | "openai" (batch)
translate = true             # Live translation overlay
summary_lag_secs = 5         # Rolling summary update interval
capture = "mic+system"       # "mic" | "system" | "mic+system"

[analysis]
llm_model = "gemini-2.5-flash"  # Primary LLM
follow_up_language = "auto"     # "auto" = same as call language

[agent]
llm_model = "gemini-2.5-flash"
search_depth = 10               # How many past calls to search

[storage]
path = "~/.supervox"
```

**Language is configurable, not hardcoded.** Default: summaries in Russian, follow-ups in call language.

---

## Status

| Component | Status | Notes |
|-----------|--------|-------|
| voxkit | **DONE** | 148 tests, 10 modules, all backends |
| schemas | TODO | JSON schemas for calls, analysis |
| supervox-agent | TODO | 3-mode agent: live, analysis, chat |
| supervox-tui | TODO | ratatui TUI with mode switching |

---

## Phase 1: Workspace + agent + schemas

### 1.1 Workspace Cargo.toml (project root)

```toml
[workspace]
members = ["crates/voxkit", "crates/supervox-agent", "crates/supervox-tui"]
resolver = "2"
```

### 1.2 Schemas (`schemas/`)

- `call.json` — id, created_at, duration_secs, participants, language, audio_path, transcript, translation, tags
- `call_analysis.json` — summary, action_items, follow_up_draft, decisions, open_questions, mood, themes
- `config.json` — user preferences (languages, models, backends)

### 1.3 supervox-agent crate (`crates/supervox-agent/`)

```toml
[dependencies]
sgr-agent = { path = "../../../shared/rust-code/crates/sgr-agent", features = ["agent", "session", "genai"] }
voxkit = { path = "../voxkit", features = ["openai", "realtime", "mic", "wav"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
```

**Domain types:**
```rust
pub struct Call { id, created_at, duration, participants, language, transcript, translation }
pub struct CallAnalysis { summary, action_items, follow_up_draft, decisions, open_questions, mood, themes }
pub struct ActionItem { text, assignee, deadline, done }
```

**Agent tools:**
- `translate(text, from, to) → String` — translate chunk
- `rolling_summary(recent_chunks, context) → String` — condensed meaning with lag
- `analyze_call(transcript) → CallAnalysis` — post-call full analysis
- `draft_follow_up(analysis, language) → String` — follow-up email
- `search_calls(query, limit) → Vec<CallMatch>` — RAG search over past calls
- `ask_about_calls(question, context) → String` — answer questions about call history

**Live pipeline (Mode 1):**
```
mic+system → VAD → STT (realtime) → transcript
                                   → translate (parallel) → translation panel
                                   → rolling_summary (every 5s) → summary panel
```

**Tests:** TDD with fixture transcripts for each tool.

---

## Phase 2: TUI app (`crates/supervox-tui/`)

### 2.1 Mode selection

```bash
supervox                    # Interactive: pick mode
supervox live               # Start live call assistant
supervox analyze <call.json> # Analyze saved call
supervox agent              # Chat with call history
supervox calls              # List past calls
```

### 2.2 Live mode TUI (Mode 1)

```
┌─ SuperVox ─ LIVE ─────────────────────────────────────┐
│ ┌─ Transcript ──────────────┐ ┌─ Rolling Summary ───┐ │
│ │ 🇬🇧 original text...       │ │ Краткий смысл:     │ │
│ │ 🇷🇺 перевод...              │ │ • пункт 1          │ │
│ │ (streaming, auto-scroll)  │ │ • пункт 2          │ │
│ │                           │ │ (updates ~5s lag)  │ │
│ └───────────────────────────┘ └────────────────────┘ │
│ Mic ● | System ● | STT: realtime | 00:04:23          │
│ [s]top & analyze | [m]ute | [q]uit                    │
└───────────────────────────────────────────────────────┘
```

**Data flow:**
1. voxkit mic + system audio capture → PCM stream
2. voxkit realtime_stt → live transcript events (delta + final)
3. supervox-agent translate tool → translation line below each utterance
4. supervox-agent rolling_summary (buffered, every N seconds) → right panel
5. On stop → auto-trigger Mode 2 (analysis)

### 2.3 Analysis mode TUI (Mode 2)

After call ends or on `supervox analyze`:
- Full summary + action items + follow-up draft
- All in user's language
- Copy-to-clipboard for follow-up

### 2.4 Agent mode TUI (Mode 3)

sgr-agent-tui ChatPanel — interactive Q&A about call history.
Session persisted in `~/.supervox/sessions/`.

### 2.5 Reuse from sgr-agent-tui

- `ChatPanel` — message display with streaming
- `FocusRing` — keyboard focus management
- Terminal setup (panic handler, alt screen)
- Event loop (crossterm + tokio channels)

---

## Phase 3: Polish

- Audio waveform in live mode
- Speaker diarization labels (You / Them)
- Hotkey to bookmark moments during call
- Export call + analysis to markdown
- Ollama local LLM (--local flag)
- Configurable STT (OpenAI / Deepgram / local Whisper)

---

## Phase 4: iOS app (future)

Same modes, native Swift:
- WhisperKit for local STT
- mlx-swift for local LLM
- Live subtitles as overlay
- TestFlight → App Store

---

## Build order for `/build`

1. Create workspace `Cargo.toml`
2. Create `schemas/` JSON files (call, analysis, config)
3. Create `supervox-agent` crate — domain types + all 6 tools (TDD)
4. Create `supervox-tui` crate — live mode first (record + transcribe + translate + summary)
5. Add analysis mode (auto-trigger after live, or standalone)
6. Add agent mode (chat with history)
7. Integration test: live call → stop → analysis → agent Q&A
8. `make check` passes

Each step = 1 commit. Tests before code for supervox-agent.
