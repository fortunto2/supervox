//! Energy-VAD utterance segmenter shared by the local STT backends (Parakeet, Whisper):
//! cuts on a pause, and on a semantic end-of-phrase when an end-of-turn check says so.

/// Input sample rate of the local STT models (16 kHz mono).
pub(crate) const SAMPLE_RATE: u32 = 16000;
/// Energy-VAD frame length.
pub(crate) const FRAME_MS: u32 = 20;
pub(crate) const FRAME_SAMPLES: usize = (SAMPLE_RATE / 1000 * FRAME_MS) as usize;
/// Default end-of-utterance pause.
pub const DEFAULT_PAUSE_MS: u32 = 1200;
/// Default forced-cut length.
pub const DEFAULT_MAX_SEGMENT_SECS: f32 = 7.0; // a noisy room never goes silent: 15 s made text arrive 15 s late
/// Pre-roll kept before the first speech frame (so onsets are not clipped).
pub(crate) const PRE_ROLL_FRAMES: usize = 15; // 300 ms
/// Silence kept after the last speech frame inside an emitted segment.
pub(crate) const TAIL_FRAMES: usize = 15; // 300 ms

/// The end-of-turn model only looks at the last 8 s of the speech so far.
pub(crate) const TURN_WINDOW_SAMPLES: usize = 8 * SAMPLE_RATE as usize;

/// Smart-turn probability that ends a turn at the early pause.
pub(crate) const SMART_TURN_THRESHOLD: f32 = 0.5;

/// End-of-turn verdict for the speech so far; no model, or a model that fails, says "finished"
/// (the segmenter then behaves as with a fixed pause).
#[cfg(feature = "smart-turn")]
pub(crate) fn turn_finished(model: Option<&mut crate::smart_turn::SmartTurn>, buf: &[f32]) -> bool {
    match model {
        Some(m) => match m.probability(buf) {
            Ok(p) => {
                tracing::debug!("smart-turn p={p:.2}");
                p >= SMART_TURN_THRESHOLD
            }
            Err(e) => {
                tracing::warn!("smart-turn failed: {e}");
                true
            }
        },
        None => true,
    }
}

/// Energy-based utterance segmenter: cuts on a pause, not on a timer.
///
/// Keeps a short pre-roll of silence before speech, drops long stretches of
/// pure silence (Parakeet hallucinates on them), and force-cuts a segment that
/// exceeds `max_samples`.
pub(crate) struct Segmenter {
    pause_frames: usize,
    /// Short pause (frames) at which the end-of-turn check runs; 0 = never (fixed pause only).
    early_frames: usize,
    /// Trailing silence (frames) of the segment closed last.
    pub(crate) closed_after: usize,
    max_samples: usize,
    /// Absolute index (16 kHz samples since the first one pushed) of `buf[0]`.
    buf_start: usize,
    /// Where each segment closed since the last `take_spans` sat in the audio, seconds `(start, end)`.
    closed_spans: Vec<(f32, f32)>,
    buf: Vec<f32>,
    partial: Vec<f32>,
    has_speech: bool,
    trailing_silence: usize,
}

impl Segmenter {
    pub(crate) fn new(pause_frames: usize, max_samples: usize) -> Self {
        Self {
            pause_frames,
            early_frames: 0,
            closed_after: 0,
            max_samples,
            buf_start: 0,
            closed_spans: Vec::new(),
            buf: Vec::new(),
            partial: Vec::new(),
            has_speech: false,
            trailing_silence: 0,
        }
    }

    /// Close early: after `early_frames` of silence ask `turn` (given the speech so far)
    /// whether the speaker is done; if not, wait for the full `pause_frames`.
    pub(crate) fn with_early_check(mut self, early_frames: usize) -> Self {
        self.early_frames = early_frames.min(self.pause_frames);
        self
    }

    /// Fixed-pause push (no end-of-turn check).
    #[cfg(test)]
    fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.push_with(samples, &mut |_| true)
    }

    /// Feed 16 kHz samples; returns zero or more finished utterances.
    pub(crate) fn push_with(
        &mut self,
        samples: &[f32],
        turn: &mut dyn FnMut(&[f32]) -> bool,
    ) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        self.partial.extend_from_slice(samples);
        let mut offset = 0;
        while self.partial.len() - offset >= FRAME_SAMPLES {
            let frame = &self.partial[offset..offset + FRAME_SAMPLES];
            offset += FRAME_SAMPLES;
            let is_speech = rms(frame) >= SILENCE_THRESHOLD;
            self.buf.extend_from_slice(frame);
            if is_speech {
                self.has_speech = true;
                self.trailing_silence = 0;
            } else {
                self.trailing_silence += 1;
            }
            if !self.has_speech {
                // Pure silence so far: keep only the pre-roll.
                let keep = PRE_ROLL_FRAMES * FRAME_SAMPLES;
                if self.buf.len() > keep {
                    let n = self.buf.len() - keep;
                    self.buf.drain(..n);
                    self.buf_start += n;
                }
                continue;
            }
            let early = self.early_frames > 0
                && self.early_frames < self.pause_frames
                && self.trailing_silence == self.early_frames
                && turn(&self.buf[self.buf.len().saturating_sub(TURN_WINDOW_SAMPLES)..]);
            if early || self.trailing_silence >= self.pause_frames {
                self.closed_after = self.trailing_silence;
                // Utterance ended: emit speech + a short tail, keep the rest as pre-roll.
                let drop_tail = (self.trailing_silence - TAIL_FRAMES.min(self.trailing_silence))
                    * FRAME_SAMPLES;
                let cut = self.buf.len().saturating_sub(drop_tail);
                let segment: Vec<f32> = self.buf.drain(..cut).collect();
                self.mark_span(segment.len());
                let keep = PRE_ROLL_FRAMES * FRAME_SAMPLES;
                if self.buf.len() > keep {
                    let n = self.buf.len() - keep;
                    self.buf.drain(..n);
                    self.buf_start += n;
                }
                self.has_speech = false;
                self.trailing_silence = 0;
                out.push(segment);
            } else if self.buf.len() >= self.max_samples {
                // Over the cap with no pause: cut at the quietest spot of the last
                // `SOFT_CUT_WINDOW_SECS`, not mid-word; the rest starts the next segment.
                self.closed_after = 0;
                let cut = soft_cut(&self.buf, SOFT_CUT_WINDOW_SECS * SAMPLE_RATE as usize);
                let segment: Vec<f32> = self.buf.drain(..cut).collect();
                self.mark_span(segment.len());
                self.has_speech = self
                    .buf
                    .chunks(FRAME_SAMPLES)
                    .any(|f| rms(f) >= SILENCE_THRESHOLD);
                out.push(segment);
            }
        }
        self.partial.drain(..offset);
        out
    }

    /// Return whatever speech is still buffered (on stop).
    pub(crate) fn flush(&mut self) -> Option<Vec<f32>> {
        if !self.has_speech || self.buf.len() < FRAME_SAMPLES * 10 {
            self.buf.clear();
            return None;
        }
        self.has_speech = false;
        self.trailing_silence = 0;
        self.mark_span(self.buf.len());
        Some(std::mem::take(&mut self.buf))
    }

    /// Spans of the segments returned since the last call, in the same order.
    pub(crate) fn take_spans(&mut self) -> Vec<(f32, f32)> {
        std::mem::take(&mut self.closed_spans)
    }

    /// Record that the `len` samples taken from the front of `buf` are the segment just
    /// closed, and move `buf_start` past them.
    fn mark_span(&mut self, len: usize) {
        let rate = SAMPLE_RATE as f32;
        self.closed_spans.push((
            self.buf_start as f32 / rate,
            (self.buf_start + len) as f32 / rate,
        ));
        self.buf_start += len;
    }
}

/// How far back from the cap a forced cut may look for a quiet frame.
const SOFT_CUT_WINDOW_SECS: usize = 2;

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
}

/// Sample index to cut `buf` at: the middle of the lowest-energy 20 ms frame among those
/// in the last `window` samples (the latest one on a tie), never past the end of `buf`.
fn soft_cut(buf: &[f32], window: usize) -> usize {
    let window = window.min(buf.len() / 2);
    let first = (buf.len() - window) / FRAME_SAMPLES;
    let frames = buf.len() / FRAME_SAMPLES;
    let quietest = (first..frames)
        .min_by(|&a, &b| {
            let e = |i: usize| rms(&buf[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]);
            e(a).total_cmp(&e(b)).then(b.cmp(&a))
        })
        .unwrap_or(frames.saturating_sub(1));
    ((quietest * FRAME_SAMPLES) + FRAME_SAMPLES / 2).min(buf.len())
}

/// RMS threshold below which audio is considered silence.
/// Parakeet hallucinates on silence — must skip quiet chunks.
const SILENCE_THRESHOLD: f32 = 0.005;

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32) -> Vec<f32> {
        let n = (secs * SAMPLE_RATE as f32) as usize;
        (0..n).map(|i| 0.3 * ((i as f32) * 0.05).sin()).collect()
    }
    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * SAMPLE_RATE as f32) as usize]
    }

    #[test]
    fn pause_closes_an_utterance_and_timer_does_not() {
        // 1.2 s pause, 15 s cap. 3 s of speech must NOT be cut at 2 s.
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        let mut out = seg.push(&tone(3.0));
        assert!(out.is_empty(), "no cut while speech continues");
        out.extend(seg.push(&silence(1.5)));
        assert_eq!(out.len(), 1, "one utterance after the pause");
        let secs = out[0].len() as f32 / SAMPLE_RATE as f32;
        assert!(
            secs > 3.0 && secs < 3.7,
            "speech + ~300 ms tail, got {secs}"
        );
    }

    #[test]
    fn spans_follow_the_audio_clock_not_the_wall_clock() {
        // 2 s silence, 3 s speech, 2 s silence, 2 s speech, 2 s silence: two utterances at
        // 2-5 s and 7-9 s of audio, however the input is chopped into blocks.
        let mut audio = silence(2.0);
        audio.extend(tone(3.0));
        audio.extend(silence(2.0));
        audio.extend(tone(2.0));
        audio.extend(silence(2.0));
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        let mut out = Vec::new();
        for block in audio.chunks(3_000) {
            out.extend(seg.push(block));
        }
        let spans = seg.take_spans();
        assert_eq!(out.len(), 2);
        assert_eq!(spans.len(), 2);
        let (a, b) = (spans[0], spans[1]);
        assert!((1.5..2.1).contains(&a.0), "pre-roll before 2 s: {a:?}");
        assert!(
            (4.9..5.5).contains(&a.1),
            "ends at the speech end plus tail: {a:?}"
        );
        assert!((6.5..7.1).contains(&b.0), "{b:?}");
        assert!((8.9..9.5).contains(&b.1), "{b:?}");
        let len_s = out[0].len() as f32 / SAMPLE_RATE as f32;
        assert!(
            (a.1 - a.0 - len_s).abs() < 0.01,
            "span length = audio length"
        );
        assert!(seg.take_spans().is_empty(), "taken once");
    }

    #[test]
    fn a_forced_cut_lands_in_the_dip_not_mid_word() {
        // 9 s of continuous speech with one 40 ms dip at 6.0 s; cap 7 s, no pause long
        // enough to close (dip < 1.2 s). A hard cut would land at 7.0 s.
        let mut audio = tone(9.0);
        let dip = (6.0 * SAMPLE_RATE as f32) as usize;
        for x in &mut audio[dip..dip + FRAME_SAMPLES * 2] {
            *x = 0.001;
        }
        let mut seg = Segmenter::new(60, 7 * SAMPLE_RATE as usize);
        let out = seg.push(&audio);
        let spans = seg.take_spans();
        assert!(!out.is_empty());
        let end = spans[0].1;
        assert!((5.99..6.06).contains(&end), "cut in the dip: {end}");
        assert!((spans[0].1 - spans[0].0 - out[0].len() as f32 / SAMPLE_RATE as f32).abs() < 0.01);
        // the next segment starts where this one ended
        assert!(spans.len() < 2 || (spans[1].0 - end).abs() < 0.01);
        assert!(seg.has_speech, "the rest of the speech is still buffered");
    }

    #[test]
    fn soft_cut_never_exceeds_the_cap_or_the_window() {
        let buf = tone(7.0);
        let cut = soft_cut(&buf, 2 * SAMPLE_RATE as usize);
        assert!(cut <= buf.len());
        assert!(cut >= buf.len() - 2 * SAMPLE_RATE as usize);
    }

    #[test]
    fn pure_silence_is_dropped_not_emitted() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        assert!(seg.push(&silence(10.0)).is_empty());
        assert!(seg.flush().is_none());
        assert!(seg.buf.len() <= PRE_ROLL_FRAMES * FRAME_SAMPLES);
    }

    #[test]
    fn long_monologue_is_force_cut_at_cap() {
        let mut seg = Segmenter::new(60, 4 * SAMPLE_RATE as usize);
        let out = seg.push(&tone(9.0));
        assert_eq!(out.len(), 2, "two forced cuts in 9 s at a 4 s cap");
    }

    #[test]
    fn finished_turn_closes_at_the_early_pause() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize).with_early_check(15);
        let mut asked = 0;
        let mut yes = |_: &[f32]| {
            asked += 1;
            true
        };
        let mut out = seg.push_with(&tone(2.0), &mut yes);
        assert!(out.is_empty());
        out.extend(seg.push_with(&silence(0.5), &mut yes));
        assert_eq!(out.len(), 1, "closed 300 ms into the pause");
        assert_eq!(seg.closed_after, 15);
        assert_eq!(asked, 1);
    }

    #[test]
    fn unfinished_turn_waits_for_the_full_pause_and_is_asked_once() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize).with_early_check(15);
        let mut asked = 0;
        let mut no = |_: &[f32]| {
            asked += 1;
            false
        };
        let mut out = seg.push_with(&tone(2.0), &mut no);
        out.extend(seg.push_with(&silence(0.8), &mut no));
        assert!(out.is_empty(), "held through 800 ms");
        out.extend(seg.push_with(&silence(0.6), &mut no));
        assert_eq!(out.len(), 1, "fixed pause is the fallback");
        assert_eq!(seg.closed_after, 60);
        assert_eq!(asked, 1);
    }

    #[test]
    fn speech_after_the_early_pause_resets_the_check() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize).with_early_check(15);
        let mut asked = 0;
        let mut no = |_: &[f32]| {
            asked += 1;
            false
        };
        let mut out = seg.push_with(&tone(1.0), &mut no);
        out.extend(seg.push_with(&silence(0.4), &mut no));
        out.extend(seg.push_with(&tone(1.0), &mut no));
        out.extend(seg.push_with(&silence(0.4), &mut no));
        assert!(out.is_empty());
        assert_eq!(asked, 2, "asked at each pause, one segment so far");
    }

    #[test]
    fn flush_returns_trailing_speech() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        assert!(seg.push(&tone(1.0)).is_empty());
        assert!(seg.flush().is_some());
    }
}
