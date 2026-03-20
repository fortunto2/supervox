use std::io::BufWriter;
use std::path::{Path, PathBuf};
use supervox_agent::types::{Config, SttBackend};
use tokio::sync::mpsc;
use voxkit::mic::MicCapture;
use voxkit::realtime_stt::{OpenAiStreamingStt, StreamingSttConfig, resample_to_24k};
use voxkit::stt::{StreamingSttBackend, SttInput, TranscriptEvent};
use voxkit::system_audio::SystemAudioCapture;

/// Audio source identifier.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioSource {
    Mic,
    System,
}

impl AudioSource {
    pub fn label(&self) -> &str {
        match self {
            AudioSource::Mic => "You",
            AudioSource::System => "Them",
        }
    }
}

/// Events from audio capture + STT pipeline.
#[derive(Debug)]
pub enum AudioEvent {
    /// Audio level update (0.0..1.0) for VU meter, per source.
    Level { source: AudioSource, level: f32 },
    /// Transcript segment from STT (delta or final).
    Transcript {
        source: AudioSource,
        text: String,
        is_final: bool,
    },
    /// Translation of a final transcript segment.
    #[allow(dead_code)]
    Translation { source_id: String, text: String },
    /// Rolling summary update.
    #[allow(dead_code)]
    Summary(String),
    /// Ducking state changed (true = mic suppressed due to system audio).
    Ducking(bool),
    /// Error from audio pipeline.
    Error(String),
    /// Recording stopped — full transcript + duration + optional audio path.
    Stopped {
        transcript: String,
        duration_secs: f64,
        audio_path: Option<String>,
    },
}

/// Handle to a running audio capture + STT pipeline.
#[derive(Default)]
pub struct AudioPipeline {
    mic_capture: Option<MicCapture>,
    system_capture: Option<SystemAudioCapture>,
    stop_tx: Option<mpsc::Sender<()>>,
}

impl AudioPipeline {
    /// Start recording with mic + optional system audio, wired to STT backend.
    pub fn start(
        &mut self,
        event_tx: mpsc::UnboundedSender<AudioEvent>,
        config: &Config,
        calls_dir: PathBuf,
    ) -> Result<(), String> {
        if self.mic_capture.is_some() {
            return Err("Already recording".into());
        }

        let stt_backend = create_stt_backend(config)?;

        // Start mic capture (raw — no VAD, STT handles voice detection)
        let (mic_rx, mic_capture) =
            MicCapture::start_raw().map_err(|e| format!("Mic error: {e}"))?;
        self.mic_capture = Some(mic_capture);

        // Optionally start system audio capture
        let system_rx = if config.capture.includes_system() {
            match SystemAudioCapture::start_raw() {
                Ok((rx, capture)) => {
                    self.system_capture = Some(capture);
                    Some(rx)
                }
                Err(e) => {
                    tracing::warn!("System audio unavailable, mic-only: {e}");
                    let _ =
                        event_tx.send(AudioEvent::Error(format!("System audio unavailable: {e}")));
                    None
                }
            }
        } else {
            None
        };

        let (stop_tx, stop_rx) = mpsc::channel::<()>(1);
        self.stop_tx = Some(stop_tx);

        // Create system audio STT backend (same type as mic backend)
        let system_stt_backend = if system_rx.is_some() {
            Some(create_stt_backend(config)?)
        } else {
            None
        };

        let ducking_threshold = config.ducking_threshold;
        tokio::spawn(async move {
            if let Err(e) = run_pipeline(
                stt_backend,
                system_stt_backend,
                mic_rx,
                system_rx,
                stop_rx,
                event_tx,
                calls_dir,
                ducking_threshold,
            )
            .await
            {
                tracing::error!("Audio pipeline error: {e}");
            }
        });

        Ok(())
    }

    /// Stop recording.
    pub fn stop(&mut self) {
        if let Some(capture) = self.mic_capture.take() {
            capture.stop();
        }
        if let Some(capture) = self.system_capture.take() {
            capture.stop();
        }
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.try_send(());
        }
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        self.mic_capture.is_some()
    }
}

/// Effective STT backend (env override or config value).
pub fn effective_stt_backend(config: &Config) -> SttBackend {
    match std::env::var("SUPERVOX_STT_BACKEND").ok().as_deref() {
        Some("whisper") => SttBackend::Whisper,
        Some("realtime") => SttBackend::Realtime,
        Some("parakeet") => SttBackend::Parakeet,
        _ => config.stt_backend.clone(),
    }
}

/// Create an STT backend based on config.
pub fn create_stt_backend(config: &Config) -> Result<Box<dyn StreamingSttBackend>, String> {
    match effective_stt_backend(config) {
        SttBackend::Realtime => {
            let api_key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| "OPENAI_API_KEY not set — required for realtime STT".to_string())?;
            let stt_config = StreamingSttConfig::new(&api_key);
            Ok(Box::new(OpenAiStreamingStt::new(stt_config)))
        }
        #[cfg(feature = "whisper")]
        SttBackend::Whisper => {
            let models_dir = supervox_agent::storage::data_dir().join("models");
            let model_path = models_dir.join(format!("ggml-{}.bin", config.whisper_model));
            if !model_path.exists() {
                return Err(format!(
                    "Whisper model not found: {}. Run `supervox` once with internet to auto-download.",
                    model_path.display()
                ));
            }
            Ok(Box::new(voxkit::whisper_stt::WhisperStt::new(
                model_path,
                &config.my_language,
            )))
        }
        #[cfg(not(feature = "whisper"))]
        SttBackend::Whisper => {
            Err("Whisper support not compiled in. Build with --features whisper".into())
        }
        #[cfg(feature = "parakeet")]
        SttBackend::Parakeet => {
            let model_dir = voxkit::parakeet_stt::default_model_dir();
            if !voxkit::parakeet_stt::model_exists(&model_dir) {
                // Fallback: check life2film models dir
                let alt_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join("startups/active/life2film/video-analyzer/models/parakeet-tdt");
                if voxkit::parakeet_stt::model_exists(&alt_dir) {
                    return Ok(Box::new(voxkit::parakeet_stt::ParakeetStt::new(
                        alt_dir,
                        &config.my_language,
                    )));
                }
                return Err(format!(
                    "Parakeet model not found: {}. Download from HuggingFace: istupakov/parakeet-tdt-0.6b-v2-onnx",
                    model_dir.display()
                ));
            }
            Ok(Box::new(voxkit::parakeet_stt::ParakeetStt::new(
                model_dir,
                &config.my_language,
            )))
        }
        #[cfg(not(feature = "parakeet"))]
        SttBackend::Parakeet => {
            Err("Parakeet support not compiled in. Build with --features parakeet".into())
        }
    }
}

/// Ensure whisper model is downloaded (called at startup if whisper backend selected).
#[cfg(feature = "whisper")]
pub async fn ensure_whisper_model(config: &Config) -> Result<std::path::PathBuf, String> {
    let models_dir = supervox_agent::storage::data_dir().join("models");
    voxkit::whisper_stt::ensure_model(&config.whisper_model, &models_dir)
        .await
        .map_err(|e| format!("Model download failed: {e}"))
}

/// Receive from an optional channel, or pend forever if None.
async fn recv_opt<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Run the audio pipeline: capture → resample → STT → events, with WAV recording.
#[allow(clippy::too_many_arguments)]
async fn run_pipeline(
    stt_backend: Box<dyn StreamingSttBackend>,
    system_stt_backend: Option<Box<dyn StreamingSttBackend>>,
    mut mic_rx: mpsc::Receiver<voxkit::AudioChunk>,
    mut system_rx: Option<mpsc::Receiver<voxkit::AudioChunk>>,
    mut stop_rx: mpsc::Receiver<()>,
    event_tx: mpsc::UnboundedSender<AudioEvent>,
    calls_dir: PathBuf,
    ducking_threshold: f32,
) -> Result<(), String> {
    // Connect mic STT
    let (stt_tx, mut stt_rx) = stt_backend
        .connect()
        .await
        .map_err(|e| format!("STT connect error: {e}"))?;

    // System audio gets its own STT connection (separate speaker)
    let (system_stt_tx, mut system_stt_rx) = if let Some(backend) = system_stt_backend {
        let (tx, rx) = backend
            .connect()
            .await
            .map_err(|e| format!("System STT connect error: {e}"))?;
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let mut total_samples = 0u64;
    let mut sample_rate = 48000u32;
    let mut full_transcript = String::new();

    // WAV writer — initialized lazily on first mic chunk (to get actual sample rate)
    let mut wav_writer: Option<hound::WavWriter<BufWriter<std::fs::File>>> = None;
    let mut wav_path: Option<PathBuf> = None;

    // Ducking state — suppress mic STT when system audio is loud
    let mut is_ducked = false;

    loop {
        tokio::select! {
            // Mic audio chunks
            chunk = mic_rx.recv() => {
                match chunk {
                    Some(chunk) => {
                        sample_rate = chunk.sample_rate;
                        total_samples += chunk.len() as u64;

                        let level = chunk.rms().min(1.0);
                        let _ = event_tx.send(AudioEvent::Level { source: AudioSource::Mic, level });

                        // Initialize WAV writer on first chunk
                        if wav_writer.is_none() {
                            match create_wav_writer(&calls_dir, chunk.sample_rate) {
                                Ok((writer, path)) => {
                                    wav_path = Some(path);
                                    wav_writer = Some(writer);
                                }
                                Err(e) => {
                                    tracing::warn!("WAV recording unavailable: {e}");
                                    let _ = event_tx.send(AudioEvent::Error(
                                        format!("WAV recording failed: {e}"),
                                    ));
                                }
                            }
                        }

                        // Write raw samples to WAV
                        if let Some(ref mut writer) = wav_writer {
                            for &sample in &chunk.samples {
                                let i16_val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                                if writer.write_sample(i16_val).is_err() {
                                    tracing::warn!("WAV write error, stopping recording");
                                    wav_writer = None;
                                    break;
                                }
                            }
                        }

                        // Resample to 24kHz i16 and send to STT (skip when ducked)
                        if !is_ducked {
                            let resampled = resample_to_24k(&chunk.samples, chunk.sample_rate);
                            let _ = stt_tx.send(SttInput::Audio(resampled)).await;
                        }
                    }
                    None => break,
                }
            }

            // System audio chunks (pends forever if not enabled)
            chunk = recv_opt(&mut system_rx) => {
                if let Some(chunk) = chunk {
                    let level = chunk.rms().min(1.0);
                    let _ = event_tx.send(AudioEvent::Level { source: AudioSource::System, level });

                    // Update ducking state
                    let should_duck = level > ducking_threshold;
                    if should_duck != is_ducked {
                        is_ducked = should_duck;
                        let _ = event_tx.send(AudioEvent::Ducking(is_ducked));
                    }

                    let resampled = resample_to_24k(&chunk.samples, chunk.sample_rate);
                    if let Some(ref tx) = system_stt_tx {
                        let _ = tx.send(SttInput::Audio(resampled)).await;
                    }
                }
            }

            // Mic STT transcript events
            event = stt_rx.recv() => {
                match event {
                    Some(TranscriptEvent::Delta { text, .. }) => {
                        let _ = event_tx.send(AudioEvent::Transcript {
                            source: AudioSource::Mic,
                            text,
                            is_final: false,
                        });
                    }
                    Some(TranscriptEvent::Final { text, .. }) => {
                        if !text.is_empty() {
                            full_transcript.push_str(&format!("You: {}\n", text));
                        }
                        let _ = event_tx.send(AudioEvent::Transcript {
                            source: AudioSource::Mic,
                            text,
                            is_final: true,
                        });
                    }
                    Some(TranscriptEvent::Error(e)) => {
                        let _ = event_tx.send(AudioEvent::Error(format!("STT: {e}")));
                    }
                    None => break,
                }
            }

            // System STT transcript events (pends forever if not enabled)
            event = recv_opt(&mut system_stt_rx) => {
                match event {
                    Some(TranscriptEvent::Delta { text, .. }) => {
                        let _ = event_tx.send(AudioEvent::Transcript {
                            source: AudioSource::System,
                            text,
                            is_final: false,
                        });
                    }
                    Some(TranscriptEvent::Final { text, .. }) => {
                        if !text.is_empty() {
                            full_transcript.push_str(&format!("Them: {}\n", text));
                        }
                        let _ = event_tx.send(AudioEvent::Transcript {
                            source: AudioSource::System,
                            text,
                            is_final: true,
                        });
                    }
                    Some(TranscriptEvent::Error(e)) => {
                        let _ = event_tx.send(AudioEvent::Error(format!("System STT: {e}")));
                    }
                    None => {} // system STT disconnected, continue with mic
                }
            }

            // Stop signal
            _ = stop_rx.recv() => {
                let _ = stt_tx.send(SttInput::Close).await;
                if let Some(ref tx) = system_stt_tx {
                    let _ = tx.send(SttInput::Close).await;
                }
                break;
            }
        }
    }

    // Finalize WAV file
    let audio_path = if let Some(writer) = wav_writer {
        match writer.finalize() {
            Ok(()) => wav_path.map(|p| p.to_string_lossy().into_owned()),
            Err(e) => {
                tracing::warn!("WAV finalize error: {e}");
                None
            }
        }
    } else {
        None
    };

    let duration_secs = total_samples as f64 / sample_rate as f64;
    let _ = event_tx.send(AudioEvent::Stopped {
        transcript: full_transcript,
        duration_secs,
        audio_path,
    });

    Ok(())
}

/// Create a WAV writer for incremental recording.
/// Returns the writer and the file path. Uses a temp filename that will be
/// renamed by save_recorded_call to match the call's final name.
fn create_wav_writer(
    calls_dir: &Path,
    sample_rate: u32,
) -> Result<(hound::WavWriter<BufWriter<std::fs::File>>, PathBuf), String> {
    std::fs::create_dir_all(calls_dir).map_err(|e| format!("Create dir: {e}"))?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = calls_dir.join(format!("recording-{timestamp}.wav"));

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let file = std::fs::File::create(&path).map_err(|e| format!("Create WAV: {e}"))?;
    let writer =
        hound::WavWriter::new(BufWriter::new(file), spec).map_err(|e| format!("WAV init: {e}"))?;

    Ok((writer, path))
}

/// Save the recorded call to storage. If `temp_audio_path` is provided,
/// renames the temp WAV to match the call's canonical name.
pub fn save_recorded_call(
    transcript: &str,
    duration_secs: f64,
    calls_dir: &Path,
    temp_audio_path: Option<&str>,
    bookmarks: &[supervox_agent::types::Bookmark],
) -> Result<String, String> {
    use chrono::Utc;
    use supervox_agent::types::Call;

    if transcript.is_empty() {
        // Clean up temp WAV if no transcript
        if let Some(p) = temp_audio_path {
            let _ = std::fs::remove_file(p);
        }
        return Ok(String::new());
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now();

    // Rename temp WAV to canonical path
    let final_audio_path = if let Some(temp_path) = temp_audio_path {
        let temp = std::path::PathBuf::from(temp_path);
        if temp.exists() {
            let date = now.format("%Y%m%d");
            let canonical = calls_dir.join(format!("{date}-{id}.wav"));
            match std::fs::rename(&temp, &canonical) {
                Ok(()) => Some(canonical.to_string_lossy().into_owned()),
                Err(e) => {
                    tracing::warn!("Failed to rename WAV: {e}");
                    // Keep temp path as fallback
                    Some(temp_path.to_string())
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let call = Call {
        id: id.clone(),
        created_at: now,
        duration_secs,
        participants: vec![],
        language: None,
        transcript: transcript.to_string(),
        translation: None,
        tags: vec![],
        audio_path: final_audio_path,
        bookmarks: bookmarks.to_vec(),
    };

    supervox_agent::storage::save_call(calls_dir, &call).map_err(|e| format!("Save error: {e}"))?;
    Ok(id)
}

/// Get the path to the most recently saved call file.
pub fn last_saved_call_path(calls_dir: &Path) -> Option<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(calls_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    entries.first().map(|e| e.path())
}
