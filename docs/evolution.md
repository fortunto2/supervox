# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 8.5/10 (updated — full pipeline)

Pipeline: build → deploy → review (x2 runs) | Iters: 10 | Waste: 20%
Run 1: Phase 1 (7 iters, 124 min) | Run 2: realtime-live (3 iters, 27 min)

### Defects
- **HIGH** | deploy: No CLI/TUI project-type detection — asked AskUserQuestion, stalling pipeline Run 1 iter 2. Recovered on retry. Clean in Run 2.
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check (binary targets + TUI/CLI keywords → auto CI/CD + release workflow)
- **MEDIUM** | solo-dev.sh: 6 aborted pre-starts before successful pipeline (config/rate-limit issues)
  - Fix: `scripts/solo-dev.sh` — add startup validation + clearer error messages
- **LOW** | review: Doubled `<solo:redo/>` signal in output (cosmetic, no impact)
  - Fix: `skills/review/SKILL.md` — output signal once at end, not in summary block

### Harness Gaps
- **Context:** None — CLAUDE.md lean (5,375 bytes), schemas-first gave clear domain model
- **Constraints:** None — agent stayed within module boundaries, all files under limits
- **Precedents:** Auto-plan cycling between runs picked right priority (realtime-live > sequential Phase 2)

### Missing
- Rust-native deploy strategy: cargo publish, binary releases, cross-compilation, homebrew tap
- TUI visual testing: snapshot-based screen capture testing for ratatui apps
- Pre-start validation in solo-dev.sh

### What worked well
- **Auto-plan:** Examined completed phases + codebase, generated focused realtime-live plan instead of running Phase 2 as-is. Better prioritization.
- **Run 2 flawless:** 3/3 productive, 0 waste, 27 min total. Build completed 4-phase plan in single iteration.
- **Build skill:** 34 tasks total across both runs, all with SHA annotations
- **Schemas-first:** JSON schemas → domain types → tools → TUI (clean dependency flow)
- **Redo mechanism:** Caught real UTF-8 slicing bug before shipping
- **CLAUDE.md quality:** Stayed lean and actionable throughout both pipeline runs
