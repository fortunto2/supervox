//! Microphone capture via cpal — cross-platform audio input.
//!
//! Captures from the default input device, runs VAD processing,
//! and emits `AudioChunk` segments through an async channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::types::AudioChunk;
use crate::vad::{VadConfig, VadEvent, VadProcessor};

/// Errors from microphone capture.
#[derive(Debug, Error)]
pub enum MicError {
    /// No input device found.
    #[error("No input device available")]
    NoDevice,
    /// Failed to get device config.
    #[error("Device config: {0}")]
    DeviceConfig(String),
    /// Failed to build audio stream.
    #[error("Stream build: {0}")]
    StreamBuild(String),
    /// Failed to start audio stream.
    #[error("Stream play: {0}")]
    StreamPlay(String),
}

/// Microphone capture handle.
///
/// Captures audio from the default input device, runs VAD, and sends
/// detected speech segments as `AudioChunk`s through a channel.
///
/// Drop or call `stop()` to end capture.
pub struct MicCapture {
    stop_flag: Arc<AtomicBool>,
    // Hold the stream to keep it alive; dropped on MicCapture drop.
    _stream: cpal::Stream,
}

impl MicCapture {
    /// Start capturing from the default input device with VAD.
    ///
    /// Returns `(audio_receiver, capture_handle)`.
    /// The receiver yields `AudioChunk` segments when speech is detected and ends.
    pub fn start(vad_config: VadConfig) -> Result<(mpsc::Receiver<AudioChunk>, Self), MicError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(MicError::NoDevice)?;

        let supported_config = device
            .default_input_config()
            .map_err(|e| MicError::DeviceConfig(e.to_string()))?;

        let sample_rate = supported_config.sample_rate().0;
        let channels = supported_config.channels();
        let sample_format = supported_config.sample_format();

        let (tx, rx) = mpsc::channel::<AudioChunk>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let config: cpal::StreamConfig = supported_config.into();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let mut vad = VadProcessor::new_rms(vad_config, sample_rate);
                build_stream_f32(&device, &config, channels, stop_clone, tx, &mut vad)?
            }
            cpal::SampleFormat::I16 => {
                let mut vad = VadProcessor::new_rms(vad_config, sample_rate);
                build_stream_i16(&device, &config, channels, stop_clone, tx, &mut vad)?
            }
            _ => {
                return Err(MicError::DeviceConfig(format!(
                    "Unsupported sample format: {sample_format:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| MicError::StreamPlay(e.to_string()))?;

        Ok((
            rx,
            Self {
                stop_flag,
                _stream: stream,
            },
        ))
    }

    /// Start capturing raw audio without VAD processing.
    ///
    /// Every buffer from the audio device is sent as an `AudioChunk`.
    pub fn start_raw() -> Result<(mpsc::Receiver<AudioChunk>, Self), MicError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(MicError::NoDevice)?;

        let supported_config = device
            .default_input_config()
            .map_err(|e| MicError::DeviceConfig(e.to_string()))?;

        let sample_rate = supported_config.sample_rate().0;
        let channels = supported_config.channels();

        let (tx, rx) = mpsc::channel::<AudioChunk>(32);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let config: cpal::StreamConfig = supported_config.into();

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if stop_clone.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono = mix_to_mono_f32(data, channels);
                    let chunk = AudioChunk::new(mono, sample_rate);
                    let _ = tx.try_send(chunk);
                },
                |err| {
                    tracing::error!("Mic stream error: {err}");
                },
                None,
            )
            .map_err(|e| MicError::StreamBuild(e.to_string()))?;

        stream
            .play()
            .map_err(|e| MicError::StreamPlay(e.to_string()))?;

        Ok((
            rx,
            Self {
                stop_flag,
                _stream: stream,
            },
        ))
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

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Build a cpal input stream for f32 sample format with VAD.
fn build_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    stop_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<AudioChunk>,
    vad: &mut VadProcessor,
) -> Result<cpal::Stream, MicError> {
    let sample_rate = config.sample_rate.0;

    // VAD processor must be in the callback closure, so we use a raw pointer
    // trick: wrap in Arc<Mutex> for thread safety.
    let vad_mutex = Arc::new(std::sync::Mutex::new(std::mem::replace(
        vad,
        VadProcessor::new_rms(VadConfig::default(), sample_rate),
    )));

    device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                let mono = mix_to_mono_f32(data, channels);
                process_vad(&vad_mutex, &mono, sample_rate, &tx);
            },
            |err| {
                tracing::error!("Mic stream error: {err}");
            },
            None,
        )
        .map_err(|e| MicError::StreamBuild(e.to_string()))
}

/// Build a cpal input stream for i16 sample format with VAD.
fn build_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    stop_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<AudioChunk>,
    vad: &mut VadProcessor,
) -> Result<cpal::Stream, MicError> {
    let sample_rate = config.sample_rate.0;

    let vad_mutex = Arc::new(std::sync::Mutex::new(std::mem::replace(
        vad,
        VadProcessor::new_rms(VadConfig::default(), sample_rate),
    )));

    device
        .build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                let mono = mix_to_mono_i16(data, channels);
                process_vad(&vad_mutex, &mono, sample_rate, &tx);
            },
            |err| {
                tracing::error!("Mic stream error: {err}");
            },
            None,
        )
        .map_err(|e| MicError::StreamBuild(e.to_string()))
}

/// Process audio through VAD and send detected speech segments.
fn process_vad(
    vad: &Arc<std::sync::Mutex<VadProcessor>>,
    mono_samples: &[f32],
    _sample_rate: u32,
    tx: &mpsc::Sender<AudioChunk>,
) {
    let mut vad = match vad.lock() {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("VAD mutex poisoned: {e}");
            return;
        }
    };

    let events = vad.feed(mono_samples);
    for event in events {
        if let VadEvent::SpeechEnd(chunk) = event {
            let _ = tx.try_send(chunk);
        }
    }
}

/// Mix multi-channel f32 audio to mono by averaging channels.
pub fn mix_to_mono_f32(data: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return data.to_vec();
    }
    let ch = channels as usize;
    data.chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Mix multi-channel i16 audio to mono f32 by averaging channels.
pub fn mix_to_mono_i16(data: &[i16], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return data.iter().map(|&s| s as f32 / 32768.0).collect();
    }
    let ch = channels as usize;
    data.chunks_exact(ch)
        .map(|frame| {
            let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
            sum / ch as f32
        })
        .collect()
}

/// List available input devices by name.
pub fn list_input_devices() -> Result<Vec<String>, MicError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| MicError::DeviceConfig(e.to_string()))?;

    let names: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_mono_f32_passthrough() {
        let data = vec![0.5, -0.3, 0.1];
        let mono = mix_to_mono_f32(&data, 1);
        assert_eq!(mono, data);
    }

    #[test]
    fn mix_stereo_f32() {
        // Stereo: [L, R, L, R, ...]
        let data = vec![0.4, 0.6, -0.2, 0.2];
        let mono = mix_to_mono_f32(&data, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.5).abs() < 0.001);
        assert!((mono[1] - 0.0).abs() < 0.001);
    }

    #[test]
    fn mix_mono_i16() {
        let data: Vec<i16> = vec![16384, -16384]; // ~0.5, ~-0.5
        let mono = mix_to_mono_i16(&data, 1);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.5).abs() < 0.01);
        assert!((mono[1] + 0.5).abs() < 0.01);
    }

    #[test]
    fn mix_stereo_i16() {
        let data: Vec<i16> = vec![16384, -16384, 0, 0];
        let mono = mix_to_mono_i16(&data, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 0.01); // (0.5 + -0.5) / 2
        assert!((mono[1] - 0.0).abs() < 0.01);
    }

    #[test]
    fn list_devices_runs() {
        // Just verify it doesn't panic — may return empty on CI
        let result = list_input_devices();
        assert!(result.is_ok());
    }

    #[test]
    fn mic_error_display() {
        let e = MicError::NoDevice;
        assert_eq!(format!("{e}"), "No input device available");
    }
}
