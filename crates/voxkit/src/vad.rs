//! Voice Activity Detection — trait + configuration.

use crate::types::AudioChunk;

/// VAD events emitted during audio processing.
#[derive(Debug, Clone)]
pub enum VadEvent {
    /// Speech started (onset detected).
    SpeechStart,
    /// A chunk of speech audio (accumulating).
    SpeechChunk(AudioChunk),
    /// Speech ended — contains the complete speech segment.
    SpeechEnd(AudioChunk),
}

/// VAD configuration parameters.
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Speech probability threshold (0.0..1.0). Above = speech.
    pub speech_threshold: f32,
    /// Number of silence chunks before declaring end-of-speech.
    /// At 16kHz with 512-sample chunks: each chunk ≈ 32ms.
    pub silence_chunks: usize,
    /// Minimum speech chunks to emit a segment (filters out clicks/noise).
    pub min_speech_chunks: usize,
    /// Pre-speech buffer chunks (captures speech onset).
    pub pre_speech_chunks: usize,
    /// Max recording seconds (safety cap).
    pub max_recording_secs: u64,
    /// RMS fallback threshold (used when neural VAD unavailable).
    pub fallback_rms: f32,
    /// Peak fallback threshold.
    pub fallback_peak: f32,
    /// Noise gate (below this RMS = silence, no VAD needed).
    pub fallback_noise_gate: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            speech_threshold: 0.5,
            silence_chunks: 24,   // ~768ms at 16kHz/512
            min_speech_chunks: 6, // ~192ms minimum utterance
            pre_speech_chunks: 3, // ~96ms onset capture
            max_recording_secs: 300,
            fallback_rms: 0.003,
            fallback_peak: 0.01,
            fallback_noise_gate: 0.001,
        }
    }
}

impl VadConfig {
    /// Preset for clean/isolated voice (lower thresholds).
    pub fn voice_isolated() -> Self {
        Self {
            speech_threshold: 0.35,
            silence_chunks: 20,
            min_speech_chunks: 4,
            pre_speech_chunks: 3,
            max_recording_secs: 300,
            fallback_rms: 0.002,
            fallback_peak: 0.008,
            fallback_noise_gate: 0.0,
        }
    }

    /// Preset for noisy environments (higher thresholds).
    pub fn noisy() -> Self {
        Self {
            speech_threshold: 0.65,
            silence_chunks: 30,
            min_speech_chunks: 8,
            pre_speech_chunks: 5,
            max_recording_secs: 300,
            fallback_rms: 0.005,
            fallback_peak: 0.015,
            fallback_noise_gate: 0.002,
        }
    }
}

/// Voice Activity Detection backend.
///
/// Implementations: `SileroVad` (feature `silero`), `RmsVad` (built-in).
pub trait VadBackend: Send {
    /// Process a chunk of audio, return speech probability (0.0..1.0).
    ///
    /// Chunk should be 512 samples at 16kHz (32ms) for Silero.
    /// Other backends may accept different chunk sizes.
    fn process_chunk(&mut self, audio: &[f32]) -> f32;

    /// Reset internal state (call between separate audio streams).
    fn reset(&mut self);

    /// Backend name.
    fn name(&self) -> &str;
}

/// Simple RMS-based VAD — no model needed, works everywhere.
///
/// Good enough for clean audio (e.g., close microphone).
/// Use Silero for noisy environments.
pub struct RmsVad {
    /// RMS threshold for speech detection.
    pub threshold: f32,
}

impl RmsVad {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl Default for RmsVad {
    fn default() -> Self {
        Self::new(0.01) // Very sensitive default
    }
}

impl VadBackend for RmsVad {
    fn process_chunk(&mut self, audio: &[f32]) -> f32 {
        if audio.is_empty() {
            return 0.0;
        }
        let rms: f32 = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
        // Map RMS to 0.0..1.0 probability
        (rms / self.threshold).min(1.0)
    }

    fn reset(&mut self) {}

    fn name(&self) -> &str {
        "rms"
    }
}

/// Stateful VAD processor — feeds audio samples, emits speech segments.
///
/// Works with any `VadBackend`. Handles buffering, onset detection,
/// silence timeout, and minimum duration filtering.
pub struct VadProcessor {
    config: VadConfig,
    backend: Box<dyn VadBackend>,
    sample_rate: u32,
    /// Circular pre-speech buffer (captures onset).
    pre_buffer: Vec<Vec<f32>>,
    /// Accumulated speech samples.
    speech_buffer: Vec<f32>,
    /// Current state.
    is_speaking: bool,
    /// Consecutive silence chunks count.
    silence_count: usize,
    /// Total speech chunks in current segment.
    speech_chunks: usize,
}

impl VadProcessor {
    /// Create with given backend and config.
    pub fn new(backend: Box<dyn VadBackend>, config: VadConfig, sample_rate: u32) -> Self {
        Self {
            pre_buffer: Vec::with_capacity(config.pre_speech_chunks),
            config,
            backend,
            sample_rate,
            speech_buffer: Vec::new(),
            is_speaking: false,
            silence_count: 0,
            speech_chunks: 0,
        }
    }

    /// Create with RMS backend (no model needed).
    pub fn new_rms(config: VadConfig, sample_rate: u32) -> Self {
        Self::new(Box::new(RmsVad::default()), config, sample_rate)
    }

    /// Feed raw audio samples. Returns completed speech segments as `VadEvent`s.
    ///
    /// Internally splits into 512-sample chunks for the backend.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        let chunk_size = 512;
        let mut events = Vec::new();

        for chunk in samples.chunks(chunk_size) {
            let prob = self.backend.process_chunk(chunk);
            let is_speech = prob >= self.config.speech_threshold;

            if is_speech {
                if !self.is_speaking {
                    // Speech onset
                    self.is_speaking = true;
                    self.silence_count = 0;
                    self.speech_chunks = 0;
                    self.speech_buffer.clear();

                    // Prepend pre-speech buffer
                    for pre in &self.pre_buffer {
                        self.speech_buffer.extend_from_slice(pre);
                    }
                    self.pre_buffer.clear();

                    events.push(VadEvent::SpeechStart);
                }
                self.speech_buffer.extend_from_slice(chunk);
                self.speech_chunks += 1;
                self.silence_count = 0;
            } else if self.is_speaking {
                // In speech but got silence chunk
                self.speech_buffer.extend_from_slice(chunk);
                self.silence_count += 1;

                if self.silence_count >= self.config.silence_chunks {
                    // End of speech
                    self.is_speaking = false;
                    if self.speech_chunks >= self.config.min_speech_chunks {
                        let segment = AudioChunk::new(
                            std::mem::take(&mut self.speech_buffer),
                            self.sample_rate,
                        );
                        events.push(VadEvent::SpeechEnd(segment));
                    }
                    self.speech_buffer.clear();
                    self.speech_chunks = 0;
                    self.silence_count = 0;
                }
            } else {
                // Silence — maintain pre-speech buffer
                if self.pre_buffer.len() >= self.config.pre_speech_chunks {
                    self.pre_buffer.remove(0);
                }
                self.pre_buffer.push(chunk.to_vec());
            }
        }

        events
    }

    /// Flush any remaining speech (call when stopping capture).
    pub fn flush(&mut self) -> Option<AudioChunk> {
        if self.is_speaking && self.speech_chunks >= self.config.min_speech_chunks {
            self.is_speaking = false;
            let segment =
                AudioChunk::new(std::mem::take(&mut self.speech_buffer), self.sample_rate);
            self.speech_chunks = 0;
            Some(segment)
        } else {
            self.is_speaking = false;
            self.speech_buffer.clear();
            self.speech_chunks = 0;
            None
        }
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        self.backend.reset();
        self.pre_buffer.clear();
        self.speech_buffer.clear();
        self.is_speaking = false;
        self.silence_count = 0;
        self.speech_chunks = 0;
    }

    /// Whether currently in speech.
    pub fn is_speaking(&self) -> bool {
        self.is_speaking
    }

    /// Backend name.
    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vad_config_defaults() {
        let c = VadConfig::default();
        assert_eq!(c.speech_threshold, 0.5);
        assert_eq!(c.silence_chunks, 24);
    }

    #[test]
    fn rms_vad_silence() {
        let mut vad = RmsVad::default();
        let silence = vec![0.0_f32; 512];
        assert_eq!(vad.process_chunk(&silence), 0.0);
    }

    #[test]
    fn rms_vad_speech() {
        let mut vad = RmsVad::new(0.01);
        // Loud signal
        let loud: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let prob = vad.process_chunk(&loud);
        assert!(prob > 0.5, "Expected speech, got {prob}");
    }

    #[test]
    fn rms_vad_empty() {
        let mut vad = RmsVad::default();
        assert_eq!(vad.process_chunk(&[]), 0.0);
    }

    #[test]
    fn processor_silence_no_events() {
        let config = VadConfig::default();
        let mut proc = VadProcessor::new_rms(config, 16000);
        let silence = vec![0.0_f32; 16000]; // 1 second of silence
        let events = proc.feed(&silence);
        assert!(events.is_empty());
        assert!(!proc.is_speaking());
    }

    #[test]
    fn processor_speech_detected() {
        let config = VadConfig {
            speech_threshold: 0.3,
            silence_chunks: 3,
            min_speech_chunks: 2,
            pre_speech_chunks: 1,
            max_recording_secs: 10,
            ..Default::default()
        };
        let mut proc = VadProcessor::new_rms(config, 16000);

        // Generate loud audio (speech)
        let speech: Vec<f32> = (0..512 * 10)
            .map(|i| (i as f32 * 0.1).sin() * 0.5)
            .collect();
        let events = proc.feed(&speech);
        assert!(
            events.iter().any(|e| matches!(e, VadEvent::SpeechStart)),
            "Expected SpeechStart"
        );
        assert!(proc.is_speaking());

        // Follow with silence to end speech
        let silence = vec![0.0_f32; 512 * 5];
        let events2 = proc.feed(&silence);
        assert!(
            events2.iter().any(|e| matches!(e, VadEvent::SpeechEnd(_))),
            "Expected SpeechEnd"
        );
        assert!(!proc.is_speaking());
    }

    #[test]
    fn processor_flush() {
        let config = VadConfig {
            speech_threshold: 0.3,
            silence_chunks: 100, // won't trigger naturally
            min_speech_chunks: 2,
            pre_speech_chunks: 1,
            max_recording_secs: 10,
            ..Default::default()
        };
        let mut proc = VadProcessor::new_rms(config, 16000);

        let speech: Vec<f32> = (0..512 * 5).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        proc.feed(&speech);
        assert!(proc.is_speaking());

        let flushed = proc.flush();
        assert!(flushed.is_some());
        assert!(!proc.is_speaking());
    }

    #[test]
    fn processor_short_noise_filtered() {
        let config = VadConfig {
            speech_threshold: 0.3,
            silence_chunks: 3,
            min_speech_chunks: 10, // require long speech
            pre_speech_chunks: 1,
            max_recording_secs: 10,
            ..Default::default()
        };
        let mut proc = VadProcessor::new_rms(config, 16000);

        // Short burst (2 chunks) — below min_speech_chunks
        let burst: Vec<f32> = (0..512 * 2).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        proc.feed(&burst);

        // Silence to end
        let silence = vec![0.0_f32; 512 * 5];
        let events = proc.feed(&silence);

        // Should NOT produce SpeechEnd (too short)
        assert!(!events.iter().any(|e| matches!(e, VadEvent::SpeechEnd(_))));
    }

    #[test]
    fn processor_reset() {
        let config = VadConfig::default();
        let mut proc = VadProcessor::new_rms(config, 16000);
        let speech: Vec<f32> = (0..512 * 10)
            .map(|i| (i as f32 * 0.1).sin() * 0.5)
            .collect();
        proc.feed(&speech);
        assert!(proc.is_speaking());

        proc.reset();
        assert!(!proc.is_speaking());
        assert_eq!(proc.backend_name(), "rms");
    }
}
