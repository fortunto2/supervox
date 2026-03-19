//! macOS system audio capture via ScreenCaptureKit helper binary.
//!
//! Captures system audio output using a companion Swift binary (`system-audio-tap`)
//! that uses ScreenCaptureKit. The binary streams raw PCM data to stdout.
//!
//! Only available on macOS (`#[cfg(target_os = "macos")]`).

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;
use tokio::sync::mpsc;

use crate::types::AudioChunk;
use crate::vad::{VadConfig, VadEvent, VadProcessor};

/// Search paths for the system-audio-tap binary (tried in order).
const BINARY_CANDIDATES: &[&str] = &["system-audio-tap"];

/// System audio sample rate (ScreenCaptureKit default).
const SYSTEM_AUDIO_SAMPLE_RATE: u32 = 48000;

/// Errors from system audio capture.
#[derive(Debug, Error)]
pub enum SystemAudioError {
    /// Helper binary not found.
    #[error("system-audio-tap binary not found (searched: {paths})")]
    BinaryNotFound { paths: String },
    /// Failed to spawn the helper process.
    #[error("Spawn: {0}")]
    Spawn(#[from] std::io::Error),
    /// Process exited with error.
    #[error("Process exited: {0}")]
    ProcessError(String),
}

/// Guard that kills the child process on drop.
struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// System audio capture handle.
///
/// Spawns a `system-audio-tap` helper binary that captures macOS system audio
/// via ScreenCaptureKit and streams raw PCM data to stdout.
pub struct SystemAudioCapture {
    stop_flag: Arc<AtomicBool>,
}

impl SystemAudioCapture {
    /// Start capturing system audio with VAD processing.
    ///
    /// Returns `(audio_receiver, capture_handle)`.
    /// Speech segments are detected by VAD and sent as `AudioChunk`.
    pub fn start(
        vad_config: VadConfig,
    ) -> Result<(mpsc::Receiver<AudioChunk>, Self), SystemAudioError> {
        let binary = find_binary()?;
        let mut child = Command::new(&binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("No stdout"))?;

        let stderr = child.stderr.take();

        // Child is now owned by guard — will be killed on drop
        let guard = ChildGuard::new(child);

        let (tx, rx) = mpsc::channel::<AudioChunk>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        // Spawn stderr reader for logging
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => tracing::debug!("system-audio-tap: {line}"),
                        Err(_) => break,
                    }
                }
            });
        }

        // Spawn stdout reader with VAD processing (guard moves here)
        std::thread::spawn(move || {
            let _guard = guard;
            read_loop_with_vad(stdout, vad_config, stop_clone, tx);
        });

        Ok((rx, Self { stop_flag }))
    }

    /// Start capturing raw system audio without VAD.
    ///
    /// Every audio buffer is sent as an `AudioChunk` without speech detection.
    pub fn start_raw() -> Result<(mpsc::Receiver<AudioChunk>, Self), SystemAudioError> {
        let binary = find_binary()?;
        let mut child = Command::new(&binary)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("No stdout"))?;

        let stderr = child.stderr.take();

        let guard = ChildGuard::new(child);

        let (tx, rx) = mpsc::channel::<AudioChunk>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        // Spawn stderr reader
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => tracing::debug!("system-audio-tap: {line}"),
                        Err(_) => break,
                    }
                }
            });
        }

        // Spawn raw read loop (guard moves here)
        std::thread::spawn(move || {
            let _guard = guard;
            read_loop_raw(stdout, stop_clone, tx);
        });

        Ok((rx, Self { stop_flag }))
    }

    /// Stop capturing audio.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Whether capture has been stopped.
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Read PCM data from stdout, run VAD, emit speech segments.
fn read_loop_with_vad(
    stdout: impl Read,
    vad_config: VadConfig,
    stop_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<AudioChunk>,
) {
    let mut reader = BufReader::new(stdout);
    let mut vad = VadProcessor::new_rms(vad_config, SYSTEM_AUDIO_SAMPLE_RATE);

    // Read in chunks of 2048 f32 samples (4 bytes each)
    let chunk_samples = 2048;
    let mut buf = vec![0u8; chunk_samples * 4];

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(_) => break,
        }

        // Convert raw bytes to f32 samples (little-endian)
        let samples: Vec<f32> = buf
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let events = vad.feed(&samples);
        for event in events {
            if let VadEvent::SpeechEnd(chunk) = event
                && tx.blocking_send(chunk).is_err()
            {
                return; // Receiver dropped
            }
        }
    }

    // Flush remaining speech
    if let Some(chunk) = vad.flush() {
        let _ = tx.blocking_send(chunk);
    }
}

/// Read PCM data from stdout, emit raw audio chunks.
fn read_loop_raw(stdout: impl Read, stop_flag: Arc<AtomicBool>, tx: mpsc::Sender<AudioChunk>) {
    let mut reader = BufReader::new(stdout);
    let chunk_samples = 2048;
    let mut buf = vec![0u8; chunk_samples * 4];

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(_) => break,
        }

        let samples: Vec<f32> = buf
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let chunk = AudioChunk::new(samples, SYSTEM_AUDIO_SAMPLE_RATE);
        if tx.blocking_send(chunk).is_err() {
            return;
        }
    }
}

/// Find the system-audio-tap binary.
fn find_binary() -> Result<String, SystemAudioError> {
    // Check fixed candidates
    for candidate in BINARY_CANDIDATES {
        if std::path::Path::new(candidate).exists() {
            return Ok(candidate.to_string());
        }

        // Check in PATH
        if let Ok(output) = Command::new("which").arg(candidate).output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    // Check next to current executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for candidate in BINARY_CANDIDATES {
            let path = dir.join(candidate);
            if path.exists() {
                return Ok(path.to_string_lossy().to_string());
            }
        }
    }

    let paths = BINARY_CANDIDATES.join(", ");
    Err(SystemAudioError::BinaryNotFound { paths })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_binary_graceful_fail() {
        // Should fail gracefully when binary not found
        let result = find_binary();
        // May succeed if binary is installed, may fail if not
        if let Err(e) = &result {
            assert!(format!("{e}").contains("not found"));
        }
    }

    #[test]
    fn child_guard_drop() {
        // Just test construction -- can't easily test kill without a real process
        let child = Command::new("echo")
            .arg("test")
            .stdout(Stdio::null())
            .spawn();
        if let Ok(child) = child {
            let _guard = ChildGuard::new(child);
            // Guard dropped here -- should not panic
        }
    }

    #[test]
    fn system_audio_error_display() {
        let e = SystemAudioError::BinaryNotFound {
            paths: "system-audio-tap".to_string(),
        };
        assert!(format!("{e}").contains("not found"));
    }
}
