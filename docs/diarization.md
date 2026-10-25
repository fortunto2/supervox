# Live diarization: measured 24 Oct 2026

Symptom: every segment of a 50-minute, 3-speaker investor panel was labelled `S1` (193 of 193).

Harness: `cargo run --release -p voxkit --features speaker --example diarize_sweep -- clip.f32 model.onnx`
(3 s windows, CAM++ embeddings, the live `SpeakerTracker` replayed at thresholds 0.0–0.9;
`CENTER=1` subtracts the session-mean embedding first).

| Input | Raw cosines | Result at the live threshold 0.5 |
|---|---|---|
| Known answer: macOS TTS Daniel → Samantha → Daniel, same text | same voice 0.90–0.95, male vs female 0.83 | 1 speaker (wrong: 2) |
| Panel, 10 min from t=15:00 | windows 0.78–0.91 | 1 speaker (wrong: ≥3) |

- The embeddings carry a large shared component: even a male and a female voice sit at cos 0.83, so no
  threshold below ~0.85 separates them, and the tracker's 0.5 merges everyone.
- Scaling to int16 and per-bin CMN before the model changed nothing measurable (identical sweep); reverted.
- Session-mean centering separates voices (male vs female → −0.20) but the online nearest-centroid tracker
  then over-splits (TTS A-B-A gives 3 clusters at 0.0; the panel 4–8).

Next, in order: (1) check the front end against WeSpeaker's reference (fbank params: 25 ms/10 ms, 80 bins,
dither 0, the exact input scale knf-rs expects) with a Python known-answer on the same TTS clip;
(2) if embeddings are right, cluster offline after the call (agglomerative on centered embeddings with a
stopping threshold tuned on the TTS clip), since analysis happens after the call anyway; keep live labels
as "voice changed" only. Until then, treat S1..S3 as unreliable.
