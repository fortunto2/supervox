use std::io::BufWriter;
use std::path::{Path, PathBuf};
use supervox_agent::types::Config;
use tokio::sync::mpsc;
use voxkit::mic::MicCapture;
use voxkit::realtime_stt::{OpenAiStreamingStt, StreamingSttConfig, SttInput, resample_to_24k};
use voxkit::stt::TranscriptEvent;
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
    /// Audio level update (0.0..1.0) for VU meter.
    Level(f32),
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
    /// Start recording with mic + optional system audio, wired to realtime STT.
    pub fn start(
        &mut self,
        event_tx: mpsc::UnboundedSender<AudioEvent>,
        config: &Config,
        calls_dir: PathBuf,
    ) -> Result<(), String> {
        if self.mic_capture.is_some() {
            return Err("Already recording".into());
        }

        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY not set — required for realtime STT".to_string())?;

        // Start mic capture (raw — no VAD, STT handles voice detection)
        let (mic_rx, mic_capture) =
            MicCapture::start_raw().map_err(|e| format!("Mic error: {e}"))?;
        self.mic_capture = Some(mic_capture);

        // Optionally start system audio capture
        let system_rx = if config.capture.contains("system") {
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

        let stt_config = StreamingSttConfig::new(&api_key);

        tokio::spawn(async move {
            if let Err(e) =
                run_pipeline(stt_config, mic_rx, system_rx, stop_rx, event_tx, calls_dir).await
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

/// Receive from an optional channel, or pend forever if None.
async fn recv_opt<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Run the audio pipeline: capture → resample → STT → events, with WAV recording.
async fn run_pipeline(
    stt_config: StreamingSttConfig,
    mut mic_rx: mpsc::Receiver<voxkit::AudioChunk>,
    mut system_rx: Option<mpsc::Receiver<voxkit::AudioChunk>>,
    mut stop_rx: mpsc::Receiver<()>,
    event_tx: mpsc::UnboundedSender<AudioEvent>,
    calls_dir: PathBuf,
) -> Result<(), String> {
    // Connect to OpenAI realtime STT for mic
    let (stt_tx, mut stt_rx) = OpenAiStreamingStt::connect(stt_config)
        .await
        .map_err(|e| format!("STT connect error: {e}"))?;

    // System audio gets its own STT connection (separate speaker)
    let (system_stt_tx, mut system_stt_rx) = if system_rx.is_some() {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let cfg = StreamingSttConfig::new(&api_key);
        let (tx, rx) = OpenAiStreamingStt::connect(cfg)
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

    loop {
        tokio::select! {
            // Mic audio chunks
            chunk = mic_rx.recv() => {
                match chunk {
                    Some(chunk) => {
                        sample_rate = chunk.sample_rate;
                        total_samples += chunk.len() as u64;

                        let level = chunk.rms().min(1.0);
                        let _ = event_tx.send(AudioEvent::Level(level));

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

                        // Resample to 24kHz i16 and send to STT
                        let resampled = resample_to_24k(&chunk.samples, chunk.sample_rate);
                        let _ = stt_tx.send(SttInput::Audio(resampled)).await;
                    }
                    None => break,
                }
            }

            // System audio chunks (pends forever if not enabled)
            chunk = recv_opt(&mut system_rx) => {
                if let Some(chunk) = chunk {
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
        bookmarks: vec![],
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
