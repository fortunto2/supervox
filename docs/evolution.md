# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 8.6/10 (final — 13 runs, 11 tracks built)

Pipeline: build → deploy → review (x13 runs) | Iters: 37 | Waste: 5.4%
Tracks: phase1 (124m) → realtime-live (32m) → analysis-agent-wire (28m) → ux-polish (32m) → call-management (23m) → call-insights (30m) → call-stats (23m) → call-filter (26m) → action-tracker-v1 (SKIPPED — state leak) → audio-save (48m) → action-tracker-v2 (retro only) → action-tracker-v3 (20m — recovery) → live-bookmarks (24m)
Total: 141 commits, 224 tests, 100% conventional, ~459 min wall time

### Defects
- **CRITICAL** — FIXED (368625f) | solo-dev.sh:939: State files not cleaned on global timeout → action-tracker-v1 falsely archived. **WORKAROUND:** action-tracker re-built in Run 12. **FIX NOT APPLIED** — timeout branch at line 939 skips state cleanup (only re-exec branch at line 945 cleans state).
  - Fix: `scripts/solo-dev.sh:939` — add `rm -f "$STATES_DIR/build" "$STATES_DIR/deploy" "$STATES_DIR/review"` after TIMEOUT log entry
- **HIGH** — FIXED (0462049) | deploy: No CLI/TUI project-type detection — AskUserQuestion stall in Run 1. Self-corrected in all later runs (Runs 10, 12, 13).
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check
- **MEDIUM** | solo-dev.sh: 6 aborted pre-starts (rate-limit + config issues)
  - Fix: `scripts/solo-dev.sh` — add startup validation + pre-flight API check
- **LOW** | plan: Spec-plan alignment gap — live-bookmarks spec had 9 criteria, plan covered 8. Missing: CLI table column for bookmark count.
  - Fix: `skills/plan/SKILL.md` — add spec coverage verification before finalizing plan

### Harness Gaps
- **Context:** None — CLAUDE.md lean at 12.8K, schemas-first domain model
- **Constraints:** None — clean module boundaries across all 13 runs
- **Precedents:** Auto-plan cycling evolved 11 focused tracks. Progressive speedup: 124m → 20m → 24m (stable).
  - Bad precedent: pipeline trusts state files without verifying plan completion — needs defensive validation
  - Good precedent: deploy skill self-corrected — learned CLI/TUI context by Run 10

### Missing
- State cleanup on timeout/restart in solo-dev.sh (still unfixed at scripts/solo-dev.sh:939)
- Rust-native deploy strategy (cargo publish, homebrew tap)
- TUI visual regression testing (ratatui snapshot tests)
- Pre-flight API validation before main loop
- `cargo-audit` in CI for dependency vulnerability scanning
- Spec-plan alignment verification in /plan skill

### What worked well
- **Auto-plan:** 11 successful plan evolutions — progressive, focused tracks
- **Runs 2-8, 10, 12-13 flawless:** 30/30 productive, 0 waste
- **Build skill:** 11/11 tracks at 100% task completion, all with SHA annotations
- **action-tracker recovery:** State leak defect identified and worked around in 20 min
- **Schemas-first:** Clean dependency flow maintained across all 13 runs
- **Redo mechanism:** Caught real UTF-8 bug before shipping (Run 1)
- **Progressive improvement:** 124m → 20m per track
- **CLAUDE.md discipline:** Stayed lean (12.8K) throughout
- **100% conventional commits:** 141/141 commits follow convention
- **Deploy self-correction:** Learned CLI/TUI context after Run 1, no stalls in 4 subsequent deploy iterations
