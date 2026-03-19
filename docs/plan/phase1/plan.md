# SuperVox Phase 1 — Implementation Plan

**Status:** [x] Complete
**Track:** phase1
**Estimated tasks:** 17

## Context Handoff

**Intent:** Build SuperVox — a voice-powered productivity TUI with 3 modes: live call assistant (real-time subtitles + translation + rolling summary), post-call analysis (summary + action items + follow-up draft), and agent chat (Q&A over call history).

**Key files:**
- `CLAUDE.md` — project instructions, tech stack, architecture
- `docs/plan.md` — full plan with mode descriptions and data flows
- `crates/voxkit/` — voice pipeline (DONE, 148 tests: STT, VAD, TTS, mic, system audio)

**Dependencies (path deps from shared workspace):**
- `sgr-agent` at `../../shared/rust-code/crates/sgr-agent` — LLM agent framework (tool calling, sessions, compaction)
- `sgr-agent-tui` at `../../shared/rust-code/crates/sgr-agent-tui` — ratatui TUI components (ChatPanel, FocusRing, terminal setup)

**Key decisions:**
- voxkit already done — use it for all audio ops
- Languages configurable via config.toml (default: summaries in Russian, follow-ups in call language)
- Storage: JSON files in `~/.supervox/` (calls, sessions)
- LLM: sgr-agent genai (Gemini Flash primary, OpenRouter fallback)
- Real-time STT: voxkit realtime (OpenAI WebSocket)

---

## Phase 1: Workspace + Schemas

- [x] Task 1.1: Create root `Cargo.toml` workspace with members `crates/voxkit`, `crates/supervox-agent`, `crates/supervox-tui`. Verify `cargo check -p voxkit` passes. <!-- sha:fe376ad -->
- [x] Task 1.2: Create `schemas/call.json` (id, created_at, duration_secs, participants, language, transcript, translation, tags), `schemas/call_analysis.json` (summary, action_items, follow_up_draft, decisions, open_questions, mood, themes), `schemas/config.json` (my_language, stt_backend, llm_model, summary_lag_secs, capture). <!-- sha:3d74123 -->

## Phase 2: supervox-agent — Domain Types + Tools

- [x] Task 2.1: Create `crates/supervox-agent/Cargo.toml` with deps (sgr-agent path dep, voxkit path dep, serde, serde_json, chrono). Define domain types in `src/types.rs`: `Call`, `CallAnalysis`, `ActionItem`, `Mood` enum, `CallMatch`. Tests: serialization roundtrip. <!-- sha:26af44d -->
- [x] Task 2.2: Implement `translate` tool in `src/tools/translate.rs` — sgr-agent ToolDef, takes text + source_lang + target_lang, returns translated text via LLM structured call. TDD with English→Russian fixture. <!-- sha:4b7cf3e -->
- [x] Task 2.3: Implement `rolling_summary` tool in `src/tools/rolling_summary.rs` — takes recent transcript chunks + prior summary context, returns 2-3 bullet points of condensed meaning in target language. TDD with multi-turn fixture. <!-- sha:4b7cf3e -->
- [x] Task 2.4: Implement `analyze_call` tool in `src/tools/analyze.rs` — takes full transcript, returns `CallAnalysis` struct. TDD with fixture call transcript. <!-- sha:4b7cf3e -->
- [x] Task 2.5: Implement `draft_follow_up` tool in `src/tools/follow_up.rs` — takes `CallAnalysis` + language, returns email draft string. TDD. <!-- sha:4b7cf3e -->
- [x] Task 2.6: Implement `search_calls` tool in `src/tools/search.rs` — searches saved call JSON files by text query, returns `Vec<CallMatch>` with snippets. TDD with temp dir fixtures. <!-- sha:4b7cf3e -->
- [x] Task 2.7: Implement `ask_about_calls` tool in `src/tools/ask.rs` — takes question + call context, answers via LLM. TDD. <!-- sha:4b7cf3e -->
- [x] Task 2.8: Add storage module `src/storage.rs` — `save_call()`, `load_call()`, `list_calls()`. Path: `~/.supervox/calls/<date>-<id>.json`. TDD: roundtrip in temp dir. <!-- sha:4b7cf3e -->

## Phase 3: supervox-tui — Foundation + CLI

- [x] Task 3.1: Create `crates/supervox-tui/Cargo.toml` with deps (sgr-agent, sgr-agent-tui, voxkit with features `openai mic wav`, supervox-agent, ratatui, crossterm, clap, tokio). Clap CLI: subcommands `live`, `analyze <file>`, `agent`, `calls`. <!-- sha:388dbcb -->
- [x] Task 3.2: Build TUI app framework in `src/app.rs` — `App` struct with `Mode` enum (Live, Analysis, Agent), event loop (crossterm + tokio mpsc), terminal init/restore from sgr-agent-tui. Key binding: `q` = quit. <!-- sha:388dbcb -->
- [x] Task 3.3: Implement `calls` subcommand (non-TUI stdout) — list saved calls from storage with date, duration, first line of transcript. <!-- sha:388dbcb -->

## Phase 4: supervox-tui — Live Mode

- [x] Task 4.1: Build Live mode layout in `src/modes/live.rs` — left panel (transcript + translation, auto-scroll Paragraph), right panel (rolling summary, Paragraph), bottom bar (mic ●, STT backend, timer). Ratatui Layout::horizontal split. <!-- sha:f5ab1f7 -->
- [x] Task 4.2: Integrate mic capture — `r` key starts `voxkit::mic::MicCapture`, audio level shown in status bar. `s` key stops. Wire captured `AudioChunk` to STT pipeline. <!-- sha:7dbea6a -->
- [x] Task 4.3: Integrate STT + auto-save — pipe audio to `voxkit::openai_stt::OpenAiStt`, display transcript segments in left panel. On stop: save call JSON via supervox-agent storage. <!-- sha:7dbea6a -->

## Phase 5: supervox-tui — Analysis + Agent

- [x] Task 5.1: Implement Analysis mode in `src/modes/analysis.rs` — triggered after live stop or via `analyze <file>`. Run `analyze_call` + `draft_follow_up` tools, display in scrollable panel. `c` key = copy follow-up to clipboard. <!-- sha:44f52d1 -->
- [x] Task 5.2: Implement Agent mode in `src/modes/agent.rs` — sgr-agent-tui `ChatPanel` for interactive Q&A. Load recent calls as context. Wire `ask_about_calls` and `search_calls` tools. <!-- sha:44f52d1 -->

## Phase 6: Verification

- [x] Task 6.1: Run full verification: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --all -- --check`. Fix all issues. Commit with message `feat: SuperVox Phase 1 complete`.
