# SuperVox Phase 3 — Analysis + Agent Modes

**Status:** [ ] Not Started
**Track:** phase3

## Context Handoff

**Intent:** Make Analysis and Agent modes production-quality. Phase 1 built the skeleton — now add rich output, clipboard integration, cross-call search, and conversation memory.

**Key files:** `crates/supervox-tui/src/modes/analysis.rs`, `crates/supervox-tui/src/modes/agent.rs`, `crates/supervox-agent/src/tools/`

**Depends on:** Phase 2 complete

---

- [ ] Task 1.1: Enrich Analysis mode output — structured sections: "Итоги" (summary), "Мои действия" (action items as checklist), "Ключевые решения", "Открытые вопросы", "Follow-up draft". Use ratatui styled text (headers bold, action items as checkboxes).
- [ ] Task 1.2: Add clipboard support — `c` key copies follow-up draft to system clipboard (pbcopy on macOS, xclip on Linux). `C` (shift) copies full analysis as markdown.
- [ ] Task 1.3: Improve Agent mode context — on startup load last 10 calls as context. Use sgr-agent Compactor with journal-focused prompt to fit in context window. Show "N calls loaded" in status.
- [ ] Task 1.4: Add Agent mode search — when user asks about specific call, use `search_calls` tool to find relevant calls. Display search results with date + snippet before answering.
- [ ] Task 1.5: Add Agent mode follow-up generation — user can say "напиши фолоап" and agent generates follow-up email using `draft_follow_up` tool with context from the conversation.
- [ ] Task 1.6: Add call tagging — after analysis, auto-tag calls with themes. Show tags in `calls` list. Allow `search_calls` to filter by tag.
- [ ] Task 1.7: Add export — `e` key in Analysis mode exports call + analysis as markdown file to `~/Desktop/supervox-export-<date>.md`.
- [ ] Task 1.8: Run full verification: tests, clippy, fmt.
