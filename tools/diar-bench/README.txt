Diarization benchmark, 19.11.2026 (room recording, 48 min, 4-6 people + loudspeaker AI).
Pipeline: FluidAudio `transcribe --word-timestamps` -> align3.py (stream.jsonl lines to WAV time;
live arrival times ran 100-170 s behind the audio) -> eval.py (needs sim_probe output, see
crates/voxkit/examples/sim_probe.rs). gt_final.tsv: 19 hand-labelled lines (content) kept only
where the CAM++ voiceprint agrees. Ground truth for owner recognition is the voiceprint itself:
4 s windows with raw cosine >= 0.60 are Rustam, <= 0.20 with speech are not.
