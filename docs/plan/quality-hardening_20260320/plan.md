# Implementation Plan: Quality Hardening

**Track ID:** quality-hardening_20260320
**Spec:** [spec.md](./spec.md)
**Created:** 2026-03-20
**Status:** [ ] Not Started

## Overview

Replace stringly-typed Config fields with enums, add validation on load, sync JSON schema, and eliminate production `unwrap()` — all while maintaining backward compatibility with existing config files and tests.

## Phase 1: Typed Config Enums <!-- checkpoint:036bda2 -->

Add enums for config fields that are currently bare `String`, maintaining serde compatibility.

### Tasks
- [x] Task 1.1: Add `SttBackend`, `CaptureMode`, `LlmBackend` enums <!-- sha:a45672d --> to `crates/supervox-agent/src/types.rs` with `#[serde(rename_all = "lowercase")]` and appropriate variants. Update `Config` struct fields from `String` to these enums. Update default functions.
- [x] Task 1.2: Update all Config consumers <!-- sha:a45672d --> in `crates/supervox-tui/` — replace string comparisons (`== "realtime"`, `== "mic+system"`, `== "ollama"`) with enum matches. Key files: `src/modes/live.rs`, `src/audio.rs`, `src/intelligence.rs`, `src/main.rs`.
- [x] Task 1.3: Update test Config literals <!-- sha:a45672d --> across all crates to use enum variants instead of strings.
- [x] Task 1.4: Write tests <!-- sha:036bda2 --> — enum serde roundtrip (serialize + deserialize for each variant), backward compat with existing TOML strings.

### Verification
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` clean
- [x] Existing config.toml files parse without changes

## Phase 2: Config Validation + Schema Sync

Add a validation step after config load. Sync JSON schema with current code.

### Tasks
- [ ] Task 2.1: Add `Config::validate(&self) -> Vec<String>` method in `types.rs` — returns list of warnings. Check: `ducking_threshold` in 0.0..=1.0, `summary_lag_secs` >= 1, `whisper_model` is one of known values (tiny/base/small/medium).
- [ ] Task 2.2: Call `validate()` in `storage::load_config()` — log warnings via `tracing::warn!`, but do NOT reject config (lenient).
- [ ] Task 2.3: Update `schemas/config.json` — add `whisper_model` field (string, enum: tiny/base/small/medium, default "base"), add `ducking_threshold` (number, min 0.0, max 1.0, default 0.05), fix `stt_backend` enum to `["realtime", "whisper"]`.
- [ ] Task 2.4: Write tests — validation warns on out-of-range ducking_threshold, validation warns on zero summary_lag_secs, validation returns empty on valid config.

### Verification
- [ ] `cargo test --workspace` passes
- [ ] Schema validates default config without errors
- [ ] Invalid config prints warnings but still loads

## Phase 3: Unwrap Cleanup + Docs

Replace production `unwrap()` calls with safe alternatives.

### Tasks
- [ ] Task 3.1: Replace `unwrap()` in `storage.rs` production code — `file_stem().unwrap_or_default()` is already safe; check `find_action_by_prefix` line 478 (`next().unwrap()` after length==1 check — replace with `.into_iter().next().expect("checked length")`or pattern match).
- [ ] Task 3.2: Audit and fix `unwrap()` in `crates/supervox-tui/src/main.rs` — replace `.parse().unwrap()` with `.parse()?` or with_context.
- [ ] Task 3.3: Update CLAUDE.md with typed enum config changes (SttBackend, CaptureMode, LlmBackend types).

### Verification
- [ ] No `unwrap()` in production code paths of storage.rs and main.rs (tests exempt)
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] CLAUDE.md reflects typed config

## Final Verification

- [ ] All acceptance criteria from spec met
- [ ] Tests pass (55+, likely more after new tests)
- [ ] Linter clean
- [ ] Build succeeds (debug + release)
- [ ] Documentation up to date

## Context Handoff

_Summary for /build to load at session start — keeps context compact._

### Session Intent
Harden config type safety, add validation, sync schema, remove production unwrap() calls.

### Key Files
- `crates/supervox-agent/src/types.rs` — Config struct + new enums + validate()
- `crates/supervox-agent/src/storage.rs` — load_config() validation call + unwrap() cleanup
- `crates/supervox-tui/src/modes/live.rs` — string→enum comparisons for stt_backend, capture
- `crates/supervox-tui/src/audio.rs` — capture mode string→enum
- `crates/supervox-tui/src/intelligence.rs` — stt_backend string→enum
- `crates/supervox-tui/src/main.rs` — llm_backend string→enum + unwrap() cleanup
- `schemas/config.json` — add missing fields, fix stt_backend enum

### Decisions Made
- Enums use `#[serde(rename_all = "lowercase")]` for TOML backward compat
- `CaptureMode::MicSystem` serializes as `"mic+system"` (custom serde)
- Validation is lenient (warnings, not errors) — don't break existing configs
- `whisper_model` stays as String (too many possible models to enumerate)

### Risks
- `CaptureMode::MicSystem` needs `#[serde(rename = "mic+system")]` since `rename_all = "lowercase"` won't handle the `+`
- Consumers may compare strings in ways not caught by compiler after enum switch — grep thoroughly
- `effective_model()` method uses string comparison on llm_backend — needs enum match

---
_Generated by /plan. Tasks marked [~] in progress and [x] complete by /build._
