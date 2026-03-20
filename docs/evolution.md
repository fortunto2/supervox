# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 9.2/10 (final — 6 runs complete)

Pipeline: build → deploy → review (x6 runs) | Iters: 22 | Waste: 9.1%
Run 1: Phase 1 (7 iters, 124 min) | Run 2: realtime-live (3 iters, 32 min) | Run 3: analysis-agent-wire (3 iters, 28 min) | Run 4: ux-polish (3 iters, 32 min) | Run 5: call-management (3 iters, 23 min) | Run 6: call-insights (3 iters, 25 min)

### Defects
- **HIGH** | deploy: No CLI/TUI project-type detection — asked AskUserQuestion, stalling pipeline Run 1 iter 2. Recovered on retry. Clean in Runs 2-6.
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check (binary targets + TUI/CLI keywords → auto CI/CD + release workflow)
- **MEDIUM** | solo-dev.sh: 6 aborted pre-starts before successful pipeline (config/rate-limit issues)
  - Fix: `scripts/solo-dev.sh` — add startup validation + clearer error messages
- **LOW** | review: Doubled `<solo:redo/>` signal in output (cosmetic, no impact)
  - Fix: `skills/review/SKILL.md` — output signal once at end, not in summary block

### Harness Gaps
- **Context:** None — CLAUDE.md lean at 8.8K, schemas-first gave clear domain model
- **Constraints:** None — agent stayed within module boundaries across all 6 runs, all files under limits
- **Precedents:** Auto-plan cycling evolved broad phases into focused tracks (Phase 2 → realtime-live, Phase 3 → analysis-agent-wire, Phase 4 → ux-polish, Phase 4 backlog → call-management, backlog → call-insights). Each run faster and cleaner.

### Missing
- Rust-native deploy strategy: cargo publish, binary releases, cross-compilation, homebrew tap
- TUI visual testing: snapshot-based screen capture testing for ratatui apps
- Pre-start validation in solo-dev.sh

### What worked well
- **Auto-plan:** Five successful plan evolutions — examined completed phases + codebase, generated focused tracks instead of running broad phases as-is
- **Runs 2-6 flawless:** 15/15 productive, 0 waste, total 140 min combined
- **Build skill:** 96 tasks across 6 runs, all with SHA annotations, 100% completion
- **Schemas-first:** JSON schemas → domain types → tools → pipelines → TUI (clean dependency flow)
- **Redo mechanism:** Caught real UTF-8 slicing bug before shipping (Run 1)
- **Progressive improvement:** Each run faster (124m → 32m → 28m → 32m → 23m → 25m) and cleaner (71% → 100% × 5 productive)
- **CLAUDE.md discipline:** Stayed lean (8.8K) and actionable throughout all 6 pipeline runs
- **Analysis persistence:** call-insights track correctly identified ephemeral LLM analysis as waste and added caching + cross-call intelligence
