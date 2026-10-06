# Conference mode (2026-10-05) — live run while Rustam is at a conference

Goal: `supervox live` on the built-in mic, local STT (Parakeet), summary in Russian,
agent-readable live stream, 1–3 speakers told apart.

- [x] A. Summary language: map `my_language` code → full name in prompts (ru → Russian)
- [x] B. Live stream for agents: append JSONL events to `~/.supervox/live/current.jsonl`
      (transcript + summary), so Claude Code can `tail -f` it and summarize itself
- [x] C. Transcript panel auto-scrolls to bottom
- [x] D. VAD end-of-speech pause configurable (`vad_silence_ms`, default 1500)
- [x] E. Diarization: voxkit `speaker` feature (knf-rs fbank + WeSpeaker ONNX on ort rc.12),
      online cosine clustering, `diarize = true`, `max_speakers = 3`, labels S1..S3
- [ ] F. Release build, `make install`, live test, commit
- [x] G. Mic picker: `mic_device` config key + hotkey `m` (idle) cycles input devices, name in status bar
- [ ] H. Companion loop in Claude Code: poll live JSONL every ~30 s, one evolving Russian summary,
      research names/claims, critic/mentor notes → summary.md + chat

## Status 2026-10-05 evening
- A–E, G: code done, 282 tests green (`cargo test` on tui+agent+voxkit with parakeet,speaker).
- Known-answer for the speaker model on a real conference clip: same-speaker cos 0.969,
  speaker-vs-noise −0.043 (`speaker::model_tests`, needs SPEAKER_TEST_WAV).
- Found on the way: the Parakeet path never used VAD — it cut every 2 s, which is why
  phrases were chopped. Replaced by a pause-based `Segmenter`.
- Found on the way: `~/Library/Caches/ort.pyke.io/dfbin/...` held empty extract dirs, so
  ort-sys skipped the download and linking failed. Deleting the dir fixes it.
- F: release build running; install must replace BOTH `/opt/homebrew/bin/supervox`
  (first on PATH, Mar 25 build) and `~/.cargo/bin/supervox`.
- H: skill written at `~/.claude/skills/supervox-companion/SKILL.md`; run as /loop here.
