---
type: plan
status: draft
title: "Finish SuperVox and Voxlog: one voice core, two shells"
created: 2026-10-25
tags: [supervox, voxlog, plan, on-device, parakeet]
---

# Finish SuperVox and Voxlog

Written after a full day of live use at Cal Hacks 13.0 (25.10.2026): SuperVox recorded the
finalist-judges briefing, ~10 project pitches, a deliberation and Sergey Levine's talk,
with Claude Code reading `current.jsonl` and writing `summary.md`. Ideas below marked
**[muselet]** / **[brainsync]** / **[vigil]** come from Cal Hacks repos, see
`6-crm/trips/talks/2026-10-25-calhacks/repo-ideas.md`.

## What the two products are

| | SuperVox | Voxlog |
|---|---|---|
| Shell | Rust TUI on macOS (`supervox live`) | iOS app (Swift) + macOS bash daemon |
| Job | A conversation *now*: live transcript, diarization, an agent writes the panel | The whole day: background segments, on-device transcription later |
| STT | Parakeet (local), OpenAI Realtime | Apple Speech, Parakeet, GigaAM (Core ML port) |
| Output | `~/.supervox/live/*.jsonl`, `calls/` | per-segment transcripts, summaries via Foundation Models |
| State | Works daily; diarization merges speakers (measured, `docs/diarization.md`) | 1.0 rejected 12.10 (4.3/4.2.6), reply sent, no answer 13 days |

They already share the *idea* of a core: capture → VAD → on-device ASR → speakers →
summary. They do not share code (Rust vs Swift). Unify the **contract**, not the language.

## Decision: one core contract, two shells

1. **One event stream format.** Both write the same JSONL: `start`, `transcript`
   (`t`, `ts`, `speaker`, `text`, `partial?`), `heartbeat`, `bookmark`, `stop`.
   SuperVox already does (`live/current.jsonl`). Voxlog writes it per segment. Any agent
   (Claude Code, Foundation Models, the summary skill) reads one format.
2. **One model shelf.** Parakeet / GigaAM Core ML weights live in one place on the Mac
   (`~/Library/Application Support/vox-models/`), downloaded once, used by both.
3. **Two shells stay.** SuperVox = live, in the room, with an agent beside it.
   Voxlog = all-day pocket recorder. No merge into one app (that is also Apple's 4.3
   question 6: they are different jobs, keep them separate products).

## SuperVox: done when

- [x] Live record, external summary panel, heartbeat, archive of sessions
- [x] `auto_record = true` by default (25.10, commit 346c137): no R/Space needed
- [ ] **Distance mic.** Today's lesson: from a table the far speaker is lost. Support
      the DJI Mic / Pocket 3 over USB (`docs/dji.md`) and an iPhone as remote mic
      (Voxlog streams its `transcript` events to SuperVox over the LAN; see below).
- [ ] **Diarization that separates people** in a room, measured on today's recordings
      (`~/.supervox/live/sessions/2026-10-25*`) with a held-out baseline **[brainsync]**:
      thresholds fixed before looking, permutation check, "always one speaker" baseline.
- [ ] **Speculative partials** **[muselet]**: re-transcribe the open buffer every
      ~450 ms, cancel stale work with a generation counter; the panel moves before the
      phrase ends. Keep the final pass as the source of truth.
- [ ] **Parakeet warm-up** **[muselet]**: 1 s of silence through the model on start
      (builds the ANE plan), pad short utterances to 1 s, 60 ms tail.
- [ ] **Modes as config, not code**: briefing / pitch-judging / talk / deliberation,
      each a summary template the agent fills (today's tables were written by hand).
- [ ] **Battery**: a `--lowpower` profile (no diarization, longer VAD pause) for
      conferences; today the laptop hit 13 % by 15:00.
- [ ] Release: `brew` tap or signed binary, README with a 60-second demo, v1.0 tag.

## Voxlog: done when

- [ ] **App Review**: follow up in Resolution Center (13 days silent). If no answer by
      01.11, request a call. Do not resubmit a new build for 4.3 (see app-review-1.0.md).
- [ ] **Remote mic for SuperVox**: Voxlog on iPhone streams `transcript` events (not
      audio) over the LAN / Bonjour to `supervox live`. Gives the far-speaker fix above
      and is a clear feature difference for 4.3.
- [ ] **One-pass classifier** **[muselet]**: tag each segment (meeting, lecture, note,
      noise) by scoring options in a cached prompt and reading choice logits, no
      generation. Foundation Models on iOS 26, Qwen 4B MLX on the Mac.
- [ ] **Test from replays** **[muselet `TESTING_POSTMORTEM.md`]**: every bug becomes a
      recorded-audio replay that fails on `main` before the fix; no fakes that accept
      anything; assert on what is on screen, not on what the code says it did.
- [ ] Ship 1.0, then 1.1 = SuperVox link.

## Order (two weeks, while travelling)

1. Voxlog: Resolution Center follow-up (10 min, today).
2. SuperVox: Parakeet warm-up + speculative partials (small, measurable).
3. SuperVox: diarization eval on 25.10 recordings with a baseline.
4. Voxlog → SuperVox LAN transcript stream (the feature that ties them).
5. SuperVox release v1.0; Voxlog 1.1.

## Not now

A shared Rust/Swift FFI core; cloud sync; a desktop GUI for SuperVox
(`crates/supervox-desktop` stays experimental).
