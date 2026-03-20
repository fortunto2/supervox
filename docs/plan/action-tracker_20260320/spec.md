# Specification: Action Item Tracker

**Track ID:** action-tracker_20260320
**Type:** Feature
**Created:** 2026-03-20
**Status:** Draft

## Summary

SuperVox extracts action items from call analysis (via LLM), but they're display-only — users can't mark them done or track completion across calls. For a "productivity TUI," this is the missing loop: record → analyze → act → **track** → done.

This track adds persistent action item tracking: a stable ID for each action, completion state stored in `~/.supervox/actions.json`, CLI commands to list/complete/undo items, and TUI integration in Analysis mode.

## Acceptance Criteria

- [ ] Each action item has a deterministic ID (hash of call_id + description)
- [ ] Action completion state persists in `~/.supervox/actions.json`
- [ ] `supervox actions` lists all open action items across calls (grouped by call)
- [ ] `supervox actions --all` includes completed items
- [ ] `supervox actions done <id-prefix>` marks an action as complete
- [ ] `supervox actions undo <id-prefix>` reverts completion
- [ ] `supervox actions --json` outputs structured JSON
- [ ] Filter flags (--tag, --since, --until) work with `supervox actions`
- [ ] Analysis mode TUI shows ☐/☑ status for each action item
- [ ] All new functions have unit tests

## Dependencies

- No new external crates required
- Uses existing `supervox-agent::storage` and `supervox-agent::types` modules
- Uses existing `CallFilter` for narrowing scope

## Out of Scope

- Due date reminders / notifications
- Action item editing (reassign, change deadline)
- Priority ordering beyond call date
- Syncing action items to external tools (calendar, Todoist)

## Technical Notes

- `ActionItem` already has `description`, `assignee`, `deadline` — add nothing to the struct itself
- ID derived as: first 8 chars of SHA-256(`{call_id}:{description}`) — deterministic, collision-resistant
- Action store is a simple HashMap<action_id, ActionState> serialized to JSON
- `ActionState { completed: bool, completed_at: Option<DateTime<Utc>> }`
- Insights `open_action_items` can later use this store to filter truly completed items (optional, not in scope for this track)
