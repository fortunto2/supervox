# Specification: Quality Hardening

**Track ID:** quality-hardening_20260320
**Type:** Refactor
**Created:** 2026-03-20
**Status:** Draft

## Summary

After 14 feature tracks and 166 commits, the codebase has quality gaps that need addressing before the next feature cycle. Config fields use bare `String` where typed enums would catch invalid values at compile time. The JSON schema is missing 2 fields added in recent tracks (whisper_model, ducking_threshold) and has a wrong enum for stt_backend. Several production code paths use `unwrap()` where `?` or graceful fallbacks should be used. Config is loaded without any validation — invalid enum values, out-of-range numbers, or nonsensical combinations are silently accepted and cause runtime errors.

This track hardens the codebase by: (1) adding typed enums for Config fields, (2) adding validation on config load with user-friendly warnings, (3) syncing the JSON schema, and (4) replacing production `unwrap()` calls with proper error handling.

## Acceptance Criteria

- [x] Config `stt_backend` uses a typed enum (`Realtime | Whisper`) instead of `String`
- [x] Config `capture` uses a typed enum (`Mic | MicSystem`) instead of `String`
- [x] Config `llm_backend` uses a typed enum (`Auto | Ollama`) instead of `String`
- [x] Config `ducking_threshold` is validated: 0.0–1.0 range, warning on load if out of range
- [x] Config `summary_lag_secs` is validated: minimum 1, warning if 0
- [x] JSON schema `schemas/config.json` includes `whisper_model` and `ducking_threshold` fields
- [x] JSON schema `stt_backend` enum is `["realtime", "whisper"]` (not `["realtime", "openai"]`)
- [x] All `unwrap()` calls in non-test code in `storage.rs` are replaced with `?` or safe alternatives
- [x] All existing tests pass (55+)
- [x] Clippy clean, no new warnings

## Dependencies

- None (purely internal refactoring)

## Out of Scope

- TUI integration test coverage (separate track)
- Config hot-reload / file watching
- New config fields
- Custom tag management UI

## Technical Notes

- Config is in `crates/supervox-agent/src/types.rs:148-226` — serde-driven with defaults
- Config load is in `crates/supervox-agent/src/storage.rs:494-505` — no validation step
- Schema is in `schemas/config.json` — missing whisper_model, ducking_threshold; stt_backend enum wrong
- `unwrap()` in `storage.rs:36` (`file_stem`), `storage.rs:478` (`next()` after length check — safe but idiomatic improvement)
- Typed enums must serde to/from the same strings as current bare strings (backward compatible)
- Config tests in `types.rs` already test defaults and partial TOML — extend for validation
