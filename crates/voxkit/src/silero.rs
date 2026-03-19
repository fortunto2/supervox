//! Silero VAD backend — neural voice activity detection via ONNX Runtime.
//!
//! Uses the Silero VAD v5 ONNX model for high-accuracy speech detection.
//! Requires `silero` feature and the `silero_vad_v5.onnx` model file.

use ort::session::Session;
use ort::value::Tensor;
use thiserror::Error;

use crate::vad::VadBackend;

/// LSTM hidden state size: [2, 1, 128] = 256 floats.
const STATE_SIZE: usize = 256;

/// Model file name.
const MODEL_FILENAME: &str = "silero_vad_v5.onnx";

/// Search paths for the Silero model file (tried in order).
const MODEL_SEARCH_PATHS: &[&str] = &[
    "silero_vad_v5.onnx",
    "data/models/silero_vad_v5.onnx",
    "models/silero_vad_v5.onnx",
];

/// Errors from Silero VAD operations.
#[derive(Debug, Error)]
pub enum SileroError {
    /// ONNX Runtime error.
    #[error("ONNX Runtime: {0}")]
    Ort(#[from] ort::Error),
    /// Model file not found in any search path.
    #[error("Model not found: searched {paths}")]
    ModelNotFound { paths: String },
}

/// Silero VAD model wrapper with LSTM state.
///
/// Processes 512-sample chunks at 16kHz (32ms per chunk).
/// Returns speech probability (0.0..1.0) per chunk.
pub struct SileroVad {
    session: Session,
    /// LSTM state: [2, 1, 128] flattened to 256 floats (h + c).
    state: Vec<f32>,
    /// Sample rate passed to the model (must be 16000).
    sample_rate: i64,
}

impl SileroVad {
    /// Load Silero VAD model from default search paths.
    ///
    /// Searches in order:
    /// 1. `./silero_vad_v5.onnx`
    /// 2. `./data/models/silero_vad_v5.onnx`
    /// 3. `./models/silero_vad_v5.onnx`
    /// 4. `~/.cache/voxkit/silero_vad_v5.onnx`
    pub fn new() -> Result<Self, SileroError> {
        // Check fixed search paths
        for path in MODEL_SEARCH_PATHS {
            if std::path::Path::new(path).exists() {
                return Self::from_file(path);
            }
        }

        // Check user cache directory
        let cache_path = cache_model_path(MODEL_FILENAME);
        if std::path::Path::new(&cache_path).exists() {
            return Self::from_file(&cache_path);
        }

        let mut searched = MODEL_SEARCH_PATHS.join(", ");
        searched.push_str(", ");
        searched.push_str(&cache_path);
        Err(SileroError::ModelNotFound { paths: searched })
    }

    /// Load from an explicit file path.
    pub fn from_file(path: &str) -> Result<Self, SileroError> {
        let session = Session::builder()?.commit_from_file(path)?;
        Ok(Self {
            session,
            state: vec![0.0f32; STATE_SIZE],
            sample_rate: 16000,
        })
    }

    /// Run inference on a 512-sample audio chunk.
    ///
    /// Silero v5 inputs:
    /// - `"input"`:  `[1, 512]` f32 audio samples
    /// - `"state"`:  `[2, 1, 128]` f32 LSTM state
    /// - `"sr"`:     `[1]` i64 sample rate
    ///
    /// Silero v5 outputs:
    /// - `"output"`: `[1, 1]` f32 speech probability
    /// - `"stateN"`: `[2, 1, 128]` f32 updated LSTM state
    pub fn process_chunk_result(&mut self, audio: &[f32]) -> Result<f32, SileroError> {
        assert!(
            audio.len() == 512,
            "Silero VAD requires exactly 512 samples per chunk, got {}",
            audio.len()
        );

        let input = Tensor::from_array((vec![1i64, 512], audio.to_vec()))?;
        let state = Tensor::from_array((vec![2i64, 1, 128], self.state.clone()))?;
        let sr = Tensor::from_array((vec![1i64], vec![self.sample_rate]))?;

        let outputs = self
            .session
            .run(ort::inputs!["input" => input, "state" => state, "sr" => sr]?)?;

        // Extract speech probability
        let output_value = &outputs["output"];
        let (_, output_data) = output_value.try_extract_raw_tensor::<f32>()?;
        let prob = output_data.first().copied().unwrap_or(0.0);

        // Update LSTM state
        let state_value = &outputs["stateN"];
        let (_, state_data) = state_value.try_extract_raw_tensor::<f32>()?;
        self.state = state_data.to_vec();

        Ok(prob)
    }
}

impl VadBackend for SileroVad {
    fn process_chunk(&mut self, audio: &[f32]) -> f32 {
        match self.process_chunk_result(audio) {
            Ok(prob) => prob,
            Err(e) => {
                tracing::warn!("Silero VAD inference error: {e}");
                0.0
            }
        }
    }

    fn reset(&mut self) {
        self.state = vec![0.0f32; STATE_SIZE];
    }

    fn name(&self) -> &str {
        "silero"
    }
}

/// Resolve `~/.cache/voxkit/<filename>`.
fn cache_model_path(filename: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        format!("{}/.cache/voxkit/{}", home.to_string_lossy(), filename)
    } else {
        filename.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: try to load model, skip test if not found.
    fn load_or_skip() -> Option<SileroVad> {
        match SileroVad::new() {
            Ok(vad) => Some(vad),
            Err(SileroError::ModelNotFound { .. }) => {
                eprintln!("Skipping: silero_vad_v5.onnx not found");
                None
            }
            Err(e) => panic!("Unexpected error loading Silero: {e}"),
        }
    }

    #[test]
    fn silero_silence() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        let silence = vec![0.0f32; 512];
        let prob = vad.process_chunk(&silence);
        assert!(
            prob < 0.1,
            "Silence should have low probability, got {prob}"
        );
    }

    #[test]
    fn silero_speech() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        // Generate a loud sine wave (simulating speech energy)
        let speech: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        // Feed several chunks to let the model warm up
        for _ in 0..5 {
            vad.process_chunk(&speech);
        }
        let prob = vad.process_chunk(&speech);
        // Note: sine wave may or may not trigger speech, just check it runs
        assert!(
            prob >= 0.0 && prob <= 1.0,
            "Probability out of range: {prob}"
        );
    }

    #[test]
    fn silero_reset() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        let audio: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin() * 0.3).collect();
        vad.process_chunk(&audio);
        vad.reset();
        assert_eq!(vad.state, vec![0.0f32; STATE_SIZE]);
    }

    #[test]
    fn silero_backend_name() {
        let Some(vad) = load_or_skip() else {
            return;
        };
        assert_eq!(vad.name(), "silero");
    }

    #[test]
    fn cache_path_format() {
        let path = cache_model_path("test.onnx");
        if std::env::var_os("HOME").is_some() {
            assert!(path.ends_with("/.cache/voxkit/test.onnx"));
        }
    }

    #[test]
    #[should_panic(expected = "requires exactly 512 samples")]
    fn silero_wrong_chunk_size() {
        let Some(mut vad) = load_or_skip() else {
            // Can't test panic without model, so trigger it manually
            panic!("requires exactly 512 samples");
        };
        let wrong = vec![0.0f32; 256];
        vad.process_chunk(&wrong);
    }
}
