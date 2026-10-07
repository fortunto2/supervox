//! Automatic gain control for far-field speech.
//!
//! Conference halls deliver speech at -40 dBFS and below, under the energy
//! VAD's silence floor. `Agc` tracks the running peak of the input with a slow
//! release and scales it toward `target`, capped at `max_gain` so a silent room
//! is not amplified into noise.

/// Running-peak automatic gain control.
#[derive(Debug, Clone)]
pub struct Agc {
    target: f32,
    max_gain: f32,
    release: f32,
    peak: f32,
}

/// Peak the output is normalised toward (linear amplitude, 0..1).
pub const DEFAULT_TARGET: f32 = 0.5;
/// Maximum amplification (20x = +26 dB).
pub const DEFAULT_MAX_GAIN: f32 = 20.0;
/// Per-sample peak decay at 16 kHz: a 1-second-ish release.
pub const DEFAULT_RELEASE: f32 = 0.9999;

impl Default for Agc {
    fn default() -> Self {
        Self::new(DEFAULT_TARGET, DEFAULT_MAX_GAIN, DEFAULT_RELEASE)
    }
}

impl Agc {
    /// `release` is the per-sample multiplicative decay of the tracked peak
    /// (closer to 1.0 = slower release).
    pub fn new(target: f32, max_gain: f32, release: f32) -> Self {
        Self {
            target,
            max_gain,
            release,
            peak: 0.0,
        }
    }

    /// Current gain that would be applied to the next sample.
    pub fn gain(&self) -> f32 {
        if self.peak <= f32::EPSILON {
            self.max_gain
        } else {
            (self.target / self.peak).clamp(1.0, self.max_gain)
        }
    }

    /// Scale `samples` in place toward the target level.
    pub fn process(&mut self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            let a = s.abs();
            // Instant attack, slow release: never clip a sudden loud onset.
            self.peak = if a > self.peak {
                a
            } else {
                self.peak * self.release
            };
            *s = (*s * self.gain()).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 0.1).sin()).collect()
    }

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |m, &v| m.max(v.abs()))
    }

    #[test]
    fn quiet_speech_is_lifted_toward_target() {
        let mut agc = Agc::default();
        let mut s = sine(0.05, 16000);
        agc.process(&mut s);
        let p = peak(&s);
        assert!((0.4..=0.55).contains(&p), "peak after agc = {p}");
    }

    #[test]
    fn loud_input_is_not_attenuated_or_clipped() {
        let mut agc = Agc::default();
        let mut s = sine(0.9, 16000);
        agc.process(&mut s);
        let p = peak(&s);
        assert!((p - 0.9).abs() < 0.01, "peak = {p}");
    }

    #[test]
    fn gain_is_capped_for_near_silence() {
        let mut agc = Agc::default();
        let mut s = sine(0.001, 16000);
        agc.process(&mut s);
        // 0.001 * 20 = 0.02: still well under the VAD floor region of real speech.
        assert!(peak(&s) <= 0.001 * DEFAULT_MAX_GAIN + 1e-6);
        assert!((agc.gain() - DEFAULT_MAX_GAIN).abs() < 1e-3);
    }

    #[test]
    fn release_recovers_gain_after_a_loud_burst() {
        let mut agc = Agc::default();
        let mut loud = sine(0.9, 1600);
        agc.process(&mut loud);
        assert!(agc.gain() < 1.01);
        let mut silence = vec![0.0f32; 48000]; // 3 s at 16 kHz
        agc.process(&mut silence);
        assert!(agc.gain() > 5.0, "gain after release = {}", agc.gain());
    }
}
