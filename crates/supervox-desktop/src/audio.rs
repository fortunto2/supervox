//! Audio pipeline for desktop app — mic capture + STT + level events.

use tokio::sync::{mpsc, watch};
use voxkit::mic::MicCapture;
use voxkit::stt::{StreamingSttBackend, SttInput, TranscriptEvent};

/// Audio events sent to the UI.
#[derive(Clone, Debug)]
pub enum AudioEvent {
    MicLevel(f32),
    Transcript { text: String, is_final: bool },
    Error(String),
    Stopped,
}

/// Manages mic capture → STT pipeline.
pub struct AudioPipeline {
    stop_tx: Option<watch::Sender<bool>>,
    #[allow(dead_code)]
    mic_capture: Option<MicCapture>,
}

impl AudioPipeline {
    pub fn new() -> Self {
        Self {
            stop_tx: None,
            mic_capture: None,
        }
    }

    /// Start mic capture and STT. Returns channel of AudioEvents.
    pub fn start(
        &mut self,
        config: &supervox_agent::types::Config,
    ) -> Result<mpsc::UnboundedReceiver<AudioEvent>, String> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        // Start mic (MicCapture is !Send, keep handle here)
        let (mic_rx, mic_capture) =
            MicCapture::start_raw().map_err(|e| format!("Mic error: {e}"))?;
        self.mic_capture = Some(mic_capture);

        // Create STT backend
        let stt_backend = create_stt_backend(config)?;

        // Spawn pipeline task (mic_rx is Send, mic_capture stays in AudioPipeline)
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            run_pipeline(mic_rx, stt_backend, event_tx_clone, stop_rx).await;
        });

        Ok(event_rx)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(mic) = self.mic_capture.take() {
            mic.stop();
        }
    }
}

async fn run_pipeline(
    mut mic_rx: mpsc::Receiver<voxkit::types::AudioChunk>,
    stt: Box<dyn StreamingSttBackend>,
    event_tx: mpsc::UnboundedSender<AudioEvent>,
    mut stop_rx: watch::Receiver<bool>,
) {
    // Connect STT
    let (stt_tx, mut stt_rx) = match stt.connect().await {
        Ok(pair) => pair,
        Err(e) => {
            let _ = event_tx.send(AudioEvent::Error(format!("STT connect: {e}")));
            return;
        }
    };

    loop {
        tokio::select! {
            // Stop signal
            _ = stop_rx.changed() => {
                let _ = stt_tx.send(SttInput::Close).await;
                let _ = event_tx.send(AudioEvent::Stopped);
                break;
            }
            // Mic chunks
            chunk = mic_rx.recv() => {
                match chunk {
                    Some(chunk) => {
                        let level = chunk.rms().min(1.0);
                        let _ = event_tx.send(AudioEvent::MicLevel(level));

                        // Resample to 24kHz and convert to i16 for STT
                        let resampled = voxkit::realtime_stt::resample_to_24k(
                            &chunk.samples, chunk.sample_rate,
                        );
                        let _ = stt_tx.send(SttInput::Audio(resampled)).await;
                    }
                    None => break,
                }
            }
            // STT events
            event = stt_rx.recv() => {
                match event {
                    Some(TranscriptEvent::Delta { text, .. }) => {
                        let _ = event_tx.send(AudioEvent::Transcript { text, is_final: false });
                    }
                    Some(TranscriptEvent::Final { text, .. }) => {
                        let _ = event_tx.send(AudioEvent::Transcript { text, is_final: true });
                    }
                    Some(TranscriptEvent::Error(e)) => {
                        tracing::warn!("STT error: {e}");
                    }
                    None => break,
                }
            }
        }
    }
}

fn create_stt_backend(
    config: &supervox_agent::types::Config,
) -> Result<Box<dyn StreamingSttBackend>, String> {
    use supervox_agent::types::SttBackend;

    let backend = match std::env::var("SUPERVOX_STT_BACKEND").ok().as_deref() {
        Some("whisper") => SttBackend::Whisper,
        Some("realtime") => SttBackend::Realtime,
        Some("parakeet") => SttBackend::Parakeet,
        _ => config.stt_backend.clone(),
    };

    match backend {
        SttBackend::Realtime => {
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| "OPENAI_API_KEY not set".to_string())?;
            let stt_config = voxkit::realtime_stt::StreamingSttConfig::new(&api_key);
            Ok(Box::new(voxkit::realtime_stt::OpenAiStreamingStt::new(
                stt_config,
            )))
        }
        #[cfg(feature = "whisper")]
        SttBackend::Whisper => {
            let models_dir = supervox_agent::storage::data_dir().join("models");
            let model_path = models_dir.join(format!("ggml-{}.bin", config.whisper_model));
            if !model_path.exists() {
                return Err(format!("Whisper model not found: {}", model_path.display()));
            }
            Ok(Box::new(voxkit::whisper_stt::WhisperStt::new(
                model_path,
                &config.my_language,
            )))
        }
        #[cfg(feature = "parakeet")]
        SttBackend::Parakeet => {
            let model_dir = voxkit::parakeet_stt::default_model_dir();
            let dir = if voxkit::parakeet_stt::model_exists(&model_dir) {
                model_dir
            } else {
                let alt = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join("startups/active/life2film/video-analyzer/models/parakeet-tdt");
                if voxkit::parakeet_stt::model_exists(&alt) {
                    alt
                } else {
                    return Err("Parakeet model not found".into());
                }
            };
            Ok(Box::new(voxkit::parakeet_stt::ParakeetStt::new(
                dir,
                &config.my_language,
            )))
        }
        #[allow(unreachable_patterns)]
        _ => Err(format!("STT backend {backend} not compiled in")),
    }
}
