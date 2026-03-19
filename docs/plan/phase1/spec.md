# SuperVox Phase 1 — Acceptance Criteria

## Workspace
- Root `Cargo.toml` workspace with 3 members
- `cargo check --workspace` passes

## Schemas
- `schemas/call.json`, `schemas/call_analysis.json`, `schemas/config.json` exist with valid JSON

## supervox-agent
- Domain types: Call, CallAnalysis, ActionItem, Mood, CallMatch — all Serialize/Deserialize
- 6 tools implemented as sgr-agent ToolDef: translate, rolling_summary, analyze_call, draft_follow_up, search_calls, ask_about_calls
- Storage: save_call, load_call, list_calls working with JSON files
- `cargo test -p supervox-agent` — all pass

## supervox-tui
- CLI subcommands: live, analyze, agent, calls
- Live mode: TUI layout with transcript + summary + status panels
- Live mode: mic capture with r/s keys, STT transcription
- Analysis mode: post-call summary + action items + follow-up
- Agent mode: interactive chat via sgr-agent-tui ChatPanel
- `cargo build -p supervox-tui` compiles

## Quality
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` clean
- `cargo fmt --all -- --check` clean
