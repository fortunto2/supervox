# Evolution Log — supervox

---

## 2026-03-20 | supervox | Factory Score: 8/10

Pipeline: build → deploy → review | Iters: 7 | Waste: 28.6%

### Defects
- **HIGH** | deploy: No CLI/TUI project-type detection — asked AskUserQuestion, stalling pipeline iter 2
  - Fix: `skills/deploy/SKILL.md` — add pre-flight project-type check (binary targets + TUI/CLI keywords → auto CI/CD + release workflow)
- **LOW** | review: Doubled `<solo:redo/>` signal in output (cosmetic, no impact)
  - Fix: `skills/review/SKILL.md` — output signal once at end, not in summary block

### Harness Gaps
- **Context:** None — CLAUDE.md was lean (4,251 bytes), schemas-first approach gave agent clear domain model
- **Constraints:** None — agent stayed within module boundaries, all files under 220 LOC
- **Precedents:** Review→redo→build→review cycle worked exactly as designed for fixing real bugs

### Missing
- Rust-native deploy strategy: cargo publish, binary releases, cross-compilation, homebrew tap
- TUI visual testing: snapshot-based screen capture testing for ratatui apps

### What worked well
- Build skill: 20 tasks with SHA annotations, clean execution
- Schemas-first: JSON schemas → domain types → tools → TUI (clean dependency flow)
- Redo mechanism: caught UTF-8 slicing bug that would have panicked on Cyrillic input
