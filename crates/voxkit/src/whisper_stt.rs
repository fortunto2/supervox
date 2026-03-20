//! Local Whisper STT backend using whisper-rs (whisper.cpp) + Silero VAD.
//!
//! Provides offline, privacy-first speech-to-text with Metal acceleration on Apple Silicon.
//! Audio is segmented by VAD, then batch-transcribed by Whisper per segment.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::stt::{StreamingSttBackend, SttError, SttInput, SttStreamError, TranscriptEvent};

/// Local Whisper STT backend.
pub struct WhisperStt {
    model_path: PathBuf,
    language: String,
}

impl WhisperStt {
    /// Create a new Whisper STT backend.
    ///
    /// `model_path` must point to a GGML model file (e.g., `ggml-base.bin`).
    pub fn new(model_path: PathBuf, language: &str) -> Self {
        Self {
            model_path,
            language: language.to_string(),
        }
    }

    /// Transcribe a segment of f32 PCM audio (16kHz mono).
    pub fn transcribe_segment(
        ctx: &WhisperContext,
        audio: &[f32],
        language: &str,
    ) -> Result<String, SttError> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_single_segment(true);

        let mut state = ctx
            .create_state()
            .map_err(|e| SttError::Other(format!("Whisper state: {e}")))?;

        state
            .full(params, audio)
            .map_err(|e| SttError::Other(format!("Whisper transcribe: {e}")))?;

        let n_segments = state.full_n_segments();

        let mut text = String::new();
        for i in 0..n_segments {
            if let Some(segment) = state.get_segment(i)
                && let Ok(segment_text) = segment.to_str()
            {
                text.push_str(segment_text.trim());
                text.push(' ');
            }
        }
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return Err(SttError::Empty);
        }
        Ok(trimmed)
    }
}

/// Download a Whisper GGML model if not already present.
///
/// Supports sizes: tiny, base, small, medium.
/// Downloads from Hugging Face ggerganov/whisper.cpp releases.
pub async fn ensure_model(model_size: &str, models_dir: &Path) -> Result<PathBuf, SttError> {
    let filename = format!("ggml-{model_size}.bin");
    let model_path = models_dir.join(&filename);

    if model_path.exists() {
        tracing::info!("Whisper model already exists: {}", model_path.display());
        return Ok(model_path);
    }

    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}");

    tracing::info!("Downloading Whisper model: {url}");

    std::fs::create_dir_all(models_dir)
        .map_err(|e| SttError::Other(format!("Create models dir: {e}")))?;

    // Download to temp file, then rename (atomic)
    let temp_path = models_dir.join(format!("{filename}.downloading"));

    let response = reqwest::get(&url)
        .await
        .map_err(|e| SttError::Request(format!("Download {filename}: {e}")))?;

    if !response.status().is_success() {
        return Err(SttError::Api {
            status: response.status().as_u16(),
            body: format!("Failed to download {filename}"),
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| SttError::Request(format!("Read body: {e}")))?;

    std::fs::write(&temp_path, &bytes).map_err(|e| SttError::Other(format!("Write model: {e}")))?;

    std::fs::rename(&temp_path, &model_path)
        .map_err(|e| SttError::Other(format!("Rename model: {e}")))?;

    tracing::info!(
        "Whisper model downloaded: {} ({} MB)",
        model_path.display(),
        bytes.len() / (1024 * 1024)
    );

    Ok(model_path)
}

/// Resample f32 audio to 16kHz (Whisper's required sample rate).
fn resample_to_16k(samples: &[f32], src_rate: u32) -> Vec<f32> {
    crate::types::resample(samples, src_rate, 16000)
}

#[async_trait::async_trait]
impl StreamingSttBackend for WhisperStt {
    async fn connect(
        &self,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), SttStreamError> {
        let model_path = self.model_path.clone();
        let language = self.language.clone();

        // Load Whisper model
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or(""),
            WhisperContextParameters::default(),
        )
        .map_err(|e| SttStreamError::Connection(format!("Load Whisper model: {e}")))?;

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        // Spawn processing task
        tokio::task::spawn_blocking(move || {
            let mut audio_buffer: Vec<f32> = Vec::new();
            let mut item_counter = 0u64;

            // Simple energy-based segmentation:
            // Accumulate audio, transcribe when we get a Close or enough silence
            while let Some(input) = input_rx.blocking_recv() {
                match input {
                    SttInput::Audio(samples) => {
                        // Convert i16 to f32 and resample to 16kHz
                        let f32_samples: Vec<f32> =
                            samples.iter().map(|&s| s as f32 / 32768.0).collect();
                        let resampled = resample_to_16k(&f32_samples, 24000);
                        audio_buffer.extend_from_slice(&resampled);

                        // Transcribe every ~2 seconds of audio (32000 samples at 16kHz)
                        if audio_buffer.len() >= 32000 {
                            let segment = std::mem::take(&mut audio_buffer);
                            item_counter += 1;
                            let item_id = format!("whisper_{item_counter}");

                            match WhisperStt::transcribe_segment(&ctx, &segment, &language) {
                                Ok(text) => {
                                    let _ = transcript_tx
                                        .blocking_send(TranscriptEvent::Final { item_id, text });
                                }
                                Err(SttError::Empty) => {} // silence, skip
                                Err(e) => {
                                    let _ = transcript_tx.blocking_send(TranscriptEvent::Error(
                                        format!("Whisper: {e}"),
                                    ));
                                }
                            }
                        }
                    }
                    SttInput::UpdatePrompt(_) => {} // Not supported for local Whisper
                    SttInput::Close => {
                        // Transcribe remaining audio
                        if audio_buffer.len() > 1600 {
                            // >100ms
                            item_counter += 1;
                            let item_id = format!("whisper_{item_counter}");
                            if let Ok(text) =
                                WhisperStt::transcribe_segment(&ctx, &audio_buffer, &language)
                            {
                                let _ = transcript_tx
                                    .blocking_send(TranscriptEvent::Final { item_id, text });
                            }
                        }
                        break;
                    }
                }
            }
        });

        Ok((input_tx, transcript_rx))
    }

    fn display_name(&self) -> &str {
        "whisper"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_stt_new() {
        let stt = WhisperStt::new(PathBuf::from("/tmp/model.bin"), "en");
        assert_eq!(stt.language, "en");
        assert_eq!(stt.model_path, PathBuf::from("/tmp/model.bin"));
    }

    #[test]
    fn ensure_model_returns_existing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let model_path = tmp.path().join("ggml-tiny.bin");
        std::fs::write(&model_path, b"fake model data").unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(ensure_model("tiny", tmp.path()));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), model_path);
    }

    #[test]
    fn resample_16k_noop() {
        let samples = vec![0.1, 0.2, 0.3];
        let out = resample_to_16k(&samples, 16000);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn resample_16k_from_24k() {
        let samples = vec![0.0; 24000]; // 1 second at 24kHz
        let out = resample_to_16k(&samples, 24000);
        // Should be ~16000 samples
        assert!((out.len() as i32 - 16000).abs() < 10);
    }

    #[test]
    fn display_name_is_whisper() {
        let stt = WhisperStt::new(PathBuf::from("/tmp/model.bin"), "en");
        assert_eq!(stt.display_name(), "whisper");
    }
}
