# Specification: Persist Analysis & Cross-call Insights

**Track ID:** call-insights_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox analyzes calls via LLM but analysis results are ephemeral — lost when the TUI exits and re-run from scratch every time a past call is opened. This wastes LLM API calls and adds latency. Additionally, `Call.tags` is always empty (never populated from analysis themes), and there's no way to see patterns across multiple calls.

This track adds three capabilities:
1. **Persist analysis** — save `CallAnalysis` as `.analysis.json` files alongside call files, skip LLM if already analyzed
2. **Auto-tag calls** — backfill `Call.tags` from `CallAnalysis.themes` when analysis completes
3. **Cross-call insights** — `supervox insights` command that loads all calls + analyses and generates recurring patterns, mood trends, and action item overview via LLM

## Acceptance Criteria

- [x] Analysis results saved as `{date}-{id}.analysis.json` in `~/.supervox/calls/` after LLM analysis completes
- [x] Opening a previously-analyzed call in Analysis mode loads persisted analysis instantly (no LLM call)
- [x] If no persisted analysis exists, LLM analysis runs as before and result is saved
- [x] `Call.tags` updated from `CallAnalysis.themes` when analysis is saved
- [x] `build_calls_context()` in agent mode includes analysis summaries + themes (not just 200-char transcript preview)
- [x] `supervox insights` CLI command generates cross-call patterns from all calls + analyses
- [x] `supervox insights --json` outputs structured `CallInsights` JSON
- [x] `CallInsights` type includes: recurring_themes, mood_trend, open_action_items, key_patterns
- [x] All new storage functions have unit tests
- [x] All new CLI commands have integration-style tests

## Dependencies

- `supervox_agent::storage` — needs `save_analysis()`, `load_analysis()`, `update_call_tags()`
- `supervox_agent::types` — needs `CallInsights` type
- Existing `analysis_pipeline.rs` — hook save after `analyze_transcript()` returns
- No new external crates needed

## Out of Scope

- TUI insights mode / dashboard (separate track — this is CLI-only)
- Scheduled/automatic insights generation (manual CLI command only)
- Call filtering by tag in History mode (separate UX track)
- Re-analysis of old calls (user can delete `.analysis.json` and re-open)
- Cross-call agent tool (agent mode already has richer context from Phase 2)

## Technical Notes

- Analysis files use same naming convention as calls: `{date}-{id}.analysis.json` — sibling files in same `~/.supervox/calls/` directory
- `load_analysis()` finds file by ID suffix match (same pattern as `load_call`)
- `update_call_tags()` loads call JSON, sets `tags = themes`, re-saves — idempotent
- Agent context enrichment: `build_calls_context()` should include summary + themes per call instead of 200-char transcript preview — much higher signal for LLM
- Insights generation: load all calls + analyses, send summaries/themes/action_items to LLM for structured cross-call analysis. Use `Llm::structured()` for typed output.
- The `CallInsights` type needs `JsonSchema` derive for structured LLM output
