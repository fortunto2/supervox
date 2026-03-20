//! Core audio types — zero external dependencies.

use serde::{Deserialize, Serialize};

/// A chunk of PCM audio data.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// Raw PCM samples (mono, f32 normalized to -1.0..1.0).
    pub samples: Vec<f32>,
    /// Sample rate in Hz (e.g., 16000, 24000, 44100).
    pub sample_rate: u32,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

impl AudioChunk {
    /// Create from f32 samples.
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        let duration_ms = if sample_rate > 0 {
            (samples.len() as u64 * 1000) / sample_rate as u64
        } else {
            0
        };
        Self {
            samples,
            sample_rate,
            duration_ms,
        }
    }

    /// Create from i16 PCM samples (converts to f32).
    pub fn from_i16(samples: &[i16], sample_rate: u32) -> Self {
        let f32_samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        Self::new(f32_samples, sample_rate)
    }

    /// Convert to i16 PCM (for WAV encoding, API calls).
    pub fn to_i16(&self) -> Vec<i16> {
        self.samples
            .iter()
            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect()
    }

    /// Encode as WAV bytes (16-bit PCM mono).
    #[cfg(feature = "wav")]
    pub fn to_wav_bytes(&self) -> Result<Vec<u8>, std::io::Error> {
        use std::io::Cursor;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(std::io::Error::other)?;
        for &sample in &self.samples {
            let i16_val = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer
                .write_sample(i16_val)
                .map_err(std::io::Error::other)?;
        }
        writer.finalize().map_err(std::io::Error::other)?;
        Ok(cursor.into_inner())
    }

    /// True if chunk has no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// RMS (root mean square) energy level.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.samples.iter().map(|s| s * s).sum();
        (sum / self.samples.len() as f32).sqrt()
    }
}

/// A speaker identified in diarized transcription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Speaker {
    /// Speaker label ("speaker_0", "speaker_1", or a name).
    pub id: String,
}

/// A time-aligned text segment within a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Transcribed text for this segment.
    pub text: String,
    /// Speaker label (if diarization was used).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// A complete transcription result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    /// Full transcribed text.
    pub text: String,
    /// Time-aligned segments (if available).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// Detected language (ISO 639-1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Total audio duration in seconds.
    pub duration_secs: f64,
}

impl Transcript {
    /// Create a simple transcript (text only, no segments).
    pub fn plain(text: String, duration_secs: f64) -> Self {
        Self {
            text,
            segments: Vec::new(),
            language: None,
            duration_secs,
        }
    }

    /// True if transcript has no text.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Speakers found in segments (deduplicated).
    pub fn speakers(&self) -> Vec<String> {
        let mut speakers: Vec<String> = self
            .segments
            .iter()
            .filter_map(|s| s.speaker.clone())
            .collect();
        speakers.sort();
        speakers.dedup();
        speakers
    }
}

/// Read a WAV file into an AudioChunk.
///
/// Handles mono and stereo (averages channels). Normalizes i16/i32 to f32 -1.0..1.0.
#[cfg(feature = "wav")]
pub fn read_wav_file(path: &std::path::Path) -> Result<AudioChunk, crate::stt::SttError> {
    use crate::stt::SttError;

    let reader = hound::WavReader::open(path)
        .map_err(|e| SttError::Other(format!("Read WAV {}: {e}", path.display())))?;

    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    let samples_f32: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bit_depth = spec.bits_per_sample;
            let max_val = (1u32 << (bit_depth - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max_val)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|s| s.unwrap_or(0.0))
            .collect(),
    };

    // Convert to mono if stereo (average channels)
    let mono = if channels > 1 {
        samples_f32
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        samples_f32
    };

    if mono.is_empty() {
        return Err(SttError::Empty);
    }

    Ok(AudioChunk::new(mono, sample_rate))
}

/// Resample f32 audio from `src_rate` to `dst_rate` Hz (linear interpolation).
pub fn resample(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s0 = samples[idx.min(samples.len() - 1)];
        let s1 = samples[(idx + 1).min(samples.len() - 1)];
        output.push(s0 + (s1 - s0) * frac as f32);
    }
    output
}

/// Resample f32 audio to 24kHz i16 (OpenAI Realtime API format).
pub fn resample_to_24k_i16(samples: &[f32], src_rate: u32) -> Vec<i16> {
    let resampled = resample(samples, src_rate, 24000);
    resampled
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_chunk_new() {
        let chunk = AudioChunk::new(vec![0.0; 16000], 16000);
        assert_eq!(chunk.duration_ms, 1000);
        assert_eq!(chunk.len(), 16000);
        assert!(!chunk.is_empty());
    }

    #[test]
    fn audio_chunk_from_i16() {
        let samples: Vec<i16> = vec![0, 16384, -16384, 32767];
        let chunk = AudioChunk::from_i16(&samples, 16000);
        assert_eq!(chunk.len(), 4);
        assert!((chunk.samples[1] - 0.5).abs() < 0.01);
        assert!((chunk.samples[2] + 0.5).abs() < 0.01);
    }

    #[test]
    fn audio_chunk_to_i16_roundtrip() {
        let original = vec![0.0_f32, 0.5, -0.5, 1.0, -1.0];
        let chunk = AudioChunk::new(original.clone(), 16000);
        let i16_data = chunk.to_i16();
        let back = AudioChunk::from_i16(&i16_data, 16000);
        for (a, b) in original.iter().zip(back.samples.iter()) {
            assert!((a - b).abs() < 0.001, "{a} != {b}");
        }
    }

    #[test]
    fn audio_chunk_empty() {
        let chunk = AudioChunk::new(vec![], 16000);
        assert!(chunk.is_empty());
        assert_eq!(chunk.duration_ms, 0);
        assert_eq!(chunk.rms(), 0.0);
    }

    #[test]
    fn audio_chunk_rms() {
        // Sine-like: RMS of [1.0, -1.0] = 1.0
        let chunk = AudioChunk::new(vec![1.0, -1.0, 1.0, -1.0], 16000);
        assert!((chunk.rms() - 1.0).abs() < 0.001);

        // Silence
        let silent = AudioChunk::new(vec![0.0; 100], 16000);
        assert_eq!(silent.rms(), 0.0);
    }

    #[test]
    fn transcript_plain() {
        let t = Transcript::plain("Hello world".into(), 2.5);
        assert_eq!(t.text, "Hello world");
        assert_eq!(t.duration_secs, 2.5);
        assert!(t.segments.is_empty());
        assert!(t.speakers().is_empty());
    }

    #[test]
    fn transcript_speakers() {
        let t = Transcript {
            text: "A: hi. B: hey.".into(),
            segments: vec![
                Segment {
                    start: 0.0,
                    end: 1.0,
                    text: "hi".into(),
                    speaker: Some("A".into()),
                },
                Segment {
                    start: 1.0,
                    end: 2.0,
                    text: "hey".into(),
                    speaker: Some("B".into()),
                },
                Segment {
                    start: 2.0,
                    end: 3.0,
                    text: "ok".into(),
                    speaker: Some("A".into()),
                },
            ],
            language: Some("en".into()),
            duration_secs: 3.0,
        };
        assert_eq!(t.speakers(), vec!["A", "B"]);
    }

    #[test]
    fn transcript_empty() {
        assert!(Transcript::plain("".into(), 0.0).is_empty());
        assert!(Transcript::plain("  ".into(), 0.0).is_empty());
        assert!(!Transcript::plain("hello".into(), 1.0).is_empty());
    }

    #[test]
    fn resample_noop() {
        let samples = vec![1.0, 2.0, 3.0];
        assert_eq!(resample(&samples, 16000, 16000), samples);
    }

    #[test]
    fn resample_downsample() {
        // 4 samples at 16kHz → ~2 samples at 8kHz
        let samples = vec![0.0, 1.0, 0.0, -1.0];
        let out = resample(&samples, 16000, 8000);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn resample_upsample() {
        let samples = vec![0.0, 1.0];
        let out = resample(&samples, 8000, 16000);
        assert_eq!(out.len(), 4);
        // First sample = 0.0, interpolated samples approach 1.0
        assert!((out[0] - 0.0).abs() < 0.01);
        assert!(out[1] > 0.0 && out[1] < 1.0);
    }

    #[test]
    fn resample_empty() {
        assert!(resample(&[], 16000, 24000).is_empty());
    }

    #[cfg(feature = "wav")]
    #[test]
    fn read_wav_file_mono() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(tmp.path(), spec).unwrap();
        for i in 0..16000 {
            let sample = (i as f32 / 16000.0 * std::f32::consts::TAU).sin();
            writer.write_sample((sample * 32767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();

        let chunk = super::read_wav_file(tmp.path()).unwrap();
        assert_eq!(chunk.sample_rate, 16000);
        assert_eq!(chunk.len(), 16000);
        assert_eq!(chunk.duration_ms, 1000);
    }

    #[cfg(feature = "wav")]
    #[test]
    fn read_wav_file_stereo_to_mono() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(tmp.path(), spec).unwrap();
        // Write 1 second of stereo silence
        for _ in 0..44100 {
            writer.write_sample(0i16).unwrap(); // left
            writer.write_sample(0i16).unwrap(); // right
        }
        writer.finalize().unwrap();

        let chunk = super::read_wav_file(tmp.path()).unwrap();
        assert_eq!(chunk.sample_rate, 44100);
        assert_eq!(chunk.len(), 44100); // mono after averaging
        assert_eq!(chunk.duration_ms, 1000);
    }

    #[cfg(feature = "wav")]
    #[test]
    fn read_wav_file_not_found() {
        let result = super::read_wav_file(std::path::Path::new("/tmp/nonexistent_voxkit_test.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn transcript_serialize() {
        let t = Transcript::plain("test".into(), 1.0);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"text\":\"test\""));
        // Empty segments should be skipped
        assert!(!json.contains("segments"));
    }
}
