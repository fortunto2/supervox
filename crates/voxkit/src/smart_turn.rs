//! End-of-turn detection: smart-turn v3.2 (pipecat-ai, BSD-2, 8 MB int8 ONNX).
//!
//! Given the speech so far (16 kHz mono, ending at a short pause) it returns the probability
//! that the speaker has finished their turn. Port of `talk/scripts/turns.py` `SmartTurn`:
//! last 8 s, zero-padded at the start, zero-mean/unit-variance, Whisper log-mel (80 bins).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ort::session::Session;
use ort::value::Tensor;
use realfft::{RealFftPlanner, RealToComplex};

const SR: usize = 16_000;
const WINDOW_SAMPLES: usize = 8 * SR;
const N_FFT: usize = 400;
const HOP: usize = 160;
const N_MELS: usize = 80;
const N_BINS: usize = N_FFT / 2 + 1;
const N_FRAMES: usize = WINDOW_SAMPLES / HOP; // 800

/// Default model location (shared with the talk skill's cache).
pub fn default_model_path() -> PathBuf {
    crate::paths::home_dir().join(".cache/solo-tts/smart-turn/smart-turn-v3.2-cpu.onnx")
}

/// Smart-turn model plus the Whisper feature extractor it expects.
pub struct SmartTurn {
    session: Session,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    filters: Vec<f32>, // N_MELS x N_BINS
    /// Per mel row, the bins with non-zero weight (the triangles are narrow).
    bands: Vec<std::ops::Range<usize>>,
}

impl SmartTurn {
    pub fn new(model: &Path) -> Result<Self, String> {
        let load = || -> ort::Result<Session> {
            let mut builder = Session::builder()?.with_intra_threads(2)?;
            builder.commit_from_file(model)
        };
        let session = load().map_err(|e| format!("smart-turn model {}: {e}", model.display()))?;
        let filters = mel_filters();
        Ok(Self {
            session,
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT),
            window: (0..N_FFT)
                .map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / N_FFT as f32).cos())
                .collect(),
            bands: mel_bands(&filters),
            filters,
        })
    }

    /// Probability that the speech in `samples` (ends at the pause) is a finished turn.
    pub fn probability(&mut self, samples: &[f32]) -> Result<f32, String> {
        let feats = self.features(samples);
        let input = Tensor::from_array((vec![1_i64, N_MELS as i64, N_FRAMES as i64], feats))
            .map_err(|e| e.to_string())?;
        let out = self
            .session
            .run(ort::inputs!["input_features" => input])
            .map_err(|e| e.to_string())?;
        let (_, data) = out[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        data.first().copied().ok_or_else(|| "empty output".into())
    }

    /// Whisper log-mel features, `[N_MELS][N_FRAMES]` flattened, of the last 8 s.
    pub fn features(&mut self, samples: &[f32]) -> Vec<f32> {
        let tail = &samples[samples.len().saturating_sub(WINDOW_SAMPLES)..];
        let mut x = vec![0.0f32; WINDOW_SAMPLES - tail.len()];
        x.extend_from_slice(tail);
        let mean = x.iter().sum::<f32>() / x.len() as f32;
        let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / x.len() as f32;
        let inv = 1.0 / (var + 1e-7).sqrt();
        x.iter_mut().for_each(|v| *v = (*v - mean) * inv);

        // Reflect padding (center=True), as torch/numpy stft does.
        let pad = N_FFT / 2;
        let mut p = Vec::with_capacity(x.len() + 2 * pad);
        p.extend((1..=pad).rev().map(|i| x[i]));
        p.extend_from_slice(&x);
        p.extend((1..=pad).map(|i| x[x.len() - 1 - i]));

        let mut log_mel = vec![0.0f32; N_MELS * N_FRAMES];
        let mut frame = vec![0.0f32; N_FFT];
        let mut spec = self.fft.make_output_vec();
        let mut power = vec![0.0f32; N_BINS];
        for t in 0..N_FRAMES {
            // The 801st frame is dropped, as in the Whisper extractor.
            for (i, f) in frame.iter_mut().enumerate() {
                *f = p[t * HOP + i] * self.window[i];
            }
            self.fft.process(&mut frame, &mut spec).expect("fft size");
            for (pw, c) in power.iter_mut().zip(&spec) {
                *pw = c.re * c.re + c.im * c.im;
            }
            for m in 0..N_MELS {
                let band = self.bands[m].clone();
                let row = &self.filters[m * N_BINS + band.start..m * N_BINS + band.end];
                let s: f32 = row.iter().zip(&power[band]).map(|(a, b)| a * b).sum();
                log_mel[m * N_FRAMES + t] = s.max(1e-10).log10();
            }
        }
        let max = log_mel.iter().cloned().fold(f32::MIN, f32::max);
        log_mel
            .iter_mut()
            .for_each(|v| *v = (v.max(max - 8.0) + 4.0) / 4.0);
        log_mel
    }
}

fn hz_to_mel(f: f64) -> f64 {
    if f < 1000.0 {
        f / (200.0 / 3.0)
    } else {
        15.0 + (f / 1000.0).ln() / (6.4f64.ln() / 27.0)
    }
}

fn mel_to_hz(m: f64) -> f64 {
    if m < 15.0 {
        m * (200.0 / 3.0)
    } else {
        1000.0 * ((6.4f64.ln() / 27.0) * (m - 15.0)).exp()
    }
}

/// The span of bins with non-zero weight in each row of a `[N_MELS][N_BINS]` filterbank.
fn mel_bands(filters: &[f32]) -> Vec<std::ops::Range<usize>> {
    (0..N_MELS)
        .map(|m| {
            let row = &filters[m * N_BINS..(m + 1) * N_BINS];
            let lo = row.iter().position(|w| *w != 0.0).unwrap_or(0);
            let hi = row.iter().rposition(|w| *w != 0.0).map_or(0, |i| i + 1);
            lo..hi.max(lo)
        })
        .collect()
}

/// Slaney-scale, slaney-normalised mel filterbank (HF `mel_filter_bank`), `[N_MELS][N_BINS]`.
fn mel_filters() -> Vec<f32> {
    let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(8000.0));
    let pts: Vec<f64> = (0..N_MELS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (N_MELS + 1) as f64))
        .collect();
    let mut out = vec![0.0f32; N_MELS * N_BINS];
    for m in 0..N_MELS {
        let (d0, d1) = (pts[m + 1] - pts[m], pts[m + 2] - pts[m + 1]);
        let enorm = 2.0 / (pts[m + 2] - pts[m]);
        for b in 0..N_BINS {
            let f = b as f64 * (SR as f64 / 2.0) / (N_BINS - 1) as f64;
            let down = -(pts[m] - f) / d0;
            let up = (pts[m + 2] - f) / d1;
            out[m * N_BINS + b] = (down.min(up).max(0.0) * enorm) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Option<SmartTurn> {
        let p = default_model_path();
        p.exists().then(|| SmartTurn::new(&p).unwrap())
    }

    #[test]
    fn mel_scale_roundtrips() {
        for f in [0.0, 440.0, 999.0, 1000.0, 4000.0, 8000.0] {
            assert!((mel_to_hz(hz_to_mel(f)) - f).abs() < 1e-6, "{f}");
        }
    }

    #[test]
    fn filterbank_covers_the_band() {
        let fb = mel_filters();
        assert_eq!(fb.len(), N_MELS * N_BINS);
        assert!(fb.iter().all(|v| *v >= 0.0));
        // every mel row has weight somewhere
        for m in 0..N_MELS {
            assert!(fb[m * N_BINS..(m + 1) * N_BINS].iter().sum::<f32>() > 0.0);
        }
    }

    #[test]
    fn bands_hold_every_non_zero_weight() {
        let fb = mel_filters();
        for (m, band) in mel_bands(&fb).into_iter().enumerate() {
            let row = &fb[m * N_BINS..(m + 1) * N_BINS];
            assert!(band.len() < N_BINS / 4, "row {m} is sparse: {band:?}");
            let outside: f32 = row[..band.start].iter().chain(&row[band.end..]).sum();
            assert_eq!(outside, 0.0, "row {m}");
        }
    }

    #[test]
    fn tone_peaks_in_the_right_mel_row() {
        let Some(mut st) = model() else { return };
        let tone: Vec<f32> = (0..SR * 2)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR as f32).sin())
            .collect();
        let f = st.features(&tone);
        let frame = N_FRAMES - 10;
        let peak = (0..N_MELS)
            .max_by(|a, b| {
                f[a * N_FRAMES + frame]
                    .partial_cmp(&f[b * N_FRAMES + frame])
                    .unwrap()
            })
            .unwrap();
        let center = mel_to_hz(
            hz_to_mel(0.0) + (hz_to_mel(8000.0) - hz_to_mel(0.0)) * (peak + 1) as f64 / 81.0,
        );
        assert!((center - 1000.0).abs() < 120.0, "peak at {center} Hz");
        assert!(f.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn probability_is_a_probability() {
        let Some(mut st) = model() else { return };
        let p = st.probability(&vec![0.01; SR]).unwrap();
        assert!((0.0..=1.0).contains(&p), "{p}");
    }
}
