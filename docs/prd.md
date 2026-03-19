---
type: opportunity
status: validated
title: "SuperVox — Privacy-First AI Voice Diary"
created: 2026-02-11
tags: [supervox, prd, ios, macos, privacy, ai, voice, mlx, whisper]
opportunity_score: 9.5
evidence_sources: 18
related:
  - 4-opportunities/supervox/research.md
  - 0-principles/manifest.md
  - 0-principles/stream-framework.md
---

# SuperVox — Product Requirements Document

> Privacy-first AI voice diary for Mac/iOS. Record → transcribe locally → AI insights without cloud.

## Problem

People want to journal but writing is friction. Voice removes that barrier — you can journal while walking, driving, or lying in bed. But existing voice journal apps (AudioDiary, Rosebud, Voicenotes) send audio to cloud for AI processing. For the most private content humans create — thoughts, fears, therapy reflections, relationship struggles — cloud processing feels like a betrayal.

**The gap:** No app combines fully local transcription + local AI analysis. Users must choose: privacy OR AI features. SuperVox offers both.

**Evidence:**
- "Sending therapy sessions to OpenAI sounds terrifying" — HN (138-pt thread)
- Rewind.ai backlash: claimed "local first" but sent audio to cloud (164-pt HN)
- Third-party data breaches doubled to 30% (Verizon 2025 DBIR)
- 15+ voice journal competitors, zero do fully local AI

## Solution

One feature: **speak → local transcription → local AI summary + patterns → browse entries.**

### Core Features (MVP)

1. **Record** — tap to record voice memo (iOS + macOS)
2. **Transcribe** — whisper.cpp on-device (Apple Silicon MLX acceleration)
3. **Summarize** — local LLM extracts key themes, mood, action items per entry
4. **Browse** — timeline of entries with transcripts + summaries
5. **Patterns** — weekly/monthly AI analysis across entries (trends, recurring themes)

### Out of Scope (v1)

- Cloud sync (no iCloud, no accounts)
- Sharing / social features
- Photo/video attachments
- Mood tracking UI (AI detects mood from content, no manual input)
- Apple Watch recording
- Export to Obsidian/Markdown (v2)

## Target Market

**Primary:** Privacy-conscious Apple users who want AI journaling without cloud.
- Developers / HN community (early adopters, vocal advocates)
- Therapy/mental health journalers (highest-stakes privacy)
- ADHD users who prefer voice over writing

**Secondary:** Anyone frustrated with subscription fatigue in journaling apps.

**Market size:**
- TAM: $5.69B (digital journal apps, 2025)
- SAM: ~$850M (voice/AI journaling segment)
- SOM Year 1: $200K–$500K (3,300–8,300 subscribers at $5/mo)

## Tech Stack

**Stack:** `ios-swift`

| Component | Technology |
|-----------|-----------|
| UI | SwiftUI (MVVM) |
| Language | Swift 6 (async/await concurrency) |
| Transcription | whisper.cpp (via WhisperKit or swift binding) |
| ML Acceleration | MLX Swift / CoreML / Metal |
| Local LLM | mlx-swift (Llama 3.2 1B/3B or Phi-3 mini) |
| Storage | SwiftData (local SQLite) |
| Audio | AVFoundation (recording + playback) |
| Payments | StoreKit 2 (subscription) |
| i18n | String Catalog (.xcstrings) |
| Linter | SwiftLint (SPM plugin) |
| Formatter | swift-format |
| Testing | Swift Testing (@Test) + XCTest |
| Analytics | PostHog (posthog-ios SPM, privacy-respecting) |
| CI/CD | GitHub Actions (xcodebuild + fastlane) |
| Distribution | App Store (TestFlight for beta) |

### Key Technical Decisions

- **No CloudKit / No iCloud** — data never leaves device. Period.
- **No accounts / no sign-in** — zero identity, zero tracking
- **whisper.cpp over Apple Speech** — better accuracy for long-form, multilingual support, open-source
- **MLX Swift for LLM** — native Apple Silicon, runs alongside whisper without GPU contention issues
- **SwiftData over Core Data** — modern, Swift-native, simpler API
- **Llama 3.2 1B** — smallest practical model, fits in memory alongside Whisper on 8GB devices

### Model Size Constraints

| Device | RAM | Whisper Model | LLM Model |
|--------|-----|--------------|-----------|
| iPhone 15 (6GB) | Limited | whisper-tiny/base | Llama 3.2 1B (Q4) |
| iPhone 15 Pro (8GB) | Good | whisper-small | Llama 3.2 3B (Q4) |
| Mac M1+ (8GB+) | Plenty | whisper-medium | Llama 3.2 3B or Phi-3 mini |

## Architecture Principles

From `dev-principles.md` + `manifest.md`:

- **SOLID** — separate recording, transcription, LLM, and storage into distinct services
- **DRY** — shared audio processing pipeline for iOS and macOS
- **KISS** — no over-engineering. v1 is record → transcribe → summarize → browse
- **Privacy is architecture** — no network calls, no analytics that leak content, no crash reports with user text
- **Offline-first** — works on airplane, in a village, during outage
- **One pain → one feature → launch** — voice journaling with local AI. Nothing else.
- **Schemas-first** — define JournalEntry, Transcription, Summary as Swift models before UI

## Data Model

```swift
@Model
class JournalEntry {
    var id: UUID
    var createdAt: Date
    var audioPath: String           // relative path to audio file
    var duration: TimeInterval
    var transcription: String?      // whisper output
    var summary: String?            // LLM summary
    var mood: String?               // AI-detected mood
    var themes: [String]            // AI-extracted themes
    var isTranscribed: Bool
    var isSummarized: Bool
}
```

## User Flow (MVP)

```
[Home Screen - Timeline]
    ↓ tap record button
[Recording Screen]
    ↓ tap stop
[Processing: "Transcribing..." → "Summarizing..."]
    ↓ automatic
[Entry Detail: transcript + summary + mood]
    ↓ back
[Home Screen - new entry at top]
```

## Success Metrics

| Metric | Target (Month 1) | Target (Month 6) |
|--------|------------------|-------------------|
| TestFlight signups | 200 | — |
| App Store downloads | — | 5,000 |
| D7 retention | 30% | 40% |
| Entries per active user/week | 3 | 5 |
| Subscriptions | — | 500 ($2,500/mo) |
| App Store rating | — | 4.5+ |
| Crash-free rate | 99% | 99.5% |

**North Star:** Entries per active user per week. If people keep recording, everything else follows.

## Pricing

- **Free tier:** 3 entries/week, basic transcription
- **Pro:** $4.99/mo or $39.99/yr — unlimited entries, AI summaries, pattern analysis, export
- **Lifetime:** $99.99 (anti-subscription-fatigue, aligned with manifesto)

Zero server costs → high margins. Every subscriber is pure profit after Apple's 15-30% cut.

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Local LLM quality insufficient | High | Start with transcription-only (proven). Add LLM incrementally. Users value privacy even with "good enough" AI. |
| Model too large for older iPhones | Medium | Use whisper-tiny + Llama 1B Q4. Degrade gracefully: transcription always works, summaries on capable devices. |
| Apple Intelligence ships competing feature | Medium | Apple won't do "private voice journal app" — they'll add journal prompts to Notes. Our brand = privacy-first AI journal. Different positioning. |
| Low retention (novelty wears off) | Medium | Streaks, weekly pattern emails (local notification), Siri shortcut "start my journal". |
| App Store rejection | Low | No private APIs, standard StoreKit, no network = minimal review friction. |

## Launch Strategy

1. **TestFlight beta** (week 4-6) — share on HN (Show HN), r/privacy, r/journaling
2. **HN Show HN** — "I built a fully local AI voice journal for iOS" — high engagement predicted based on research
3. **Product Hunt** launch — target voice/privacy category
4. **App Store Optimization** — "No Data Collected" privacy label, keywords: "voice journal private offline AI"
5. **YouTube** — demo video showing airplane mode transcription

## Timeline

| Week | Milestone |
|------|-----------|
| 1 | Project setup, SwiftUI skeleton, audio recording |
| 2 | whisper.cpp integration, local transcription working |
| 3 | SwiftData storage, timeline UI, entry detail |
| 4 | MLX LLM integration, summary generation |
| 5 | Polish, StoreKit 2, TestFlight |
| 6 | Beta testing, bug fixes, Show HN |

---

*Generated by /validate. STREAM score: 9.5/10. Research: [research.md](./research.md)*
*Stack: ios-swift. Next: `/scaffold supervox ios-swift`*
