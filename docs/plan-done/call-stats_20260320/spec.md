# Specification: Call Statistics & Batch Analysis

**Track ID:** call-stats_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox saves calls with analysis persistence, but calls recorded before that feature (or when LLM was unavailable) have no cached analysis. These calls don't contribute to `supervox insights`. Additionally, there's no way to see aggregate call statistics — total call time, calls per period, analysis coverage.

This track adds:
1. `supervox stats` CLI command — aggregate call metrics (total calls, duration, analysis coverage, top themes, calls per week)
2. `supervox analyze-all` CLI command — batch-analyze all calls missing `.analysis.json`
3. Stats display in TUI History mode header — quick overview when browsing calls
4. Analysis status indicator in `supervox calls` output — show which calls have analysis

## Acceptance Criteria

- [x] `supervox stats` shows: total calls, total duration, analysis coverage %, top 5 themes, calls this week/month
- [x] `supervox stats --json` outputs CallStats as JSON
- [x] `supervox analyze-all` processes all calls without `.analysis.json`, shows progress
- [x] `supervox analyze-all --dry-run` lists unanalyzed calls without processing
- [x] `supervox calls` output shows analysis indicator (✓/✗) per call
- [x] History mode header shows total calls count + total duration
- [x] CallStats type defined in types.rs with JSON schema
- [x] Tests cover stats computation, batch analysis logic, and edge cases (empty dir, all analyzed)

## Dependencies

- Existing: `supervox-agent::storage` (list_calls, load_analysis, save_analysis)
- Existing: `analysis_pipeline::analyze_transcript`
- No new external dependencies

## Out of Scope

- Date range filtering in CLI/TUI (separate track)
- Tag management UI (separate track)
- Participant detection/diarization (separate track)

## Technical Notes

- `list_calls()` + `load_analysis()` already exist — stats computation is pure aggregation
- `analyze_transcript()` from `analysis_pipeline.rs` can be reused for batch processing
- Batch analysis should be sequential (not parallel) to avoid LLM rate limits
- `update_call_tags()` should be called after each batch analysis (same as live flow)
- Stats computation is O(n) over calls directory — no caching needed for reasonable call counts
