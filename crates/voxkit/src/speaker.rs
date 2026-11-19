//! Speaker embeddings + online clustering for live diarization.
//!
//! One embedding per speech segment (WeSpeaker CAM++ ONNX, 80-bin Kaldi fbank
//! via `knf-rs`), then nearest-centroid assignment with a cosine threshold and
//! a hard cap on the number of speakers. Good for "who of the 2–3 people on
//! stage is talking"; it is not a replacement for offline pyannote.

use std::path::Path;

use ndarray::Array1;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

/// Default model file name (download from the sherpa-onnx or pyannote-rs releases).
pub const DEFAULT_MODEL_FILE: &str = "wespeaker_en_voxceleb_CAM++.onnx";

/// Segments shorter than this are not embedded (too little voice to trust).
pub const MIN_EMBED_SECS: f32 = 0.8;

#[derive(Debug, thiserror::Error)]
pub enum SpeakerError {
    #[error("ONNX: {0}")]
    Ort(String),
    #[error("fbank: {0}")]
    Fbank(String),
    #[error("model output `embs` missing")]
    NoOutput,
}

/// Computes a fixed-size voice embedding for a mono 16 kHz segment.
pub struct SpeakerEmbedder {
    session: Session,
}

impl SpeakerEmbedder {
    pub fn new(model_path: &Path) -> Result<Self, SpeakerError> {
        let session = Session::builder()
            .map_err(ort_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err)?
            .with_intra_threads(1)
            .map_err(ort_err)?
            .commit_from_file(model_path)
            .map_err(ort_err)?;
        Ok(Self { session })
    }

    /// `samples` are mono f32 in -1..1 at 16 kHz.
    pub fn embed(&mut self, samples: &[f32]) -> Result<Vec<f32>, SpeakerError> {
        let feats =
            knf_rs::compute_fbank(samples).map_err(|e| SpeakerError::Fbank(e.to_string()))?;
        let (frames, bins) = (feats.shape()[0], feats.shape()[1]);
        // knf-rs and ort may disagree on the ndarray version, so cross the
        // boundary with a plain (shape, Vec) pair.
        let data: Vec<f32> = feats.iter().copied().collect();
        let input =
            Tensor::from_array((vec![1_i64, frames as i64, bins as i64], data)).map_err(ort_err)?;
        let outputs = self
            .session
            .run(ort::inputs!["feats" => input])
            .map_err(ort_err)?;
        let (_, data) = outputs
            .get("embs")
            .ok_or(SpeakerError::NoOutput)?
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        Ok(data.to_vec())
    }
}

/// Online nearest-centroid clustering with a speaker cap.
#[derive(Debug, Clone)]
pub struct SpeakerTracker {
    max_speakers: usize,
    threshold: f32,
    /// Raw (uncentered, unit-length) member means.
    centroids: Vec<Array1<f32>>,
    counts: Vec<usize>,
    last: Option<usize>,
    /// Running sum and count of every embedding seen: the session's common
    /// component (room, mic, AGC), removed before comparing in far-field mode.
    sum: Option<Array1<f32>>,
    seen: usize,
    center_after: usize,
}

/// Embeddings seen before the session mean is trusted for centering.
pub const CENTER_AFTER: usize = 6;
/// Raw cosine threshold used during that warm-up (raw far-field voices sit at 0.8-0.9).
pub const WARMUP_THRESHOLD: f32 = 0.85;

impl SpeakerTracker {
    /// `threshold` is the cosine similarity above which a segment joins an
    /// existing speaker (after centering in `far_field` mode).
    pub fn new(max_speakers: usize, threshold: f32) -> Self {
        Self {
            max_speakers: max_speakers.max(1),
            threshold,
            centroids: Vec::new(),
            counts: Vec::new(),
            last: None,
            sum: None,
            seen: 0,
            center_after: usize::MAX,
        }
    }

    /// Far-field mode: after `CENTER_AFTER` embeddings, compare voices with the
    /// session mean subtracted. Raw CAM++ vectors from one room all share a big
    /// common component (measured: distinct voices at cos 0.82-0.92, so any
    /// threshold up to 0.8 gave one speaker); centered, they spread to -0.6..0.4.
    pub fn far_field(mut self) -> Self {
        self.center_after = CENTER_AFTER;
        self
    }

    /// Speaker index (1-based) for this embedding. Updates the centroid.
    pub fn assign(&mut self, embedding: &[f32]) -> usize {
        let e = normalize(Array1::from_vec(embedding.to_vec()));
        match &mut self.sum {
            Some(s) => *s += &e,
            None => self.sum = Some(e.clone()),
        }
        self.seen += 1;
        let centered = self.seen > self.center_after;
        let (threshold, mean) = if centered {
            let m = self.sum.as_ref().map(|s| s / self.seen as f32);
            (self.threshold, m)
        } else if self.center_after == usize::MAX {
            (self.threshold, None)
        } else {
            (WARMUP_THRESHOLD, None)
        };
        let ec = match &mean {
            Some(m) => normalize(&e - m),
            None => e.clone(),
        };
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.centroids.iter().enumerate() {
            let sim = match &mean {
                Some(m) => ec.dot(&normalize(c - m)),
                None => e.dot(c),
            };
            if best.is_none_or(|(_, b)| sim > b) {
                best = Some((i, sim));
            }
        }
        let idx = match best {
            Some((i, sim)) if sim >= threshold || self.centroids.len() >= self.max_speakers => {
                let n = self.counts[i] as f32;
                self.centroids[i] = normalize((&self.centroids[i] * n + &e) / (n + 1.0));
                self.counts[i] += 1;
                i
            }
            _ => {
                self.centroids.push(e);
                self.counts.push(1);
                self.centroids.len() - 1
            }
        };
        self.last = Some(idx + 1);
        idx + 1
    }

    /// Last assigned speaker, for short segments that were not embedded.
    pub fn last(&self) -> Option<usize> {
        self.last
    }

    pub fn speaker_count(&self) -> usize {
        self.centroids.len()
    }
}

fn ort_err<R>(e: ort::Error<R>) -> SpeakerError {
    SpeakerError::Ort(e.to_string())
}

fn normalize(v: Array1<f32>) -> Array1<f32> {
    let n = v.dot(&v).sqrt();
    if n > 0.0 { v / n } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_distinct_voices_get_two_ids_and_a_repeat_matches() {
        let mut t = SpeakerTracker::new(3, 0.5);
        assert_eq!(t.assign(&[1.0, 0.0, 0.0]), 1);
        assert_eq!(t.assign(&[0.0, 1.0, 0.0]), 2);
        assert_eq!(t.assign(&[0.9, 0.1, 0.0]), 1);
        assert_eq!(t.speaker_count(), 2);
        assert_eq!(t.last(), Some(1));
    }

    #[test]
    fn speaker_cap_forces_nearest_match() {
        let mut t = SpeakerTracker::new(2, 0.9);
        t.assign(&[1.0, 0.0, 0.0]);
        t.assign(&[0.0, 1.0, 0.0]);
        // A third, unlike voice must still land on one of the two.
        let id = t.assign(&[0.0, 0.0, 1.0]);
        assert!(id == 1 || id == 2);
        assert_eq!(t.speaker_count(), 2);
    }

    #[test]
    fn far_field_separates_voices_sharing_a_big_common_component() {
        // Every vector carries a huge shared offset (raw cos ~0.99), voices differ in two small dims.
        let v = |a: f32, b: f32| [10.0, 10.0, 10.0, a, b];
        let mut t = SpeakerTracker::new(4, 0.2).far_field();
        let seq = [
            (1.0, 0.0),
            (0.0, 1.0),
            (1.0, 0.1),
            (0.1, 1.0),
            (1.0, 0.0),
            (0.0, 1.0),
        ];
        for (a, b) in seq {
            t.assign(&v(a, b));
        }
        let a1 = t.assign(&v(1.0, 0.05));
        let b1 = t.assign(&v(0.05, 1.0));
        assert_ne!(a1, b1);
        // Plain tracker collapses them (the bug).
        let mut plain = SpeakerTracker::new(4, 0.5);
        plain.assign(&v(1.0, 0.0));
        assert_eq!(plain.assign(&v(0.0, 1.0)), 1);
    }

    #[test]
    fn centroid_moves_toward_new_evidence() {
        let mut t = SpeakerTracker::new(3, 0.5);
        t.assign(&[1.0, 0.0]);
        t.assign(&[0.8, 0.6]);
        let c = &t.centroids[0];
        assert!(c[1] > 0.2, "centroid should have moved: {c:?}");
    }
}

/// Known-answer check against the real model: two halves of one speaker's
/// recording must be far more similar than a speaker vs. silence-noise vector.
/// Runs only when the model and a test WAV are present.
#[cfg(all(test, feature = "wav"))]
mod model_tests {
    use super::*;

    #[test]
    fn same_speaker_halves_are_similar() {
        let model = dirs_fallback().join(DEFAULT_MODEL_FILE);
        let Ok(wav) = std::env::var("SPEAKER_TEST_WAV") else {
            return;
        };
        if !model.exists() {
            return;
        }
        let chunk = crate::types::read_wav_file(std::path::Path::new(&wav)).expect("wav");
        let s = crate::types::resample(&chunk.samples, chunk.sample_rate, 16000);
        let (a, b) = s.split_at(s.len() / 2);
        let mut e = SpeakerEmbedder::new(&model).expect("model");
        let ea = normalize(Array1::from_vec(e.embed(a).unwrap()));
        let eb = normalize(Array1::from_vec(e.embed(b).unwrap()));
        let noise: Vec<f32> = (0..16000 * 2)
            .map(|i| ((i * 7919) % 13) as f32 / 65.0 - 0.1)
            .collect();
        let en = normalize(Array1::from_vec(e.embed(&noise).unwrap()));
        let same = ea.dot(&eb);
        let diff = ea.dot(&en);
        eprintln!("same-speaker cos={same:.3}  speaker-vs-noise cos={diff:.3}");
        assert!(same > 0.5, "same speaker halves should match: {same}");
        assert!(
            same > diff + 0.15,
            "speaker must beat noise: {same} vs {diff}"
        );
    }

    fn dirs_fallback() -> std::path::PathBuf {
        std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".supervox/models"))
            .unwrap_or_default()
    }
}
