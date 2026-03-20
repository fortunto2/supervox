# SuperVox Phase 1 — Acceptance Criteria

## Workspace
- [x] Root `Cargo.toml` workspace with 3 members
- [x] `cargo check --workspace` passes

## Schemas
- [x] `schemas/call.json`, `schemas/call_analysis.json`, `schemas/config.json` exist with valid JSON

## supervox-agent
- [x] Domain types: Call, CallAnalysis, ActionItem, Mood, CallMatch — all Serialize/Deserialize
- [x] 6 tools implemented as sgr-agent ToolDef: translate, rolling_summary, analyze_call, draft_follow_up, search_calls, ask_about_calls
- [x] Storage: save_call, load_call, list_calls working with JSON files
- [x] `cargo test -p supervox-agent` — all pass (32 tests)

## supervox-tui
- [x] CLI subcommands: live, analyze, agent, calls
- [x] Live mode: TUI layout with transcript + summary + status panels
- [x] Live mode: mic capture with r/s keys, STT transcription
- [x] Analysis mode: post-call summary + action items + follow-up
- [x] Agent mode: interactive chat via sgr-agent-tui ChatPanel
- [x] `cargo build -p supervox-tui` compiles

## Quality
- [x] `cargo test --workspace` passes (83 tests total)
- [x] `cargo clippy --workspace -- -D warnings` clean
- [x] `cargo fmt --all -- --check` clean
