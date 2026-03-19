use std::path::Path;
use tokio::sync::mpsc;
use voxkit::mic::MicCapture;
use voxkit::vad::VadConfig;

/// Events from audio capture + STT pipeline.
#[derive(Debug)]
#[allow(dead_code)] // Transcript variant used when STT is wired
pub enum AudioEvent {
    /// Audio level update (0.0..1.0) for status bar.
    Level(f32),
    /// New transcript segment from STT.
    Transcript(String),
    /// Error from audio pipeline.
    #[allow(dead_code)]
    Error(String),
    /// Recording stopped.
    Stopped {
        /// Full transcript text.
        transcript: String,
        /// Duration in seconds.
        duration_secs: f64,
    },
}

/// Handle to a running audio capture + STT pipeline.
#[derive(Default)]
pub struct AudioPipeline {
    capture: Option<MicCapture>,
    stop_tx: Option<mpsc::Sender<()>>,
}

impl AudioPipeline {
    /// Start recording with mic capture. Sends AudioEvents to the provided sender.
    pub fn start(&mut self, event_tx: mpsc::UnboundedSender<AudioEvent>) -> Result<(), String> {
        if self.capture.is_some() {
            return Err("Already recording".into());
        }

        let (mut audio_rx, capture) =
            MicCapture::start(VadConfig::default()).map_err(|e| format!("Mic error: {e}"))?;

        self.capture = Some(capture);

        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        self.stop_tx = Some(stop_tx);

        tokio::spawn(async move {
            let mut total_samples = 0u64;
            let mut sample_rate = 16000u32;

            loop {
                tokio::select! {
                    chunk = audio_rx.recv() => {
                        match chunk {
                            Some(chunk) => {
                                sample_rate = chunk.sample_rate;
                                total_samples += chunk.len() as u64;

                                let level = chunk.rms().min(1.0);
                                let _ = event_tx.send(AudioEvent::Level(level));
                            }
                            None => break,
                        }
                    }
                    _ = stop_rx.recv() => {
                        break;
                    }
                }
            }

            let duration_secs = total_samples as f64 / sample_rate as f64;
            let _ = event_tx.send(AudioEvent::Stopped {
                transcript: String::new(),
                duration_secs,
            });
        });

        Ok(())
    }

    /// Stop recording.
    pub fn stop(&mut self) {
        if let Some(capture) = self.capture.take() {
            capture.stop();
        }
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.try_send(());
        }
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        self.capture.is_some()
    }
}

/// Save the recorded call to storage.
pub fn save_recorded_call(
    transcript: &str,
    duration_secs: f64,
    calls_dir: &Path,
) -> Result<(), String> {
    use chrono::Utc;
    use supervox_agent::types::Call;

    if transcript.is_empty() {
        return Ok(());
    }

    let call = Call {
        id: uuid::Uuid::now_v7().to_string(),
        created_at: Utc::now(),
        duration_secs,
        participants: vec![],
        language: None,
        transcript: transcript.to_string(),
        translation: None,
        tags: vec![],
    };

    supervox_agent::storage::save_call(calls_dir, &call).map_err(|e| format!("Save error: {e}"))
}
