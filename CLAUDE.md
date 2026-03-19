# CLAUDE.md — SuperVox

Privacy-first AI voice diary. Record → transcribe locally → AI insights. No cloud. No accounts.

## Project Structure

```
supervox/
  crates/
    voxkit/              — Voice pipeline: STT, VAD, TTS (Rust crate, 148 tests)
    journal-agent/       — LLM analysis: mood, themes, patterns (TODO)
    journal-cli/         — CLI: record, transcribe, analyze (TODO)
  SuperVox/              — iOS/macOS Swift app (TODO)
  schemas/               — Shared JSON schemas (TODO)
  docs/
    prd.md               — Product requirements
    plan.md              — Development plan
  Makefile
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Voice pipeline | Rust — `voxkit` crate (STT, VAD, TTS, mic capture) |
| LLM agent | Rust — `sgr-agent` (reasoning loop, compaction, sessions) |
| iOS/macOS app | Swift 6, SwiftUI, SwiftData |
| Local STT | WhisperKit (Apple Silicon optimized) |
| Local LLM | mlx-swift (Llama 3.2 1B Q4) |
| Audio | AVFoundation (recording + playback) |
| Payments | StoreKit 2 |
| Analytics | PostHog EU (privacy-respecting) |
| CLI STT | voxkit OpenAiStt (cloud, for dev/testing) |
| CLI LLM | sgr-agent + OpenRouter |

## Tooling

- **Rust:** `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt`
- **Swift:** SwiftLint, swift-format, Swift Testing (@Test)
- **Deps (Rust):** managed via Cargo
- **Deps (Swift):** SPM (Swift Package Manager)

## voxkit (DONE)

Feature-gated voice pipeline crate at `crates/voxkit/`:

| Feature | What |
|---------|------|
| default | AudioChunk, Transcript, SttBackend, VadBackend, VadProcessor, RmsVad |
| `openai` | OpenAiStt (gpt-4o-transcribe) |
| `realtime` | OpenAI Realtime WebSocket STT |
| `silero` | SileroVad (ONNX neural VAD) |
| `openai-tts` | OpenAiTts client |
| `player` | TtsPlayer (sentence split + rodio) |
| `mic` | cpal mic capture with VAD |
| `macos-system-audio` | ScreenCaptureKit via Swift subprocess |
| `macos-mic-mode` | Voice Isolation detection |
| `wav` | WAV encoding |

## Key Principles

- **Privacy is architecture** — no network calls from the app, no cloud, no accounts
- **Offline-first** — works on airplane
- **CLI-first testing** — every feature works in journal-cli before Swift
- **Schemas-first** — define JournalEntry, Summary as schemas before code
- **One pain → one feature → launch**

## Essential Commands

```bash
# voxkit
cd crates/voxkit && cargo test
cd crates/voxkit && cargo test --features "wav"
cd crates/voxkit && cargo clippy -- -D warnings

# journal-cli (when ready)
cargo run -p journal-cli -- transcribe audio.wav
cargo run -p journal-cli -- analyze transcript.json
cargo run -p journal-cli -- listen
```

## Don't

- Send any user data to cloud from the iOS app
- Add accounts, sign-in, CloudKit
- Over-engineer — v1 is record → transcribe → summarize → browse
- Add features not in the PRD

## Do

- TDD for business logic (mood extraction, theme detection, patterns)
- Test on real audio files before Swift integration
- Use voxkit for all audio operations (don't duplicate in Swift)
- Keep journal-cli and iOS app using same domain schemas
