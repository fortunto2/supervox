# Specification: Wire LLM to Analysis & Agent Modes

**Track ID:** analysis-agent-wire_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

Analysis and Agent modes have complete UIs and all 6 agent tools are implemented with tests, but the TUI doesn't call any LLM. Analysis mode only shows raw call metadata via `load_from_call()`. Agent mode responds with a hardcoded "not yet connected to LLM" placeholder.

This track wires the existing tools to the existing UI — no new tools, no new UI panels. The analyze_call tool feeds structured results into AnalysisState fields. The draft_follow_up tool activates on 'f' key. Agent mode gets a real sgr-agent loop with search_calls and ask_about_calls tools.

## Acceptance Criteria

- [x] Entering Analysis mode (auto-flow from live, or `supervox analyze <file>`) runs `analyze_call` and populates summary, action_items, decisions, open_questions, mood, themes
- [x] Analysis shows "Analyzing..." loading state during LLM call
- [x] Press 'f' in Analysis mode runs `draft_follow_up` and displays result in follow-up panel
- [x] Press 'c' in Analysis mode copies analysis text to clipboard
- [x] Press 'C' in Analysis mode copies follow-up text to clipboard
- [x] Agent mode sends user input to LLM via sgr-agent loop with search_calls + ask_about_calls tools
- [x] Agent mode shows streaming LLM responses (not blocking UI)
- [x] Agent loads recent calls as context on startup (up to 10, compacted)
- [x] All existing tests pass (100 tests), new tests added for wiring logic
- [x] Clippy clean, fmt clean

## Dependencies

- `sgr-agent` (already in workspace) — `Llm`, `Session`, `SgrAgentStream`
- `sgr-agent-tui` (already in workspace) — `spawn_agent_loop`, `TuiAgent`, `ChannelHandler`
- `arboard` crate — clipboard (cross-platform, no external deps on macOS)

## Out of Scope

- New agent tools (all 6 already exist)
- New UI panels or modes
- Call history browser (Phase 4)
- Markdown export (Phase 4)
- Ollama/local LLM backend switching (Phase 4)

## Technical Notes

- Analysis wiring: spawn async task from `process_audio_event(Stopped{..})` and from `load_from_call()`. Task calls `AnalyzeCallTool::execute()` directly (not via agent loop — it's a single tool call). Results map 1:1 to `AnalysisState` fields.
- Follow-up wiring: on 'f' key, spawn async task calling `DraftFollowUpTool::execute()` with serialized analysis. Result goes to `AnalysisState.follow_up`.
- Agent wiring: implement `SgrAgentStream` for a `SuperVoxAgent` struct with search_calls + ask_about_calls tools. Use `spawn_agent_loop` from sgr-agent-tui. Map `AgentTaskEvent` to `AudioEvent` variants or new app events via channel.
- Clipboard: `arboard::Clipboard::new().set_text()` — single dep, works on macOS natively.
- Config: LLM model from `config.llm_model` (already loaded at startup).
