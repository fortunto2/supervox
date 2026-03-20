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
}

impl ParakeetStt {
    /// Create a new Parakeet STT backend.
    ///
    /// `model_dir` must contain: encoder-model.onnx, decoder_joint-model.onnx, vocab.txt
    pub fn new(model_dir: PathBuf, language: &str) -> Self {
        Self {
            model_dir,
            language: language.to_string(),
        }
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

        // Load Parakeet model
        let mut engine =
            parakeet_rs::ParakeetTDT::from_pretrained(model_dir.to_str().unwrap_or(""), None)
                .map_err(|e| SttStreamError::Connection(format!("Load Parakeet model: {e}")))?;

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        // Spawn processing task
        tokio::task::spawn_blocking(move || {
            let mut audio_buffer: Vec<f32> = Vec::new();
            let mut item_counter = 0u64;

            while let Some(input) = input_rx.blocking_recv() {
                match input {
                    SttInput::Audio(samples) => {
                        // Convert i16 to f32 and accumulate
                        let f32_samples: Vec<f32> =
                            samples.iter().map(|&s| s as f32 / 32768.0).collect();
                        // Resample from 24kHz (realtime input) to 16kHz (Parakeet expects)
                        let resampled = crate::types::resample(&f32_samples, 24000, 16000);
                        audio_buffer.extend_from_slice(&resampled);

                        // Transcribe every ~2 seconds of audio (32000 samples at 16kHz)
                        if audio_buffer.len() >= 32000 {
                            let segment = std::mem::take(&mut audio_buffer);

                            // Skip silent chunks — Parakeet hallucinates on silence
                            let rms = (segment.iter().map(|s| s * s).sum::<f32>()
                                / segment.len() as f32)
                                .sqrt();
                            if rms < SILENCE_THRESHOLD {
                                tracing::debug!("Parakeet: skipping silent chunk (RMS={rms:.4})");
                                continue;
                            }

                            item_counter += 1;
                            let item_id = format!("parakeet_{item_counter}");

                            match engine.transcribe_samples(segment, 16000, 1, None) {
                                Ok(result) if !result.text.trim().is_empty() => {
                                    let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                                        item_id,
                                        text: result.text,
                                    });
                                }
                                Ok(_) => {} // silence
                                Err(e) => {
                                    let _ = transcript_tx.blocking_send(TranscriptEvent::Error(
                                        format!("Parakeet: {e}"),
                                    ));
                                }
                            }
                        }
                    }
                    SttInput::UpdatePrompt(_) => {} // no-op for Parakeet
                    SttInput::Close => {
                        // Flush remaining audio
                        if audio_buffer.len() >= 1600 {
                            let segment = std::mem::take(&mut audio_buffer);
                            let rms = (segment.iter().map(|s| s * s).sum::<f32>()
                                / segment.len() as f32)
                                .sqrt();
                            if rms < SILENCE_THRESHOLD {
                                break;
                            }
                            item_counter += 1;
                            let item_id = format!("parakeet_{item_counter}");
                            if let Ok(result) = engine.transcribe_samples(segment, 16000, 1, None)
                                && !result.text.trim().is_empty()
                            {
                                let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                                    item_id,
                                    text: result.text,
                                });
                            }
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
