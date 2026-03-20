# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 8.6/10 (final — 12 runs, 10 tracks built)

Pipeline: build → deploy → review (x12 runs) | Iters: 34 | Waste: 5.9%
Tracks: phase1 (124m) → realtime-live (32m) → analysis-agent-wire (28m) → ux-polish (32m) → call-management (23m) → call-insights (30m) → call-stats (23m) → call-filter (26m) → action-tracker-v1 (SKIPPED — state leak) → audio-save (48m) → action-tracker-v2 (retro only) → action-tracker-v3 (20m — recovery)
Total: 131 commits, 211 tests, 100% conventional, ~435 min wall time

### Defects
- **CRITICAL** | solo-dev.sh: State files not cleaned on global timeout → action-tracker-v1 falsely archived. **WORKAROUND:** action-tracker re-built in Run 12. **FIX NOT APPLIED** in solo-dev.sh — `check_timeout()` function missing, state cleanup absent.
  - Fix: `scripts/solo-dev.sh` — implement `check_timeout()` with state cleanup
- **HIGH** | deploy: No CLI/TUI project-type detection — AskUserQuestion stall in Run 1. Self-corrected in later runs.
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check
- **MEDIUM** | solo-dev.sh: 6 aborted pre-starts (rate-limit + config issues)
  - Fix: `scripts/solo-dev.sh` — add startup validation + pre-flight API check

### Harness Gaps
- **Context:** None — CLAUDE.md lean at 12.3K, schemas-first domain model
- **Constraints:** None — clean module boundaries across all 12 runs
- **Precedents:** Auto-plan cycling evolved 10 focused tracks. Progressive speedup: 124m → 20m.
  - Bad precedent: pipeline trusts state files without verifying plan completion — needs defensive validation

### Missing
- State cleanup on timeout/restart in solo-dev.sh (still unfixed)
- Rust-native deploy strategy (cargo publish, homebrew tap)
- TUI visual regression testing (ratatui snapshot tests)
- Pre-flight API validation before main loop
- `cargo-audit` in CI for dependency vulnerability scanning

### What worked well
- **Auto-plan:** 10 successful plan evolutions — progressive, focused tracks
- **Runs 2-8 flawless:** 21/21 productive, 0 waste
- **Build skill:** 10/10 tracks at 100% fidelity, all with SHA annotations
- **action-tracker recovery:** State leak defect identified and worked around in 20 min
- **Schemas-first:** Clean dependency flow maintained across all 12 runs
- **Redo mechanism:** Caught real UTF-8 bug before shipping (Run 1)
- **Progressive improvement:** 124m → 20m per track
- **CLAUDE.md discipline:** Stayed lean (12.3K) throughout
- **100% conventional commits:** 131/131 commits follow convention
