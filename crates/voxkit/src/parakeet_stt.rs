//! Local Parakeet TDT STT backend using parakeet-rs (ONNX Runtime).
//!
//! NVIDIA Parakeet TDT 0.6B — fast, accurate, offline speech-to-text.
//! ~5x faster than Whisper base on Apple Silicon, better accuracy.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use parakeet_rs::Transcriber;

use crate::stt::{StreamingSttBackend, SttError, SttInput, SttStreamError, TranscriptEvent};
use crate::types::{AudioChunk, Transcript};

/// Local Parakeet TDT STT backend.
pub struct ParakeetStt {
    model_dir: PathBuf,
    language: String,
    /// End-of-utterance pause: trailing silence that closes a segment.
    pause_ms: u32,
    /// Hard cap on one segment, so a speaker who never pauses still gets subtitles.
    max_segment_secs: f32,
    /// Speaker diarization: (embedding model path, max speakers).
    #[cfg(feature = "speaker")]
    diarize: Option<(PathBuf, usize)>,
}

impl ParakeetStt {
    /// Create a new Parakeet STT backend.
    ///
    /// `model_dir` must contain: encoder-model.onnx, decoder_joint-model.onnx, vocab.txt
    pub fn new(model_dir: PathBuf, language: &str) -> Self {
        Self {
            model_dir,
            language: language.to_string(),
            pause_ms: DEFAULT_PAUSE_MS,
            max_segment_secs: DEFAULT_MAX_SEGMENT_SECS,
            #[cfg(feature = "speaker")]
            diarize: None,
        }
    }

    /// Trailing silence (ms) that ends an utterance. Default 1200.
    pub fn with_pause_ms(mut self, pause_ms: u32) -> Self {
        self.pause_ms = pause_ms.max(200);
        self
    }

    /// Longest segment before a forced cut. Default 15 s.
    pub fn with_max_segment_secs(mut self, secs: f32) -> Self {
        self.max_segment_secs = secs.max(2.0);
        self
    }

    /// Label each segment with a speaker index (1..=max_speakers).
    #[cfg(feature = "speaker")]
    pub fn with_diarization(mut self, model_path: PathBuf, max_speakers: usize) -> Self {
        self.diarize = Some((model_path, max_speakers));
        self
    }

    /// Batch-transcribe an AudioChunk (one-shot).
    pub fn transcribe_file(
        model_dir: &Path,
        audio: &AudioChunk,
        language: &str,
    ) -> Result<Transcript, SttError> {
        let mut engine =
            parakeet_rs::ParakeetTDT::from_pretrained(model_dir.to_str().unwrap_or(""), None)
                .map_err(|e| SttError::Other(format!("Load Parakeet model: {e}")))?;

        let result = engine
            .transcribe_samples(
                audio.samples.clone(),
                audio.sample_rate,
                1, // mono
                Some(parakeet_rs::TimestampMode::Words),
            )
            .map_err(|e| SttError::Other(format!("Parakeet transcription failed: {e}")))?;

        let text = result.text.clone();
        if text.trim().is_empty() {
            return Err(SttError::Empty);
        }

        let segments = result
            .tokens
            .iter()
            .map(|t| crate::types::Segment {
                start: t.start as f64,
                end: t.end as f64,
                text: t.text.clone(),
                speaker: None,
            })
            .collect();

        Ok(Transcript {
            text,
            segments,
            language: Some(language.to_string()),
            duration_secs: audio.duration_ms as f64 / 1000.0,
        })
    }
}

#[async_trait::async_trait]
impl StreamingSttBackend for ParakeetStt {
    fn display_name(&self) -> &str {
        "parakeet-tdt"
    }

    async fn connect(
        &self,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), SttStreamError> {
        let model_dir = self.model_dir.clone();
        let _language = self.language.clone();
        let pause_frames = (self.pause_ms / FRAME_MS).max(1) as usize;
        let max_segment_samples = (self.max_segment_secs * SAMPLE_RATE as f32) as usize;

        // Load Parakeet model
        let mut engine =
            parakeet_rs::ParakeetTDT::from_pretrained(model_dir.to_str().unwrap_or(""), None)
                .map_err(|e| SttStreamError::Connection(format!("Load Parakeet model: {e}")))?;

        #[cfg(feature = "speaker")]
        let mut diarizer = match &self.diarize {
            Some((path, max)) => {
                let embedder = crate::speaker::SpeakerEmbedder::new(path)
                    .map_err(|e| SttStreamError::Connection(format!("Load speaker model: {e}")))?;
                Some((
                    embedder,
                    crate::speaker::SpeakerTracker::new(*max, SPEAKER_THRESHOLD),
                ))
            }
            None => None,
        };

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        tokio::task::spawn_blocking(move || {
            let mut seg = Segmenter::new(pause_frames, max_segment_samples);
            let mut item_counter = 0u64;

            let mut emit = |segment: Vec<f32>, item_counter: &mut u64| {
                *item_counter += 1;
                let item_id = format!("parakeet_{item_counter}");
                #[allow(unused_mut)]
                let mut speaker: Option<usize> = None;
                #[cfg(feature = "speaker")]
                if let Some((embedder, tracker)) = diarizer.as_mut() {
                    if segment.len() as f32 / SAMPLE_RATE as f32 >= crate::speaker::MIN_EMBED_SECS {
                        match embedder.embed(&segment) {
                            Ok(e) => speaker = Some(tracker.assign(&e)),
                            Err(e) => tracing::warn!("speaker embedding failed: {e}"),
                        }
                    }
                    if speaker.is_none() {
                        speaker = tracker.last();
                    }
                }
                match engine.transcribe_samples(segment, SAMPLE_RATE, 1, None) {
                    Ok(result) if !result.text.trim().is_empty() => {
                        let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                            item_id,
                            text: result.text,
                            speaker,
                        });
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = transcript_tx
                            .blocking_send(TranscriptEvent::Error(format!("Parakeet: {e}")));
                    }
                }
            };

            while let Some(input) = input_rx.blocking_recv() {
                match input {
                    SttInput::Audio(samples) => {
                        let f32_samples: Vec<f32> =
                            samples.iter().map(|&s| s as f32 / 32768.0).collect();
                        // Realtime input arrives at 24 kHz; Parakeet wants 16 kHz.
                        let resampled = crate::types::resample(&f32_samples, 24000, SAMPLE_RATE);
                        for segment in seg.push(&resampled) {
                            emit(segment, &mut item_counter);
                        }
                    }
                    SttInput::UpdatePrompt(_) => {}
                    SttInput::Close => {
                        if let Some(segment) = seg.flush() {
                            emit(segment, &mut item_counter);
                        }
                        break;
                    }
                }
            }
            tracing::debug!("Parakeet STT stream closed ({item_counter} segments)");
        });

        Ok((input_tx, transcript_rx))
    }
}

/// Parakeet input sample rate.
const SAMPLE_RATE: u32 = 16000;
/// Energy-VAD frame length.
const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (SAMPLE_RATE / 1000 * FRAME_MS) as usize;
/// Default end-of-utterance pause.
pub const DEFAULT_PAUSE_MS: u32 = 1200;
/// Default forced-cut length.
pub const DEFAULT_MAX_SEGMENT_SECS: f32 = 15.0;
/// Pre-roll kept before the first speech frame (so onsets are not clipped).
const PRE_ROLL_FRAMES: usize = 15; // 300 ms
/// Silence kept after the last speech frame inside an emitted segment.
const TAIL_FRAMES: usize = 15; // 300 ms
/// Cosine threshold for joining an existing speaker cluster.
#[cfg(feature = "speaker")]
const SPEAKER_THRESHOLD: f32 = 0.5;

/// Energy-based utterance segmenter: cuts on a pause, not on a timer.
///
/// Keeps a short pre-roll of silence before speech, drops long stretches of
/// pure silence (Parakeet hallucinates on them), and force-cuts a segment that
/// exceeds `max_samples`.
struct Segmenter {
    pause_frames: usize,
    max_samples: usize,
    buf: Vec<f32>,
    partial: Vec<f32>,
    has_speech: bool,
    trailing_silence: usize,
}

impl Segmenter {
    fn new(pause_frames: usize, max_samples: usize) -> Self {
        Self {
            pause_frames,
            max_samples,
            buf: Vec::new(),
            partial: Vec::new(),
            has_speech: false,
            trailing_silence: 0,
        }
    }

    /// Feed 16 kHz samples; returns zero or more finished utterances.
    fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        self.partial.extend_from_slice(samples);
        let mut offset = 0;
        while self.partial.len() - offset >= FRAME_SAMPLES {
            let frame = &self.partial[offset..offset + FRAME_SAMPLES];
            offset += FRAME_SAMPLES;
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
            let is_speech = rms >= SILENCE_THRESHOLD;
            self.buf.extend_from_slice(frame);
            if is_speech {
                self.has_speech = true;
                self.trailing_silence = 0;
            } else {
                self.trailing_silence += 1;
            }
            if !self.has_speech {
                // Pure silence so far: keep only the pre-roll.
                let keep = PRE_ROLL_FRAMES * FRAME_SAMPLES;
                if self.buf.len() > keep {
                    self.buf.drain(..self.buf.len() - keep);
                }
                continue;
            }
            if self.trailing_silence >= self.pause_frames {
                // Utterance ended: emit speech + a short tail, keep the rest as pre-roll.
                let drop_tail = (self.trailing_silence - TAIL_FRAMES.min(self.trailing_silence))
                    * FRAME_SAMPLES;
                let cut = self.buf.len().saturating_sub(drop_tail);
                let segment: Vec<f32> = self.buf.drain(..cut).collect();
                let keep = PRE_ROLL_FRAMES * FRAME_SAMPLES;
                if self.buf.len() > keep {
                    self.buf.drain(..self.buf.len() - keep);
                }
                self.has_speech = false;
                self.trailing_silence = 0;
                out.push(segment);
            } else if self.buf.len() >= self.max_samples {
                out.push(std::mem::take(&mut self.buf));
                self.has_speech = false;
                self.trailing_silence = 0;
            }
        }
        self.partial.drain(..offset);
        out
    }

    /// Return whatever speech is still buffered (on stop).
    fn flush(&mut self) -> Option<Vec<f32>> {
        if !self.has_speech || self.buf.len() < FRAME_SAMPLES * 10 {
            self.buf.clear();
            return None;
        }
        self.has_speech = false;
        self.trailing_silence = 0;
        Some(std::mem::take(&mut self.buf))
    }
}

/// RMS threshold below which audio is considered silence.
/// Parakeet hallucinates on silence — must skip quiet chunks.
const SILENCE_THRESHOLD: f32 = 0.005;

/// Check that parakeet model files exist in the given directory.
pub fn model_exists(model_dir: &Path) -> bool {
    model_dir.join("encoder-model.onnx").exists()
        && model_dir.join("decoder_joint-model.onnx").exists()
        && model_dir.join("vocab.txt").exists()
}

/// Default model directory: ~/.supervox/models/parakeet-tdt/
pub fn default_model_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".supervox/models/parakeet-tdt"))
        .unwrap_or_else(|| PathBuf::from("models/parakeet-tdt"))
}

#[cfg(test)]
mod segmenter_tests {
    use super::*;

    fn tone(secs: f32) -> Vec<f32> {
        let n = (secs * SAMPLE_RATE as f32) as usize;
        (0..n).map(|i| 0.3 * ((i as f32) * 0.05).sin()).collect()
    }
    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * SAMPLE_RATE as f32) as usize]
    }

    #[test]
    fn pause_closes_an_utterance_and_timer_does_not() {
        // 1.2 s pause, 15 s cap. 3 s of speech must NOT be cut at 2 s.
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        let mut out = seg.push(&tone(3.0));
        assert!(out.is_empty(), "no cut while speech continues");
        out.extend(seg.push(&silence(1.5)));
        assert_eq!(out.len(), 1, "one utterance after the pause");
        let secs = out[0].len() as f32 / SAMPLE_RATE as f32;
        assert!(
            secs > 3.0 && secs < 3.7,
            "speech + ~300 ms tail, got {secs}"
        );
    }

    #[test]
    fn pure_silence_is_dropped_not_emitted() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        assert!(seg.push(&silence(10.0)).is_empty());
        assert!(seg.flush().is_none());
        assert!(seg.buf.len() <= PRE_ROLL_FRAMES * FRAME_SAMPLES);
    }

    #[test]
    fn long_monologue_is_force_cut_at_cap() {
        let mut seg = Segmenter::new(60, 4 * SAMPLE_RATE as usize);
        let out = seg.push(&tone(9.0));
        assert_eq!(out.len(), 2, "two forced cuts in 9 s at a 4 s cap");
    }

    #[test]
    fn flush_returns_trailing_speech() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        assert!(seg.push(&tone(1.0)).is_empty());
        assert!(seg.flush().is_some());
    }
}
