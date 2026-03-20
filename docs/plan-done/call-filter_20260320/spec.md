# Specification: Call Filtering (Tag + Date Range)

**Track ID:** call-filter_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox has 8 CLI commands and a TUI History mode for browsing calls, but no way to filter by tag or date range. As call history grows, finding specific calls becomes impractical. Tags are already populated from analysis themes (`update_call_tags()`), and every call has `created_at` — the filtering data exists but is inaccessible.

This track adds:
1. `CallFilter` struct with optional `tags`, `since`, `until` fields
2. `filter_calls()` function in storage that applies criteria to call list
3. `--tag`, `--since`, `--until` flags on `calls`, `search`, `stats`, `insights` CLI commands
4. Tag filter in TUI History mode — `t` to toggle tag filter popup, select tags to filter by
5. `supervox tags` CLI command — list all unique tags with counts

## Acceptance Criteria

- [x] `supervox calls --tag meeting` filters to calls tagged "meeting"
- [x] `supervox calls --tag meeting --tag budget` filters to calls with either tag (OR logic)
- [x] `supervox calls --since 2026-03-01` filters to calls from that date onward
- [x] `supervox calls --until 2026-03-15` filters to calls before that date
- [x] `--tag`, `--since`, `--until` flags work on `search`, `stats`, `insights` commands
- [x] `supervox tags` lists all unique tags sorted by frequency
- [x] `supervox tags --json` outputs tag list as JSON
- [x] TUI History mode: `t` key opens tag filter — select/deselect tags to show
- [x] TUI History mode title shows active filter count (e.g., "Call History (5/12 calls, filtered)")
- [x] `CallFilter` type defined in types.rs
- [x] `filter_calls()` function in storage.rs with tests
- [x] Tests cover: empty filter (passthrough), tag filter, date filter, combined filter, no matches

## Dependencies

- Existing: `storage::list_calls()`, `storage::load_analysis()`
- Existing: `Call.tags`, `Call.created_at`
- No new external dependencies

## Out of Scope

- Full-text search within filter results (existing `search` already does this)
- Saved/named filters
- Participant filtering (participants field not yet populated)
- Custom tag management (add/remove tags manually)

## Technical Notes

- `list_calls()` already loads all calls and sorts by date — `filter_calls()` wraps it with criteria
- Date parsing: use `chrono::NaiveDate::parse_from_str` for `--since`/`--until` (YYYY-MM-DD format)
- Tag filtering is case-insensitive, OR logic (matches any of the specified tags)
- TUI tag filter: collect unique tags from loaded calls, show as toggleable list
- History mode filter is client-side (already has full call list in memory)
