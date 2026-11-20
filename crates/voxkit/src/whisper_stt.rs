//! Local Whisper STT backend using whisper-rs (whisper.cpp) + Silero VAD.
//!
//! Provides offline, privacy-first speech-to-text with Metal acceleration on Apple Silicon.
//! Audio is segmented by VAD, then batch-transcribed by Whisper per segment.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::segmenter::{
    DEFAULT_MAX_SEGMENT_SECS, DEFAULT_PAUSE_MS, FRAME_MS, SAMPLE_RATE, Segmenter, turn_finished,
};
use crate::stt::{StreamingSttBackend, SttError, SttInput, SttStreamError, TranscriptEvent};
use crate::types::{AudioChunk, Transcript};

/// Local Whisper STT backend.
pub struct WhisperStt {
    model_path: PathBuf,
    language: String,
    pause_ms: u32,
    max_segment_secs: f32,
    /// smart-turn model and the early pause (ms) at which it is asked whether the phrase is over.
    smart_turn: Option<(PathBuf, u32)>,
}

impl WhisperStt {
    /// Create a new Whisper STT backend.
    ///
    /// `model_path` must point to a GGML model file (e.g., `ggml-base.bin`).
    pub fn new(model_path: PathBuf, language: &str) -> Self {
        Self {
            model_path,
            language: language.to_string(),
            pause_ms: DEFAULT_PAUSE_MS,
            max_segment_secs: DEFAULT_MAX_SEGMENT_SECS,
            smart_turn: None,
        }
    }

    /// Trailing silence (ms) that ends an utterance. Default 1200.
    pub fn with_pause_ms(mut self, pause_ms: u32) -> Self {
        self.pause_ms = pause_ms.max(200);
        self
    }

    /// End segments on semantic end-of-phrase: after `early_ms` of silence ask smart-turn.
    pub fn with_smart_turn(mut self, model: PathBuf, early_ms: u32) -> Self {
        self.smart_turn = Some((model, early_ms.max(100)));
        self
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
        // Hallucination guard for quiet or empty chunks (whisper "thanks for watching" in silence).
        params.set_no_speech_thold(0.6);
        params.set_logprob_thold(-1.0);
        params.set_suppress_nst(true);
        params.set_n_threads(8);

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
        if trimmed.is_empty() || is_hallucination(&trimmed) {
            return Err(SttError::Empty);
        }
        Ok(trimmed)
    }

    /// Batch-transcribe a full AudioChunk (not streaming).
    ///
    /// Resamples to 16kHz internally if needed. Returns Transcript with text + duration.
    pub fn transcribe_chunk(
        ctx: &WhisperContext,
        audio: &AudioChunk,
        language: &str,
    ) -> Result<Transcript, SttError> {
        let samples = if audio.sample_rate != 16000 {
            resample_to_16k(&audio.samples, audio.sample_rate)
        } else {
            audio.samples.clone()
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(true);
        params.set_suppress_blank(true);

        let mut state = ctx
            .create_state()
            .map_err(|e| SttError::Other(format!("Whisper state: {e}")))?;

        state
            .full(params, &samples)
            .map_err(|e| SttError::Other(format!("Whisper transcribe: {e}")))?;

        let n_segments = state.full_n_segments();

        let mut full_text = String::new();
        let mut segments = Vec::new();

        for i in 0..n_segments {
            if let Some(segment) = state.get_segment(i)
                && let Ok(segment_text) = segment.to_str()
            {
                let text = segment_text.trim().to_string();
                if !text.is_empty() {
                    full_text.push_str(&text);
                    full_text.push(' ');
                    segments.push(crate::types::Segment {
                        start: segment.start_timestamp() as f64 / 100.0,
                        end: segment.end_timestamp() as f64 / 100.0,
                        text,
                        speaker: None,
                    });
                }
            }
        }

        let trimmed = full_text.trim().to_string();
        if trimmed.is_empty() {
            return Err(SttError::Empty);
        }

        let duration_secs = audio.duration_ms as f64 / 1000.0;

        Ok(Transcript {
            text: trimmed,
            segments,
            language: Some(language.to_string()),
            duration_secs,
        })
    }

    /// Convenience: load model from path, transcribe an AudioChunk, return Transcript.
    ///
    /// One-shot batch transcription — loads context, transcribes, done.
    pub fn transcribe_file(
        model_path: &Path,
        audio: &AudioChunk,
        language: &str,
    ) -> Result<Transcript, SttError> {
        // whisper.cpp / GGML print to stderr by default, which tears the TUI apart;
        // route them into the (silent) logging hooks instead. Idempotent.
        whisper_rs::install_logging_hooks();
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or(""),
            WhisperContextParameters::default(),
        )
        .map_err(|e| SttError::Other(format!("Load Whisper model: {e}")))?;

        Self::transcribe_chunk(&ctx, audio, language)
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
/// Peak (linear) below which a 2 s chunk is treated as silence and never sent to Whisper.
const SILENCE_PEAK: f32 = 0.004;

/// Stock phrases Whisper emits on silence or line noise (trained on subtitles).
const HALLUCINATIONS: &[&str] = &[
    "izlediğiniz için teşekkür",
    "altyazı m.k",
    "abone ol",
    "редактор субтитров",
    "подписывайтесь на канал",
    "продолжение следует",
    "thanks for watching",
    "thank you for watching",
];

fn is_hallucination(text: &str) -> bool {
    let t = text.to_lowercase();
    HALLUCINATIONS.iter().any(|h| t.contains(h))
}

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
        // whisper.cpp / GGML print to stderr by default, which tears the TUI apart;
        // route them into the (silent) logging hooks instead. Idempotent.
        whisper_rs::install_logging_hooks();
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or(""),
            WhisperContextParameters::default(),
        )
        .map_err(|e| SttStreamError::Connection(format!("Load Whisper model: {e}")))?;

        let pause_frames = (self.pause_ms / FRAME_MS).max(1) as usize;
        let max_segment_samples = (self.max_segment_secs * SAMPLE_RATE as f32) as usize;
        let mut turn_model = self.smart_turn.as_ref().and_then(|(path, _)| {
            crate::smart_turn::SmartTurn::new(path)
                .map_err(|e| tracing::warn!("smart-turn off, fixed pause only: {e}"))
                .ok()
        });
        let early_frames = match (&turn_model, &self.smart_turn) {
            (Some(_), Some((_, ms))) => (*ms / FRAME_MS).max(1) as usize,
            _ => 0,
        };

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        tokio::task::spawn_blocking(move || {
            // Same segmenter as Parakeet: a pause closes a phrase, and smart-turn closes it early.
            let mut seg =
                Segmenter::new(pause_frames, max_segment_samples).with_early_check(early_frames);
            let mut item_counter = 0u64;
            // Phone and far-field audio arrives at -25 to -45 dBFS; Whisper hallucinates on it.
            let mut agc = crate::agc::Agc::default();
            // Loudest raw sample since the last phrase closed: the silence gate.
            let mut raw_peak = 0f32;
            let mut f32_samples: Vec<f32> = Vec::new();
            let mut resampled: Vec<f32> = Vec::new();

            let mut emit = |segment: Vec<f32>, span: (f32, f32), raw_peak: &mut f32| {
                let quiet = *raw_peak < SILENCE_PEAK;
                *raw_peak = 0.0;
                if quiet {
                    return;
                }
                item_counter += 1;
                let item_id = format!("whisper_{item_counter}");
                match WhisperStt::transcribe_segment(&ctx, &segment, &language) {
                    Ok(text) => {
                        let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                            item_id,
                            text,
                            speaker: None,
                            span: Some(span),
                            embedding: None,
                        });
                    }
                    Err(SttError::Empty) => {} // silence, skip
                    Err(e) => {
                        let _ = transcript_tx
                            .blocking_send(TranscriptEvent::Error(format!("Whisper: {e}")));
                    }
                }
            };

            while let Some(input) = input_rx.blocking_recv() {
                match input {
                    SttInput::Audio(samples) => {
                        f32_samples.clear();
                        f32_samples.extend(samples.iter().map(|&s| s as f32 / 32768.0));
                        crate::types::resample_into(
                            &f32_samples,
                            24000,
                            SAMPLE_RATE,
                            &mut resampled,
                        );
                        raw_peak = resampled.iter().fold(raw_peak, |m, v| m.max(v.abs()));
                        agc.process(&mut resampled);
                        let mut check = |buf: &[f32]| turn_finished(turn_model.as_mut(), buf);
                        let closed = seg.push_with(&resampled, &mut check);
                        let spans = seg.take_spans();
                        for (segment, span) in closed.into_iter().zip(spans) {
                            emit(segment, span, &mut raw_peak);
                        }
                    }
                    SttInput::UpdatePrompt(_) => {} // Not supported for local Whisper
                    SttInput::Close => {
                        if let Some(segment) = seg.flush() {
                            let span = seg.take_spans().pop().unwrap_or_default();
                            emit(segment, span, &mut raw_peak);
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
    fn hallucination_phrases_are_dropped() {
        assert!(is_hallucination("Bu videoyu izlediğiniz için teşekkürler"));
        assert!(!is_hallucination("SGK için teşekkürler, tamam"));
    }

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
