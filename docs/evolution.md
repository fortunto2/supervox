# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 8.4/10 (final — 10 runs, 9 tracks built)

Pipeline: build → deploy → review (x10 runs) | Iters: 31 | Waste: 6.5%
Tracks: phase1 (124m) → realtime-live (32m) → analysis-agent-wire (28m) → ux-polish (32m) → call-management (23m) → call-insights (30m) → call-stats (23m) → call-filter (26m) → action-tracker (SKIPPED — state leak) → audio-save (39m)
Total: 121 commits, 196 tests, 140 tasks with SHAs, ~415 min wall time

### Defects
- **CRITICAL** | solo-dev.sh:925: State files not cleaned on global timeout → action-tracker falsely archived with 0% completion. Stale `.solo/states/{build,deploy,review}` from Run 8 persisted across timeout+restart boundary.
  - Fix: `scripts/solo-dev.sh:925` — add `rm -f "$STATES_DIR/build" "$STATES_DIR/deploy" "$STATES_DIR/review"` after TIMEOUT log entry
- **HIGH** | deploy: No CLI/TUI project-type detection — AskUserQuestion stall in Run 1 iter 2. Clean in Runs 2-10.
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check
- **MEDIUM** | solo-dev.sh: 6 aborted pre-starts (rate-limit + config issues)
  - Fix: `scripts/solo-dev.sh` — add startup validation + pre-flight API check

### Harness Gaps
- **Context:** None — CLAUDE.md lean at 10.3K, schemas-first domain model
- **Constraints:** None — clean module boundaries across all 10 runs
- **Precedents:** Auto-plan cycling evolved 9 focused tracks. Each run faster and cleaner (124m → 22m).
  - Bad precedent: pipeline trusts state files without verifying plan completion — needs defensive validation

### Missing
- State cleanup on timeout/restart in solo-dev.sh
- Rust-native deploy strategy (cargo publish, homebrew tap)
- TUI visual regression testing (ratatui snapshot tests)
- Pre-flight API validation before main loop

### What worked well
- **Auto-plan:** 9 successful plan evolutions — progressive, focused tracks
- **Runs 2-8 flawless:** 21/21 productive, 0 waste
- **Build skill:** 140 tasks across 9 built tracks, all with SHA annotations, 100% completion
- **Audio-save:** Clean 3-iter run, WAV recording integrated into existing voxkit pipeline
- **Schemas-first:** Clean dependency flow maintained across all 10 runs
- **Redo mechanism:** Caught real UTF-8 bug before shipping (Run 1)
- **Progressive improvement:** 124m → 22m per track, 71% → 100% productive
- **CLAUDE.md discipline:** Stayed lean (10.3K) throughout
