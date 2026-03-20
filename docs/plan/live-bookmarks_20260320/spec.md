# Specification: Live Call Bookmarks

**Track ID:** live-bookmarks_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

During live calls, users often hear something important — a decision, a number, a name — and want to mark that moment for later review. Currently there's no way to flag key moments during recording.

This feature adds bookmarks: press 'b' during a live call to drop a timestamped marker. Bookmarks are saved with the Call, shown in Analysis mode, included in markdown exports, and listed via CLI. The LLM analysis prompt will also receive bookmarks so it can pay special attention to those moments.

## Acceptance Criteria

- [ ] Pressing 'b' during live recording creates a bookmark at current elapsed time
- [ ] Bookmark count shown in live mode status bar (e.g. "2 bookmarks")
- [ ] Bookmarks persisted in Call JSON (`bookmarks` field, backward-compatible)
- [ ] Old calls without bookmarks deserialize correctly (empty vec default)
- [ ] Analysis mode shows bookmarks section with timestamps
- [ ] Markdown export includes bookmarks section
- [ ] `supervox calls` shows bookmark count per call (if any)
- [ ] Help overlay updated with 'b' key in live mode
- [ ] JSON schema updated for bookmarks field

## Dependencies

- None — all changes within supervox workspace

## Out of Scope

- Bookmark labels/notes (future: press 'b' then type a note)
- Audio seek to bookmark timestamp
- Bookmark editing/deletion after recording
- Feeding bookmarks into LLM analysis prompt (future enhancement)

## Technical Notes

- `Bookmark` struct: `{ timestamp_secs: f64, note: Option<String> }` — note field is always None for now but keeps the type extensible
- Bookmarks stored in `LiveState` during recording, passed through `AudioEvent::Stopped`
- `Call.bookmarks: Vec<Bookmark>` with `#[serde(default)]` for backward compat
- Timestamp is `LiveState.elapsed_secs()` as f64 at moment of keypress
- Visual marker in live transcript: `▶ Bookmark at 2:34` line inserted in transcript view
