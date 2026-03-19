---
type: methodology
status: active
title: "SuperVox — Development Plan"
created: 2026-03-20
tags: [supervox, plan, voxkit, swift, rust]
---

# SuperVox — Development Plan

> Privacy-first AI voice diary. Record → transcribe locally → AI insights.

## Status

| Component | Status | Location |
|-----------|--------|----------|
| voxkit (voice pipeline) | **DONE** | `crates/voxkit/` (148 tests, 10 modules) |
| schemas | TODO | `schemas/` |
| journal-agent | TODO | `crates/journal-agent/` |
| journal-cli | TODO | `crates/journal-cli/` |
| Swift app | TODO | `SuperVox/` |
| Launch | TODO | — |

---

## Phase 1: Schemas + journal-agent + CLI (Rust)

### 1.1 Shared schemas (`schemas/`)

Create JSON schemas consumed by both Rust CLI and Swift app:

```
schemas/
  journal_entry.json     — id, created_at, audio_path, duration, transcript, summary, mood, themes
  summary.json           — text, mood, mood_confidence, themes, action_items
  weekly_insight.json    — period, top_themes, mood_trend, recurring_topics, entry_count
```

### 1.2 journal-agent crate (`crates/journal-agent/`)

Rust crate using sgr-agent for LLM analysis of journal transcripts.

**Dependencies:**
```toml
sgr-agent = { version = "0.3", features = ["agent", "session", "genai"] }
voxkit = { path = "../voxkit" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**Tools (sgr-agent ToolDef):**
- `summarize(text) → Summary` — 2-3 sentence summary of journal entry
- `extract_mood(text) → Mood` — mood enum (calm, anxious, happy, sad, excited, reflective, frustrated) + confidence 0..1
- `extract_themes(text) → Vec<Theme>` — topic tags extracted from content
- `find_patterns(summaries) → WeeklyInsight` — trends across multiple entries

**LLM config:**
- Primary: OpenRouter (gemini-2.5-flash) — fast, cheap
- Fallback: Ollama local (llama3.2)
- Compaction: `Compactor::new(budget).with_prompt(JOURNAL_COMPACTION_PROMPT)`

**Tests:** TDD — write tests with fixture transcripts before implementing tools.

### 1.3 journal-cli (`crates/journal-cli/`)

CLI binary that exercises the full pipeline:

```bash
# Core commands
journal-cli transcribe <audio.wav>              # voxkit STT → transcript.json
journal-cli analyze <transcript.json>            # journal-agent → summary.json
journal-cli record --duration 300                # voxkit mic → WAV file
journal-cli listen                               # VAD → auto-segment → transcribe → analyze

# Analysis
journal-cli patterns --last 7d                   # weekly insight from recent entries
journal-cli entries                              # list all entries
journal-cli show <entry-id>                      # show entry detail

# Pipeline
journal-cli pipeline <audio.wav>                 # transcribe + analyze in one step
```

**Storage:** JSON files in `~/.supervox/entries/` (one JSON per entry).

**Tests:** Integration tests with fixture audio/transcripts.

### 1.4 Workspace Cargo.toml

Create workspace at project root:
```toml
[workspace]
members = ["crates/voxkit", "crates/journal-agent", "crates/journal-cli"]
```

---

## Phase 2: Swift app — record + transcribe (iOS/macOS)

### 2.1 Xcode project (`SuperVox/`)

- SwiftUI + SwiftData
- Target: iOS 17+ / macOS 14+
- SPM packages: WhisperKit, posthog-ios
- project.yml (xcodegen) or manual Xcode project

### 2.2 Data model

```swift
@Model class JournalEntry {
    var id: UUID
    var createdAt: Date
    var audioPath: String
    var duration: TimeInterval
    var transcription: String?
    var summary: String?
    var mood: String?
    var moodConfidence: Double?
    var themes: [String]
    var isTranscribed: Bool
    var isSummarized: Bool
}
```

### 2.3 Services

- **AudioService** — AVAudioRecorder (start/stop), AVAudioPlayer (playback)
- **TranscriptionService** — WhisperKit (download model on first use, ~75MB)
- **AnalysisService** — mlx-swift Llama 3.2 1B Q4 (download on first "analyze")

### 2.4 Screens (MVP)

1. **TimelineView** — entry list (date, transcript preview, mood icon)
2. **RecordingView** — record button, waveform, timer
3. **EntryDetailView** — transcript + summary + themes
4. **SettingsView** — model size, language, storage info

**Deliverable:** Record → local transcribe → view transcript.

---

## Phase 3: AI analysis in Swift app

### 3.1 Local LLM (mlx-swift)

- Llama 3.2 1B Q4 (~700MB download on first use)
- Structured output: Summary JSON schema
- Background processing (don't block UI)

### 3.2 Pattern analysis

- Weekly digest: top themes, mood trend, recurring topics
- Stored as `WeeklyInsight` in SwiftData
- Computed on-demand

### 3.3 Polish

- Skeleton loading states
- Retry on failure
- Audio waveform visualization

---

## Phase 4: Launch prep

### 4.1 Paywall (StoreKit 2)

- Free: 3 entries/week, transcription only
- Pro ($4.99/mo, $39.99/yr): unlimited + AI + patterns
- Lifetime ($99.99)

### 4.2 Onboarding

- 3 screens: privacy pitch, record demo, model download

### 4.3 App Store

- Privacy label: "No Data Collected"
- Screenshots, description (EN + RU)
- Privacy policy + terms

---

## Phase 5: Beta + launch

1. TestFlight internal (5 testers, 3 days)
2. TestFlight public (200 target signups)
3. Show HN: "SuperVox — Private voice diary with local AI (no cloud)"
4. Product Hunt (week after HN)
5. Post-launch: monitor crashes, fix top 3 bugs, respond to reviews

---

## Success Metrics

| Metric | Target |
|--------|--------|
| TestFlight signups | 200 |
| App Store downloads (month 1) | 1,000 |
| D7 retention | 30% |
| Entries/active user/week | 3 |
| Subscriptions (month 6) | 500 ($2,500/mo) |

**North star:** entries per active user per week.

---

## Execution for `/build`

Start with **Phase 1** (all Rust, CLI-first):
1. Create workspace Cargo.toml
2. Create `schemas/` JSON files
3. Create `journal-agent` crate with TDD
4. Create `journal-cli` crate
5. Integration test: `journal-cli pipeline test.wav`

Then **Phase 2** (Swift app).
