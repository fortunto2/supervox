# Parakeet-TDT-0.6B-v3: MLX (Apple GPU) vs ONNX Runtime CPU

Measured 2026-11-19, M5 32 GB, **under heavy load** (load average 23-41, other builds running), so
absolute numbers are inflated and noisy; the ratio is what to read. Warm = after 2 warmup calls.
CPU path = `asr_bench` (parakeet-rs, ONNX Runtime CPU, `~/.supervox/models/parakeet-tdt-v3`), first
segment dropped as cold. MLX = `parakeet-mlx` (`mlx-community/parakeet-tdt-0.6b-v3`, 1.6 GB download),
`model.transcribe(wav)` per segment. Same 20 segments for both (cut by energy from
`recording-20261119-172626.wav`, 3-13 s; 20 LibriSpeech clips, 129 s total).

| Set | Backend | median ms | p90 ms | RTF | peak RSS |
|---|---|---|---|---|---|
| RU room | ONNX CPU | 395 | 641 | 0.072 | 2.0 GB |
| RU room | MLX GPU | 442 / 492 (2 runs) | 746 / 780 | 0.090 / 0.101 | 0.9-1.2 GB |
| LibriSpeech | ONNX CPU | 715 / 726 | 948 / 1010 | 0.105 / 0.109 | 2.0 GB |
| LibriSpeech | MLX GPU | 434 / 546 | 617 / 769 | 0.071 / 0.088 | 0.8-0.9 GB |

Result: MLX is not faster on Russian speech (CPU ~1.2x faster), and about 1.3-1.6x faster on long
English clips: below the 1.5x bar that would justify a Python sidecar. Memory is half. Transcripts
agree (RU: both give the same sentences up to punctuation and a few word variants).
Verdict: keep ONNX CPU; no `asr = "mlx"` backend. Revisit on an idle machine (the CPU path loses
more than the GPU path under load) or if RAM becomes the constraint.

Repeat: `uv run --with parakeet-mlx` bench script over a folder of 16 kHz wavs; CPU side
`asr_bench <model_dir> <concatenated.wav> vad` (segments separated by 3 s of silence).
