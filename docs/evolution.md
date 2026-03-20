# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 6.5/10 (final — 18 runs, 16 tracks built)

Pipeline: build -> deploy -> review (x18 runs) | Iters: 50 | Waste: 6%
Latest: audio-import_20260320 — 3/3 iters, 40 min, 9/9 criteria, v0.7.0
Total: 183 commits, 263 tests, 100% conventional, ~17h wall time

### Delta from Run 17
- **audio-import:** flawless run. Batch file transcription (voxkit) + CLI import command (supervox-tui). +7 tests, +7 commits.
- **Checkbox fidelity:** STABLE — audio-import maintained perfect checkbox pattern from quality-hardening.
- **State leak:** STILL UNFIXED (solo-dev.sh:939). 4th flagging.

---

## 2026-03-20 | supervox | Factory Score: 6/10 (run 17 snapshot — 17 runs, 15 tracks built)

Pipeline: build -> deploy -> review (x17 runs) | Iters: 47 | Waste: 6.4%
Tracks: phase1 (124m) -> realtime-live (32m) -> analysis-agent-wire (28m) -> ux-polish (32m) -> call-management (23m) -> call-insights (30m) -> call-stats (23m) -> call-filter (26m) -> action-tracker-v1 (STATE LEAK) -> audio-save (48m) -> action-tracker-v2 (28m, recovery) -> live-bookmarks (29m) -> whisper-stt (74m) -> live-ux-v1 (STATE LEAK) -> live-ux-v2 (69m, recovery + completion) -> quality-hardening (28m, FLAWLESS)
Total: 176 commits, 256 tests, 100% conventional, ~16h wall time, v0.6.1 released

### Defects
- **CRITICAL** | solo-dev.sh:939: State files not cleaned on global timeout — **HIT TWICE, FLAGGED THRICE, NEVER FIXED.** action-tracker recovered via re-run, live-ux recovered via double re-run.
  - Fix: `scripts/solo-dev.sh:939` — add `rm -f "$STATES_DIR/build" "$STATES_DIR/deploy" "$STATES_DIR/review"` after TIMEOUT log entry
- **HIGH** | build SKILL.md: No checkbox-ticking protocol — live-ux delivered all code but spec/plan checkboxes remained unchecked (0/9, 0/31).
  - Fix: `skills/build/SKILL.md` — add explicit "tick checkbox after each task" instruction
- **HIGH** | review SKILL.md: No checkbox verification — gave SHIP verdict on live-ux with 0/9 spec checkboxes.
  - Fix: `skills/review/SKILL.md` — verify spec checkboxes match code before verdict
- **HIGH** | deploy: No CLI/TUI project-type detection — AskUserQuestion stall in Run 1. Self-corrected later.
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check
- **HIGH** | deploy: No cross-compile compatibility check — ort-sys CI failure (26 min to diagnose).
  - Fix: `skills/deploy/SKILL.md` — check native C deps for target arch support
- **MEDIUM** | build SKILL.md: Signal emitted only at end — context exhaustion in Run 15 caused signal miss.
  - Fix: `skills/build/SKILL.md` — emit progress signals at phase boundaries, not just end

### Harness Gaps
- **Context:** None — CLAUDE.md lean at 13.7K, schemas-first domain model
- **Constraints:** None — clean module boundaries across all 17 runs
- **Precedents:**
  - Bad: state leak bug hit TWICE — known-unfixed factory bugs recur and compound
  - Bad: spec checkbox fidelity not enforced — documentation integrity breaks on recovered tracks
  - Bad: build signal at end of prompt only — vulnerable to context window exhaustion
  - Bad: deploy configures CI targets without checking dep cross-compile support
  - Good: quality-hardening track showed checkbox fidelity working correctly — correcting the live-ux pattern
  - Good: auto-plan cycling evolved 15 focused tracks, progressive speedup
  - Good: live-ux fully recovered despite 2 state leaks + 1 signal miss — resilient pipeline
  - Good: StreamingSttBackend trait — clean abstraction for swapping STT backends
  - Good: CLAUDE.md stayed lean (13K) through 16 runs — zero context bloat

### Missing
- State cleanup on ALL exit paths in solo-dev.sh (timeout, crash, manual stop)
- Checkbox-ticking protocol in /build + verification in /review
- Cross-compile compatibility check in /deploy skill
- Progress signal checkpoints in /build (not just end)
- Rust-native deploy strategy (cargo publish, homebrew tap)
- Pre-flight API validation before main loop
- `cargo-audit` in CI

### What worked well
- **Auto-plan:** 15 successful plan evolutions — progressive, focused tracks
- **Runs 2-8, 10-13, 17 flawless:** 36/36 productive iterations, 0 waste
- **Recovery resilience:** Both state-leaked tracks recovered via re-runs
- **100% conventional commits:** 176/176 commits follow convention
- **256 tests, clippy clean:** Quality hardened across 7 releases (v0.1.0-v0.6.1). Tests 5x increase in single track.
- **Redo mechanism:** Caught real UTF-8 bug before shipping
- **CLAUDE.md discipline:** Stayed lean (13.7K) throughout 17 runs
- **Whisper integration:** Complex C++ dep + Metal + trait abstraction, cleanly delivered
- **live-ux delivery:** Dual VU, mic ducking, Space/Enter keybindings — all working despite rocky pipeline
- **Quality hardening:** Typed enums, config validation, unwrap() cleanup — systematic tech debt reduction in 28 min
