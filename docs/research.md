---
type: research
status: draft
title: "Deep Research — SuperVox"
created: 2026-02-11
tags: [supervox, research, competitive-analysis, voice, ai, privacy, ios, macos]
product_type: ios
---

# Deep Research: SuperVox

## Executive Summary

SuperVox is a privacy-first AI voice diary app for Mac/iOS. Users record voice memos, MLX Whisper transcribes locally, and a local LLM summarizes patterns across entries — daily insights without cloud. The digital journal app market is $5.7B (2025) growing at 11.5% CAGR, with voice/AI as key growth drivers. The competitive landscape has 15+ voice journaling apps, but **none combine fully local transcription + local AI analysis**. This is a clear gap validated by strong HN demand (multiple Show HN posts, 138-point thread on local LLMs for personal notes). Privacy is the #1 user pain point — Rewind.ai and therapy-session-to-OpenAI controversies demonstrate visceral user reactions to cloud audio processing. Recommendation: **GO** — strong privacy differentiation in a growing market with proven demand from technical users.

## 1. Competitive Landscape

| Competitor | URL | Pricing | Key Features | Privacy | Weaknesses |
|-----------|-----|---------|-------------|---------|------------|
| AudioDiary | audiodiary.ai | Free + $6.99/mo + $149 lifetime | Voice → AI summary, goals, mood | Cloud (AWS, GDPR w/ OpenAI) | Cloud transcription, no local option |
| Voicenotes | voicenotes.com | Subscription | Voice notes + AI Q&A, tags | Cloud | No offline, subscription only |
| Rosebud | rosebud.app | Subscription | AI follow-up questions, therapeutic | Cloud | Wirecutter AI pick but cloud-only |
| Day One | dayoneapp.com | Free + Premium | Multimedia diary, rich editor | Cloud sync | Not voice-first, no AI analysis |
| Reflection | reflection.app | Free + $5.75/mo | AI insights, 100+ guides | "Bank-level encryption" | Cloud-based AI |
| Untold | untoldapp.com | Subscription | Voice journal that "writes back" | Cloud | Cloud processing |
| Murmur | App Store | Free + IAP | Voice diary, mood, prompts, photos | Cloud | No AI transcription/analysis |
| VoiceScriber | voicescriber.com | Paid | On-device transcription, Airplane Mode | **Local transcription** | No AI summarization/patterns |
| Hello Diary | App Store | Free + IAP | Voice-first, on-device recognition | **On-device** | Limited AI, basic features |
| Durian Vox | durianvox.com | Subscription | AI-powered audio diary, memories | Cloud | New entrant, cloud-based |
| Voicelore | voicelore.com | ? | Voice-first journal + task manager | ? | Niche, small user base |
| Auro | Product Hunt | Subscription | Voice diary + mood analysis | Cloud | Limited traction |
| Silkwave | Product Hunt | ? | Voice journaling | Cloud | Early stage |
| Talknotes | talknotes.io | Subscription | Voice to structured text | Cloud | Not journaling-focused |
| Penzu | penzu.com | Free + $19.99/yr | Private journal, encryption | Encrypted cloud | Text-only, no voice, dated UI |

### Gap Analysis

**The critical gap: no competitor combines local transcription + local AI analysis.** Two sub-segments exist:

1. **Cloud AI journals** (AudioDiary, Rosebud, Voicenotes, Reflection) — rich AI features but send audio/text to cloud. Users with privacy concerns must opt-out of the best features.

2. **Local transcription tools** (VoiceScriber, Hello Diary) — on-device speech recognition but no AI summarization, pattern detection, or insights. Just raw transcription.

SuperVox fills the gap: **local Whisper transcription + local LLM analysis = full AI journal with zero cloud dependency.**

Additional differentiators:
- **Mac + iOS** (most competitors are mobile-only or web-only)
- **MLX on Apple Silicon** — native performance, no Docker/server
- **Cross-entry pattern analysis** — not just per-entry summaries, but trends over time
- **Obsidian/plain-text export** — own your data forever

## 2. User Pain Points

| Pain Point | Source | URL | Sentiment |
|-----------|--------|-----|-----------||
| "The idea of sending my therapy sessions to OpenAI sounds terrifying" | HN comment (rhcom2) | https://news.ycombinator.com/item?id=39925316 | strongly negative |
| Rewind.ai claimed "local first" but sent audio to cloud for transcription — user backlash | HN thread (164 pts) | https://news.ycombinator.com/item?id=33421751 | negative/betrayed |
| "This is where having our own LLMs and stacks running locally will save us" | HN comment (Erazal) | https://news.ycombinator.com/item?id=39925316 | hopeful/demand |
| "Local means decent accuracy, cloud means better results but breaks the privacy promise" — tradeoff frustration | HN Show HN: WhisperBuddy | https://news.ycombinator.com/item?id=44217185 | mixed |
| Show HN: Local Transcription of Voice Memos for iOS/Mac — uses Ollama + whisper.cpp | HN Show HN (10 pts) | https://news.ycombinator.com/item?id=38879064 | positive/demand |
| "Is there demand for a local-first live transcription tool?" — Yes, for meetings at work, privacy concerns | HN Ask HN | https://news.ycombinator.com/item?id=42103331 | positive/demand |
| "GPT-3 is the best journal I've used" but "no privacy protections around any of it" | HN thread | https://news.ycombinator.com/item?id=34402648 | conflicted |
| Audio journaling builds habit for ADHD users who struggle with writing | YouTube (voice journaling for ADHD) | https://www.youtube.com/watch?v=3SOB-xk1UbI | positive |
| Third-party data breaches doubled to 30% (Verizon 2025 DBIR) — cloud vendors expand attack surface | Industry report | Verizon DBIR 2025 | alarming |

### Top Insights

1. **Privacy is visceral, not rational** — Users don't calculate risk; they feel betrayed when audio goes to cloud. "Terrifying" and "scary Black Mirror" are common reactions. This is an emotional moat: once users trust local-only, switching cost is enormous.

2. **HN community = early adopter market** — Multiple Show HN posts for local voice transcription (Whispering, WhisperBuddy, Local Voice Memos). Each gets engagement. These builders ARE the target users.

3. **Therapy/journaling is the highest-stakes use case** — People journal about mental health, relationships, fears. This is the most private content possible. Cloud = unacceptable for a significant segment.

4. **ADHD users prefer voice over writing** — Audio journaling removes the biggest friction: having to write. Voice-first is accessibility, not just convenience.

5. **"Personal LLM on personal notes" has 138 HN points** — The concept of querying your own diary locally resonates deeply with technical users. SuperVox's pattern analysis is exactly this.

## 3. ASO Analysis (iOS)

| Keyword | Intent | Competition | Relevance |
|---------|--------|------------|-----------|
| voice journal | transactional | high | primary |
| voice diary | transactional | high | primary |
| audio diary | transactional | medium | primary |
| audio journal app | transactional | medium | primary |
| AI journal | transactional | high | primary |
| private journal app | transactional | medium | primary |
| offline diary | transactional | low | secondary |
| voice memo transcription | informational | medium | secondary |
| local transcription app | informational | low | secondary |
| mood tracker voice | transactional | medium | secondary |
| speech to text diary | transactional | low | secondary |
| mental health journal | transactional | high | secondary |
| daily reflection app | transactional | medium | tertiary |
| whisper transcription iOS | informational | low | tertiary |

### ASO Strategy Notes

- Apple App Store now supports **natural language search** (NLP) — optimize for queries like "app to journal by speaking" not just keyword stuffing
- **App Intents framework** — register for Siri: "Hey Siri, start my voice journal" makes the app discoverable through Spotlight and Siri
- **Privacy nutrition labels** — "No Data Collected" is a powerful differentiator in App Store listing. AudioDiary cannot claim this.
- YouTube has strong audio journaling content (multiple videos 10K+ views) — indicates growing mainstream interest beyond tech users

## 4. Naming & Domains

| Name | .com | .app | Trademark | Notes |
|------|------|------|-----------|-------|
| WhisperLog | taken | **AVAIL** | clean (no conflicts) | Evokes Whisper model + journaling. Strong. |
| HushLog | taken | **AVAIL** | clean | Privacy connotation (hush = quiet/secret) |
| QuietInk | taken | **AVAIL** | clean | Voice → text ("ink"), privacy ("quiet") |
| MemoWise | taken | **AVAIL** | clean | Voice memos + wisdom/insights |
| VoxNote | taken | **AVAIL** | clean | Latin "vox" = voice + note |
| HushDiary | taken | **AVAIL** | clean | Direct: private diary |
| ThinkOud | **AVAIL** | **AVAIL** | clean | Think + Out Loud. Both .com and .app free! |
| VoxDiary | taken | **AVAIL** | voxdiary.app only | Already a competitor name concept |
| LokaLog | taken | **AVAIL** | clean | "Loka" = local, subtle nod to privacy |
| Soliloque | taken | **AVAIL** | clean | French "soliloque" = soliloquy, elegant |

*Verified via RDAP + whois (no registrar/NS) + dig (NXDOMAIN) — triple confirmed.*

### Recommended: **WhisperLog**

- Direct reference to Whisper (the transcription model users know)
- "Log" = journaling/diary without being generic
- whisperlog.app available
- Memorable, easy to spell, works in English and internationally
- No trademark conflicts found

**Runner-up: ThinkOud** — both .com and .app available (rare!). "Think out loud" captures the voice-first concept perfectly. Unique, brandable.

## 5. Market Size

- **TAM:** $5.69B (2025) → $13.58B (2033) — global digital journal apps market at 11.5% CAGR ([Business Research Insights](https://www.businessresearchinsights.com/market-reports/journal-app-market-120441), [Straits Research](https://straitsresearch.com/report/digital-journal-apps-market))
- **SAM:** ~$850M — voice/AI journaling segment (~15% of total, fastest-growing subsegment driven by smartphone adoption + AI features)
- **SOM (Year 1):** $200K–$500K — targeting privacy-conscious Apple users (Mac + iOS). Indie pricing at $4.99/mo or $39.99/yr. Need 3,300–8,300 subscribers at $5/mo.

**Growth drivers:**
- AI-powered mood analysis and insights (key market trend)
- Smartphone-based multimedia journaling (voice, images)
- Privacy regulations (GDPR, state privacy laws) pushing demand for local-first solutions
- Apple Silicon making on-device ML practical (MLX, Core ML)

## 6. Recommendation

**Verdict: GO**

**Key advantage:** Only privacy-first AI voice journal with fully local transcription AND analysis. Competitors force a choice: privacy OR AI features. SuperVox offers both.

**Key risk:** On-device LLM quality may not match cloud AI (GPT-4) for summarization and pattern detection. Mitigation: MLX models are improving rapidly; "good enough locally" beats "perfect in the cloud" for privacy-conscious users.

**Why now:**
1. MLX ecosystem maturing — whisper.cpp and local LLMs run well on M-series chips
2. Privacy backlash growing (Verizon DBIR, Rewind.ai controversy, therapy-to-OpenAI reactions)
3. Audio journaling trending on YouTube (mainstream adoption signal)
4. Apple's App Intents + NLP search reward voice-first apps
5. No incumbent owns this niche yet

**Recommended next steps:**
1. `/validate supervox` — STREAM analysis + PRD generation
2. Stack: `ios-swift` (SwiftUI + MLX + whisper.cpp + Core ML)
3. MVP scope: record → local transcribe → local summarize → browse entries
4. Target launch: 4-6 weeks for TestFlight beta

## Sources

1. [HN: Personal LLM on personal notes (138 pts)](https://news.ycombinator.com/item?id=39925316) — Privacy concerns with sending therapy notes to OpenAI
2. [HN: Rewind.ai launch (164 pts)](https://news.ycombinator.com/item?id=33421751) — "Local first" claims vs cloud transcription backlash
3. [HN: Local Transcription of Voice Memos](https://news.ycombinator.com/item?id=38879064) — Show HN using Ollama + whisper.cpp
4. [HN: Whispering - open-source local dictation](https://news.ycombinator.com/item?id=44942731) — Demand for trusted local transcription
5. [HN: WhisperBuddy - privacy-first transcription](https://news.ycombinator.com/item?id=44217185) — Local vs cloud accuracy tradeoff discussion
6. [HN: Local-first live transcription demand](https://news.ycombinator.com/item?id=42103331) — Apple Silicon local transcription for work meetings
7. [HN: Audio journaling app builder](https://news.ycombinator.com/item?id=37045568) — Community interest in voice journaling concept
8. [Business Research Insights: Journal App Market](https://www.businessresearchinsights.com/market-reports/journal-app-market-120441) — $5.69B → $13.58B by 2033
9. [Future Market Insights: Digital Journal Apps](https://www.futuremarketinsights.com/reports/digital-journal-apps-market) — $6.53B → $19.36B by 2035
10. [Straits Research: Digital Journal Apps](https://straitsresearch.com/report/digital-journal-apps-market) — 11.2% CAGR projections
11. [Best Voice Journal Apps 2025](https://journalinginsights.com/best-voice-journal-app/) — Competitor comparison
12. [7 Best Privacy-Focused Voice Recorders](https://voicescriber.com/best-privacy-focused-voice-recorder-apps-offline) — Privacy-first landscape
13. [Best Diary Apps 2026](https://blog.journey.cloud/best-diary-app-2026/) — Market overview with pricing
14. [Reflection: Best Journaling Apps 2026](https://www.reflection.app/blog/best-journaling-apps) — AI journaling trends
15. [AudioDiary](https://audiodiary.ai/) — Key competitor, GDPR-compliant cloud AI
16. [YouTube: Audio journaling for ADHD](https://www.youtube.com/watch?v=3SOB-xk1UbI) — Voice journaling for ADHD users
17. [YouTube: 30 Days of Audio Journaling](https://www.youtube.com/watch?v=PGpqd_yP0RM) — Audio vs writing comparison
18. [MobileAction: ASO keyword research 2026](https://www.mobileaction.co/blog/aso-keyword-research/) — NLP search + App Intents strategy
