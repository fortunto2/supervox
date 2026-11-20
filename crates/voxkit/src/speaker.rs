//! Speaker embeddings + online clustering for live diarization.
//!
//! One embedding per speech segment (WeSpeaker CAM++ ONNX, 80-bin Kaldi fbank
//! via `knf-rs`), then nearest-centroid assignment with a cosine threshold and
//! a hard cap on the number of speakers. Good for "who of the 2–3 people on
//! stage is talking"; it is not a replacement for offline pyannote.

use std::path::{Path, PathBuf};

use crate::stt::SpeakerId;
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
    voiceprints: Vec<VoicePrint>,
    /// Raw cosine at or above which a segment takes a voiceprint's name.
    voiceprint_threshold: f32,
}

/// A known person: the mean raw embedding of their recorded voice. `emb` is unit length
/// (every loader normalises once, so matching never has to).
#[derive(Debug, Clone)]
pub struct VoicePrint {
    pub name: String,
    pub emb: Array1<f32>,
}

/// Default raw cosine at which a segment takes a voiceprint's name.
pub const DEFAULT_VOICEPRINT_THRESHOLD: f32 = 0.65; // 19.11 room: his segments 0.76-0.82, other speakers median 0.02-0.08

/// Longest stretch used for a voiceprint, seconds.
pub const VOICEPRINT_MAX_SECS: usize = 30;
const VOICEPRINT_CHUNK_SECS: usize = 10;

/// The most speech-like `VOICEPRINT_MAX_SECS` of a 16 kHz clip: the window
/// with the highest mean energy (hop 1 s). A shorter clip is returned whole.
pub fn best_window(samples: &[f32]) -> &[f32] {
    let win = VOICEPRINT_MAX_SECS * 16_000;
    if samples.len() <= win {
        return samples;
    }
    let sec = 16_000;
    let energy: Vec<f64> = samples
        .chunks(sec)
        .map(|c| c.iter().map(|x| (*x as f64).powi(2)).sum::<f64>())
        .collect();
    let n = VOICEPRINT_MAX_SECS;
    let mut sum: f64 = energy.iter().take(n).sum();
    let (mut best, mut best_at) = (sum, 0usize);
    for start in 1..=energy.len().saturating_sub(n) {
        sum += energy[start + n - 1] - energy[start - 1];
        if sum > best {
            (best, best_at) = (sum, start);
        }
    }
    let from = best_at * sec;
    &samples[from..(from + win).min(samples.len())]
}

/// Mean (raw, unit-length) embedding of a clip: 10 s chunks, averaged.
pub fn embed_voiceprint(
    embedder: &mut SpeakerEmbedder,
    samples: &[f32],
) -> Result<Array1<f32>, SpeakerError> {
    let clip = best_window(samples);
    let chunk = VOICEPRINT_CHUNK_SECS * 16_000;
    let mut acc: Option<Array1<f32>> = None;
    for c in clip.chunks(chunk) {
        // A tail under MIN_EMBED_SECS carries too little voice, unless it is all there is.
        if c.len() < (MIN_EMBED_SECS * 16_000.0) as usize && acc.is_some() {
            continue;
        }
        let e = normalize(Array1::from_vec(embedder.embed(c)?));
        acc = Some(match acc {
            Some(a) => a + e,
            None => e,
        });
    }
    acc.map(normalize).ok_or(SpeakerError::NoOutput)
}

fn emb_cache_path(wav: &Path) -> PathBuf {
    wav.with_extension("emb")
}

fn read_emb_cache(path: &Path, wav_mtime: std::time::SystemTime) -> Option<Array1<f32>> {
    if std::fs::metadata(path).ok()?.modified().ok()? < wav_mtime {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let v: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    Some(normalize(Array1::from_vec(v)))
}

/// Load `<dir>/<Name>.wav` voiceprints, embedding each with `embedder` and
/// caching the result as `<Name>.emb` (rebuilt when the wav is newer).
/// A file that cannot be read is skipped with a warning.
pub fn load_voiceprints(dir: &Path, embedder: &mut SpeakerEmbedder) -> Vec<VoicePrint> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut wavs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();
    let mut out = Vec::new();
    for wav in wavs {
        let Some(name) = wav.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let mtime = std::fs::metadata(&wav).and_then(|m| m.modified());
        let cache = emb_cache_path(&wav);
        if let Some(e) = mtime.ok().and_then(|t| read_emb_cache(&cache, t)) {
            out.push(VoicePrint { name, emb: e });
            continue;
        }
        let emb = crate::types::read_wav_file(&wav)
            .map_err(|e| e.to_string())
            .and_then(|c| {
                let s = crate::types::resample(&c.samples, c.sample_rate, 16_000);
                embed_voiceprint(embedder, &s).map_err(|e| e.to_string())
            });
        match emb {
            Ok(e) => {
                let bytes: Vec<u8> = e.iter().flat_map(|x| x.to_le_bytes()).collect();
                if let Err(err) = std::fs::write(&cache, bytes) {
                    tracing::warn!("voiceprint cache {}: {err}", cache.display());
                }
                out.push(VoicePrint { name, emb: e });
            }
            Err(e) => tracing::warn!("voiceprint {} skipped: {e}", wav.display()),
        }
    }
    out
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
            voiceprints: Vec::new(),
            voiceprint_threshold: DEFAULT_VOICEPRINT_THRESHOLD,
        }
    }

    /// Name a segment whose raw embedding is within `threshold` (cosine) of a voiceprint.
    pub fn with_voiceprints(mut self, voiceprints: Vec<VoicePrint>, threshold: f32) -> Self {
        self.voiceprints = voiceprints;
        self.voiceprint_threshold = threshold;
        self
    }

    /// [`assign`](Self::assign), then who to show: the voiceprint's name when the
    /// segment's raw embedding is close to one, else the cluster.
    ///
    /// One person often lands in several clusters (S1/S2/S3 for one voice, 19.11), so the name
    /// is decided per segment, whatever its cluster. Raw cosine, as in the talk skill's owner
    /// check (own clips 0.60-0.89, others <= 0.48).
    pub fn assign_display(&mut self, embedding: &[f32]) -> SpeakerId {
        let e = normalize(Array1::from_vec(embedding.to_vec()));
        let c = self.assign_unit(&e);
        let best = self
            .voiceprints
            .iter()
            .enumerate()
            .map(|(v, vp)| (v, raw_similarity(&e, &vp.emb)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, sim)) = best {
            tracing::debug!(
                "voiceprint raw sim {sim:.3} -> {}",
                self.voiceprints[v].name
            );
            if sim >= self.voiceprint_threshold {
                return SpeakerId::Named(self.voiceprints[v].name.clone());
            }
        }
        SpeakerId::Cluster(c)
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
        self.assign_unit(&normalize(Array1::from_vec(embedding.to_vec())))
    }

    /// [`assign`](Self::assign) for an embedding that is already unit length.
    fn assign_unit(&mut self, e: &Array1<f32>) -> usize {
        match &mut self.sum {
            Some(s) => *s += e,
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
            Some(m) => normalize(e - m),
            None => e.clone(),
        };
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.centroids.iter().enumerate() {
            let sim = match &mean {
                Some(m) => ec.dot(&normalize(c - m)),
                None => raw_similarity(e, c),
            };
            if best.is_none_or(|(_, b)| sim > b) {
                best = Some((i, sim));
            }
        }
        let idx = match best {
            Some((i, sim)) if sim >= threshold || self.centroids.len() >= self.max_speakers => {
                let n = self.counts[i] as f32;
                self.centroids[i] = normalize((&self.centroids[i] * n + e) / (n + 1.0));
                self.counts[i] += 1;
                i
            }
            _ => {
                self.centroids.push(e.clone());
                self.counts.push(1);
                self.centroids.len() - 1
            }
        };
        self.last = Some(idx + 1);
        idx + 1
    }

    /// Last assigned speaker, for short segments that were not embedded.
    pub fn last(&self) -> Option<SpeakerId> {
        self.last.map(SpeakerId::Cluster)
    }

    pub fn speaker_count(&self) -> usize {
        self.centroids.len()
    }
}

fn ort_err<R>(e: ort::Error<R>) -> SpeakerError {
    SpeakerError::Ort(e.to_string())
}

/// Unit-length copy of `v` (zero vector unchanged).
pub fn normalize(v: Array1<f32>) -> Array1<f32> {
    let n = v.dot(&v).sqrt();
    if n > 0.0 { v / n } else { v }
}

/// Cosine of two unit-length embeddings, no centering.
pub fn raw_similarity(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
    a.dot(b)
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
        assert_eq!(t.last(), Some(SpeakerId::Cluster(1)));
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

    fn feed_two_voices(t: &mut SpeakerTracker) {
        let v = |a: f32, b: f32| [0.5, 0.5, 0.5, a, b];
        for (a, b) in [
            (1.0, 0.0),
            (0.0, 1.0),
            (1.0, 0.1),
            (0.1, 1.0),
            (1.0, 0.0),
            (0.0, 1.0),
        ] {
            t.assign(&v(a, b));
        }
        t.assign(&v(1.0, 0.05));
        t.assign(&v(0.05, 1.0));
    }

    fn print(name: &str, a: f32, b: f32) -> VoicePrint {
        VoicePrint {
            name: name.into(),
            emb: normalize(Array1::from_vec(vec![0.5, 0.5, 0.5, a, b])),
        }
    }

    #[test]
    fn voiceprint_names_the_matching_segment_and_the_other_stays_anonymous() {
        let mut t = SpeakerTracker::new(4, 0.2)
            .far_field()
            .with_voiceprints(vec![print("Ann", 0.0, 1.0)], 0.5);
        feed_two_voices(&mut t);
        let b = t.assign_display(&[0.5, 0.5, 0.5, 0.05, 1.0]);
        let a = t.assign_display(&[0.5, 0.5, 0.5, 1.0, 0.05]);
        assert_eq!(b, SpeakerId::Named("Ann".into()));
        assert!(
            matches!(a, SpeakerId::Cluster(_)),
            "the other voice stays S<n>: {a:?}"
        );
    }

    #[test]
    fn a_short_session_still_names_his_voice_by_raw_similarity() {
        // 19.11: five segments of Rustam alone were all labelled S1: the raw cosine to the
        // voiceprint names a segment from the first one, no session mean needed.
        let mut t = SpeakerTracker::new(4, 0.2)
            .far_field()
            .with_voiceprints(vec![print("Ann", 0.0, 1.0)], 0.5);
        for _ in 0..3 {
            assert_eq!(
                t.assign_display(&[0.5, 0.5, 0.5, 0.05, 1.0]),
                SpeakerId::Named("Ann".into())
            );
        }
        assert!(
            matches!(
                t.assign_display(&[0.5, 0.5, 0.5, 1.0, 0.05]),
                SpeakerId::Cluster(_)
            ),
            "a different voice stays anonymous"
        );
    }

    #[test]
    fn below_threshold_stays_anonymous() {
        let mut t = SpeakerTracker::new(4, 0.2)
            .far_field()
            .with_voiceprints(vec![print("Ann", -1.0, -1.0)], 0.9);
        feed_two_voices(&mut t);
        assert!(matches!(
            t.assign_display(&[0.5, 0.5, 0.5, 0.05, 1.0]),
            SpeakerId::Cluster(_)
        ));
    }

    #[test]
    fn best_window_picks_the_loud_30_seconds() {
        let mut x = vec![0.0f32; 16_000 * 20];
        x.extend(vec![0.5f32; 16_000 * 30]);
        x.extend(vec![0.0f32; 16_000 * 20]);
        let w = best_window(&x);
        assert_eq!(w.len(), 16_000 * 30);
        assert!(w.iter().all(|s| *s == 0.5));
        assert_eq!(best_window(&x[..16_000 * 10]).len(), 16_000 * 10);
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
        let model = crate::paths::models_dir().join(DEFAULT_MODEL_FILE);
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
}
