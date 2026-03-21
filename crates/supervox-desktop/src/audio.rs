//! Audio pipeline for desktop app — mic + system audio capture + STT + summary.

use std::time::Duration;
use tokio::sync::{mpsc, watch};
use voxkit::mic::MicCapture;
use voxkit::stt::{StreamingSttBackend, SttInput, TranscriptEvent};

/// Audio events sent to the UI.
#[derive(Clone, Debug)]
pub enum AudioEvent {
    MicLevel(f32),
    SystemLevel(f32),
    Transcript { text: String, is_final: bool },
    Summary(String),
    Error(String),
    Stopped,
}

/// Manages mic + system audio capture → STT → summary pipeline.
pub struct AudioPipeline {
    stop_tx: Option<watch::Sender<bool>>,
    #[allow(dead_code)]
    mic_capture: Option<MicCapture>,
    #[allow(dead_code)]
    system_capture: Option<voxkit::system_audio::SystemAudioCapture>,
}

impl AudioPipeline {
    pub fn new() -> Self {
        Self {
            stop_tx: None,
            mic_capture: None,
            system_capture: None,
        }
    }

    /// Start mic + system audio capture and STT. Returns channel of AudioEvents.
    pub fn start(
        &mut self,
        config: &supervox_agent::types::Config,
    ) -> Result<mpsc::UnboundedReceiver<AudioEvent>, String> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        // Start mic
        let (mic_rx, mic_capture) =
            MicCapture::start_raw().map_err(|e| format!("Mic error: {e}"))?;
        self.mic_capture = Some(mic_capture);

        // Start system audio (optional)
        let system_rx = if config.capture.includes_system() {
            match voxkit::system_audio::SystemAudioCapture::start_raw() {
                Ok((rx, capture)) => {
                    self.system_capture = Some(capture);
                    Some(rx)
                }
                Err(e) => {
                    tracing::warn!("System audio unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Create STT backend
        let stt_backend = create_stt_backend(config)?;

        // Summary config
        let summary_lang = config.my_language.clone();
        let summary_model = config.effective_model().to_string();
        let summary_lag = config.summary_lag_secs;

        // Spawn pipeline task
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            run_pipeline(
                mic_rx,
                system_rx,
                stt_backend,
                event_tx_clone,
                stop_rx,
                summary_lang,
                summary_model,
                summary_lag,
            )
            .await;
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
        if let Some(sys) = self.system_capture.take() {
            sys.stop();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_pipeline(
    mut mic_rx: mpsc::Receiver<voxkit::types::AudioChunk>,
    mut system_rx: Option<mpsc::Receiver<voxkit::types::AudioChunk>>,
    stt: Box<dyn StreamingSttBackend>,
    event_tx: mpsc::UnboundedSender<AudioEvent>,
    mut stop_rx: watch::Receiver<bool>,
    summary_lang: String,
    summary_model: String,
    summary_lag: u32,
) {
    // Connect STT
    let (stt_tx, mut stt_rx) = match stt.connect().await {
        Ok(pair) => pair,
        Err(e) => {
            let _ = event_tx.send(AudioEvent::Error(format!("STT connect: {e}")));
            return;
        }
    };

    // Summary state
    let mut summary_chunks: Vec<String> = Vec::new();
    let mut summary_timer = tokio::time::interval(Duration::from_secs(summary_lag as u64));
    summary_timer.tick().await; // skip first

    loop {
        // Helper: recv from optional system_rx
        let sys_recv = async {
            match &mut system_rx {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        };

        tokio::select! {
            _ = stop_rx.changed() => {
                let _ = stt_tx.send(SttInput::Close).await;
                let _ = event_tx.send(AudioEvent::Stopped);
                break;
            }
            chunk = mic_rx.recv() => {
                if let Some(chunk) = chunk {
                    let level = chunk.rms().min(1.0);
                    let _ = event_tx.send(AudioEvent::MicLevel(level));
                    let resampled = voxkit::realtime_stt::resample_to_24k(
                        &chunk.samples, chunk.sample_rate,
                    );
                    let _ = stt_tx.send(SttInput::Audio(resampled)).await;
                } else {
                    break;
                }
            }
            chunk = sys_recv => {
                if let Some(chunk) = chunk {
                    let level = chunk.rms().min(1.0);
                    let _ = event_tx.send(AudioEvent::SystemLevel(level));
                }
            }
            event = stt_rx.recv() => {
                match event {
                    Some(TranscriptEvent::Delta { text, .. }) => {
                        let _ = event_tx.send(AudioEvent::Transcript { text, is_final: false });
                    }
                    Some(TranscriptEvent::Final { text, .. }) => {
                        summary_chunks.push(text.clone());
                        let _ = event_tx.send(AudioEvent::Transcript { text, is_final: true });
                    }
                    Some(TranscriptEvent::Error(e)) => {
                        tracing::warn!("STT error: {e}");
                    }
                    None => break,
                }
            }
            _ = summary_timer.tick() => {
                if !summary_chunks.is_empty() {
                    let chunks = std::mem::take(&mut summary_chunks);
                    let lang = summary_lang.clone();
                    let model = summary_model.clone();
                    let tx = event_tx.clone();
                    tokio::spawn(async move {
                        match generate_summary(&chunks, None, &lang, &model).await {
                            Ok(s) => { let _ = tx.send(AudioEvent::Summary(s)); }
                            Err(e) => tracing::warn!("Summary failed: {e}"),
                        }
                    });
                }
            }
        }
    }
}

async fn generate_summary(
    chunks: &[String],
    prior: Option<&str>,
    lang: &str,
    model: &str,
) -> Result<String, String> {
    use sgr_agent::Llm;
    use sgr_agent::types::{LlmConfig, Message};

    let transcript = chunks.join("\n");
    let prior_ctx = prior
        .map(|s| format!("\nPrevious summary:\n{s}"))
        .unwrap_or_default();

    let llm = Llm::new(&LlmConfig::auto(model));
    let messages = vec![
        Message::system(format!(
            "You are a live call summarizer. Produce 3-5 bullet points capturing \
             the key meaning of the conversation so far. Write in {lang}. \
             Focus on meaning, not word-for-word transcription. Be concise."
        )),
        Message::user(format!("Transcript:\n{transcript}{prior_ctx}")),
    ];
    llm.generate(&messages)
        .await
        .map_err(|e| format!("Summary LLM error: {e}"))
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
