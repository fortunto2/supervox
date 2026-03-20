# Specification: Call Management & Export

**Track ID:** call-management_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox records calls and runs LLM analysis, but users currently have no way to delete old/test calls, export analysis outside the TUI, or search calls from the CLI. This track adds three essential call management capabilities: delete, export (markdown), and CLI search. These are practical daily-use features that prevent call history from becoming an unbounded, unmanageable pile of JSON files.

The retro (8.9/10) confirmed all 4 pipeline tracks completed cleanly. This is the logical next step — making the existing data actually manageable.

## Acceptance Criteria

- [x] `supervox delete <call-id>` removes a call JSON file from `~/.supervox/calls/` with confirmation prompt
- [x] `supervox delete <call-id> --force` skips confirmation
- [x] 'd' key in History mode deletes the selected call with inline confirmation (y/n)
- [x] After deletion in TUI, call list refreshes and cursor adjusts
- [x] `supervox export <call-id>` outputs call + analysis as markdown to stdout
- [x] `supervox export <call-id> -o <file>` writes markdown to a file
- [x] Export includes: date, duration, participants, transcript, translation (if any), and analysis sections (summary, action items, decisions, open questions, mood, themes)
- [x] `supervox search <query>` searches transcripts using existing `search_calls` tool and shows matches with context
- [x] 'e' key in Analysis mode exports current call + analysis to clipboard as markdown
- [x] All new storage functions have unit tests
- [x] All new CLI commands have integration-style tests

## Dependencies

- `supervox_agent::storage` — needs `delete_call()` function
- `supervox_agent::tools::search` — reuse for CLI search
- Existing `arboard` crate for clipboard export in Analysis mode

## Out of Scope

- Batch delete (delete all before date, etc.)
- Call tagging UI (tags field exists in schema but wiring UI is a separate track)
- Cross-call pattern analysis (separate feature)
- Export formats other than markdown (PDF, HTML, etc.)

## Technical Notes

- Storage uses `{date}-{id}.json` filename pattern — delete must find file by call ID suffix match (same as `load_call`)
- `search_calls` tool already does text search with UTF-8-safe substring matching — reuse directly
- Export markdown format should be self-contained and pasteable into Obsidian/Notion
- History mode needs `delete_call()` in storage + TUI state refresh after delete
- Analysis mode already has clipboard (`arboard`) — add 'e' for markdown export
