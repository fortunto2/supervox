//! Local Parakeet TDT STT backend using parakeet-rs (ONNX Runtime).
//!
//! NVIDIA Parakeet TDT 0.6B — fast, accurate, offline speech-to-text.
//! ~5x faster than Whisper base on Apple Silicon, better accuracy.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use parakeet_rs::Transcriber;

use crate::stt::{StreamingSttBackend, SttError, SttInput, SttStreamError, TranscriptEvent};
use crate::types::{AudioChunk, Transcript};

/// Local Parakeet TDT STT backend.
pub struct ParakeetStt {
    model_dir: PathBuf,
    language: String,
    /// End-of-utterance pause: trailing silence that closes a segment.
    pause_ms: u32,
    /// Hard cap on one segment, so a speaker who never pauses still gets subtitles.
    max_segment_secs: f32,
    /// Smart-turn model + early check pause (ms): close a segment at the short pause when
    /// the speaker has finished, else wait for the full `pause_ms`.
    #[cfg(feature = "smart-turn")]
    smart_turn: Option<(PathBuf, u32)>,
    /// Allowed languages (ISO codes). Parakeet v3 answers unclear speech with text in some
    /// other language; a segment whose letters fall outside these scripts is dropped.
    languages: Vec<String>,
    /// Run the ONNX encoder through CoreML (Apple GPU) instead of CPU.
    #[cfg(feature = "coreml")]
    coreml: bool,
    /// Speaker diarization: (embedding model path, max speakers).
    #[cfg(feature = "speaker")]
    diarize: Option<(PathBuf, usize)>,
}

impl ParakeetStt {
    /// Create a new Parakeet STT backend.
    ///
    /// `model_dir` must contain: encoder-model.onnx, decoder_joint-model.onnx, vocab.txt
    pub fn new(model_dir: PathBuf, language: &str) -> Self {
        Self {
            model_dir,
            language: language.to_string(),
            pause_ms: DEFAULT_PAUSE_MS,
            max_segment_secs: DEFAULT_MAX_SEGMENT_SECS,
            #[cfg(feature = "smart-turn")]
            smart_turn: None,
            languages: vec!["ru".into(), "en".into()],
            #[cfg(feature = "coreml")]
            coreml: false,
            #[cfg(feature = "speaker")]
            diarize: None,
        }
    }

    /// Trailing silence (ms) that ends an utterance. Default 1200.
    pub fn with_pause_ms(mut self, pause_ms: u32) -> Self {
        self.pause_ms = pause_ms.max(200);
        self
    }

    /// Longest segment before a forced cut. Default 15 s.
    pub fn with_max_segment_secs(mut self, secs: f32) -> Self {
        self.max_segment_secs = secs.max(2.0);
        self
    }

    /// End segments on semantic end-of-phrase: after `early_ms` of silence ask smart-turn.
    #[cfg(feature = "smart-turn")]
    pub fn with_smart_turn(mut self, model: PathBuf, early_ms: u32) -> Self {
        self.smart_turn = Some((model, early_ms.max(100)));
        self
    }

    /// Restrict output to these languages (empty = no filter).
    pub fn with_languages(mut self, languages: Vec<String>) -> Self {
        self.languages = languages;
        self
    }

    /// Use CoreML (Apple GPU) for the ASR encoder.
    #[cfg(feature = "coreml")]
    pub fn with_coreml(mut self, on: bool) -> Self {
        self.coreml = on;
        self
    }

    /// Label each segment with a speaker index (1..=max_speakers).
    #[cfg(feature = "speaker")]
    pub fn with_diarization(mut self, model_path: PathBuf, max_speakers: usize) -> Self {
        self.diarize = Some((model_path, max_speakers));
        self
    }

    fn early_frames(&self) -> usize {
        #[cfg(feature = "smart-turn")]
        if let Some((_, ms)) = &self.smart_turn {
            return (*ms / FRAME_MS).max(1) as usize;
        }
        0
    }

    /// Batch-transcribe an AudioChunk (one-shot).
    pub fn transcribe_file(
        model_dir: &Path,
        audio: &AudioChunk,
        language: &str,
    ) -> Result<Transcript, SttError> {
        let mut engine =
            parakeet_rs::ParakeetTDT::from_pretrained(model_dir.to_str().unwrap_or(""), None)
                .map_err(|e| SttError::Other(format!("Load Parakeet model: {e}")))?;

        let result = engine
            .transcribe_samples(
                audio.samples.clone(),
                audio.sample_rate,
                1, // mono
                Some(parakeet_rs::TimestampMode::Words),
            )
            .map_err(|e| SttError::Other(format!("Parakeet transcription failed: {e}")))?;

        let text = result.text.clone();
        if text.trim().is_empty() {
            return Err(SttError::Empty);
        }

        let segments = result
            .tokens
            .iter()
            .map(|t| crate::types::Segment {
                start: t.start as f64,
                end: t.end as f64,
                text: t.text.clone(),
                speaker: None,
            })
            .collect();

        Ok(Transcript {
            text,
            segments,
            language: Some(language.to_string()),
            duration_secs: audio.duration_ms as f64 / 1000.0,
        })
    }
}

#[async_trait::async_trait]
impl StreamingSttBackend for ParakeetStt {
    fn display_name(&self) -> &str {
        "parakeet-tdt"
    }

    async fn connect(
        &self,
    ) -> Result<(mpsc::Sender<SttInput>, mpsc::Receiver<TranscriptEvent>), SttStreamError> {
        let model_dir = self.model_dir.clone();
        let _language = self.language.clone();
        let pause_frames = (self.pause_ms / FRAME_MS).max(1) as usize;
        let max_segment_samples = (self.max_segment_secs * SAMPLE_RATE as f32) as usize;

        // Load Parakeet model
        #[cfg(feature = "coreml")]
        let use_coreml = self.coreml;
        #[cfg(not(feature = "coreml"))]
        let use_coreml = false;
        let mut engine = load_engine(&model_dir, use_coreml)
            .map_err(|e| SttStreamError::Connection(format!("Load Parakeet model: {e}")))?;

        let languages = self.languages.clone();
        let early_frames = self.early_frames();
        #[cfg(feature = "smart-turn")]
        let mut turn_model = match &self.smart_turn {
            Some((path, _)) => match crate::smart_turn::SmartTurn::new(path) {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::warn!("smart-turn off, fixed pause only: {e}");
                    None
                }
            },
            None => None,
        };
        #[cfg(feature = "smart-turn")]
        let early_frames = if turn_model.is_some() {
            early_frames
        } else {
            0
        };
        #[cfg(not(feature = "smart-turn"))]
        let early_frames = 0usize;

        #[cfg(feature = "speaker")]
        let mut diarizer = match &self.diarize {
            Some((path, max)) => {
                let embedder = crate::speaker::SpeakerEmbedder::new(path)
                    .map_err(|e| SttStreamError::Connection(format!("Load speaker model: {e}")))?;
                Some((
                    embedder,
                    crate::speaker::SpeakerTracker::new(*max, SPEAKER_THRESHOLD).far_field(),
                ))
            }
            None => None,
        };

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        tokio::task::spawn_blocking(move || {
            let mut seg =
                Segmenter::new(pause_frames, max_segment_samples).with_early_check(early_frames);
            let mut agc = crate::agc::Agc::default();
            let mut item_counter = 0u64;

            let mut agent =
                crate::agent_voice::AgentVoice::new(&crate::agent_voice::default_path());
            let mut emit = |segment: Vec<f32>, item_counter: &mut u64, silence_frames: usize| {
                let pause_secs = silence_frames as f64 * FRAME_MS as f64 / 1000.0;
                *item_counter += 1;
                let item_id = format!("parakeet_{item_counter}");
                // Loudspeaker audio of the agent's own voice: exact text from its
                // speaking log, never embedded (it collapses far-field clustering).
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let seg_end = now - pause_secs;
                let seg_start = seg_end - segment.len() as f64 / SAMPLE_RATE as f64;
                match agent.classify(seg_start, seg_end) {
                    crate::agent_voice::Verdict::Agent(text) => {
                        let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                            item_id,
                            text,
                            speaker: Some(crate::agent_voice::AGENT_SPEAKER),
                        });
                        return;
                    }
                    crate::agent_voice::Verdict::Duplicate => return,
                    crate::agent_voice::Verdict::Human => {}
                }
                #[allow(unused_mut)]
                let mut speaker: Option<usize> = None;
                #[cfg(feature = "speaker")]
                if let Some((embedder, tracker)) = diarizer.as_mut() {
                    if segment.len() as f32 / SAMPLE_RATE as f32 >= crate::speaker::MIN_EMBED_SECS {
                        match embedder.embed(&segment) {
                            Ok(e) => speaker = Some(tracker.assign(&e)),
                            Err(e) => tracing::warn!("speaker embedding failed: {e}"),
                        }
                    }
                    if speaker.is_none() {
                        speaker = tracker.last();
                    }
                }
                let (seg_secs, t0) = (
                    segment.len() as f32 / SAMPLE_RATE as f32,
                    std::time::Instant::now(),
                );
                let res = engine.transcribe_samples(segment, SAMPLE_RATE, 1, None);
                log_asr(seg_secs, t0.elapsed().as_millis(), pause_secs);
                match res {
                    Ok(result) if !result.text.trim().is_empty() => {
                        if !text_in_languages(&result.text, &languages) {
                            tracing::debug!("dropped out-of-language text: {}", result.text);
                            return;
                        }
                        let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                            item_id,
                            text: result.text,
                            speaker,
                        });
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = transcript_tx
                            .blocking_send(TranscriptEvent::Error(format!("Parakeet: {e}")));
                    }
                }
            };

            while let Some(input) = input_rx.blocking_recv() {
                match input {
                    SttInput::Audio(samples) => {
                        let f32_samples: Vec<f32> =
                            samples.iter().map(|&s| s as f32 / 32768.0).collect();
                        // Realtime input arrives at 24 kHz; Parakeet wants 16 kHz.
                        let mut resampled =
                            crate::types::resample(&f32_samples, 24000, SAMPLE_RATE);
                        agc.process(&mut resampled);
                        #[cfg(feature = "smart-turn")]
                        let mut check = |buf: &[f32]| match turn_model.as_mut() {
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
                        };
                        #[cfg(not(feature = "smart-turn"))]
                        let mut check = |_: &[f32]| true;
                        for segment in seg.push_with(&resampled, &mut check) {
                            emit(segment, &mut item_counter, seg.closed_after);
                        }
                    }
                    SttInput::UpdatePrompt(_) => {}
                    SttInput::Close => {
                        if let Some(segment) = seg.flush() {
                            emit(segment, &mut item_counter, 0);
                        }
                        break;
                    }
                }
            }
            tracing::debug!("Parakeet STT stream closed ({item_counter} segments)");
        });

        Ok((input_tx, transcript_rx))
    }
}

/// True when the text's letters belong to the allowed languages' scripts.
///
/// `ru`/`uk`/`bg`/`be`/`sr` allow Cyrillic, `en` allows plain ASCII Latin, any other code
/// allows all Latin. More than 5% of letters outside that set rejects the text (a lone
/// foreign name does not). Plain-ASCII Portuguese passes: script is all this can see.
pub fn text_in_languages(text: &str, languages: &[String]) -> bool {
    if languages.is_empty() {
        return true;
    }
    let has = |codes: &[&str]| languages.iter().any(|l| codes.contains(&l.as_str()));
    let cyr = has(&["ru", "uk", "bg", "be", "sr"]);
    let ascii = has(&["en"]);
    let latin = languages
        .iter()
        .any(|l| !["ru", "uk", "bg", "be", "sr", "en"].contains(&l.as_str()));
    let (mut total, mut bad) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        total += 1;
        let ok = match c as u32 {
            0x400..=0x52F => cyr,
            _ if c.is_ascii() => ascii || latin,
            0x80..=0x24F | 0x1E00..=0x1EFF => latin,
            _ => false,
        };
        bad += usize::from(!ok);
    }
    total == 0 || bad as f32 / total as f32 <= 0.05
}

/// Smart-turn probability that ends a turn at the early pause.
const SMART_TURN_THRESHOLD: f32 = 0.5;

fn load_engine(dir: &Path, coreml: bool) -> Result<parakeet_rs::ParakeetTDT, parakeet_rs::Error> {
    let dir = dir.to_str().unwrap_or("");
    #[cfg(feature = "coreml")]
    if coreml {
        let cfg = parakeet_rs::ExecutionConfig::new()
            .with_execution_provider(parakeet_rs::ExecutionProvider::CoreML);
        return parakeet_rs::ParakeetTDT::from_pretrained(dir, Some(cfg));
    }
    let _ = coreml;
    parakeet_rs::ParakeetTDT::from_pretrained(dir, None)
}

/// Offline run of the live pipeline over a recording (benchmark): segments with the same
/// segmenter, transcribes each, returns `(secs, wait_ms, asr_ms, text)` per segment.
/// `wait_ms` is the silence the cut waited for after the last speech frame.
#[doc(hidden)]
pub fn bench_file(
    dir: &Path,
    samples: &[f32],
    rate: u32,
    smart: bool,
    coreml: bool,
) -> Vec<(f32, u32, u128, String)> {
    let mut engine = load_engine(dir, coreml).expect("load model");
    let mut x = if rate == SAMPLE_RATE {
        samples.to_vec()
    } else {
        crate::types::resample(samples, rate, SAMPLE_RATE)
    };
    crate::agc::Agc::default().process(&mut x);
    let pause_frames = (DEFAULT_PAUSE_MS / FRAME_MS) as usize;
    #[cfg(feature = "smart-turn")]
    let mut model =
        crate::smart_turn::SmartTurn::new(&crate::smart_turn::default_model_path()).ok();
    #[cfg(feature = "smart-turn")]
    let early = if smart && model.is_some() { 15 } else { 0 };
    #[cfg(not(feature = "smart-turn"))]
    let early = {
        let _ = smart;
        0
    };
    let mut seg = Segmenter::new(pause_frames, 15 * SAMPLE_RATE as usize).with_early_check(early);
    let mut closed = Vec::new();
    for block in x.chunks(FRAME_SAMPLES * 5) {
        #[cfg(feature = "smart-turn")]
        let mut check = |b: &[f32]| {
            model
                .as_mut()
                .is_none_or(|m| m.probability(b).unwrap_or(1.0) >= SMART_TURN_THRESHOLD)
        };
        #[cfg(not(feature = "smart-turn"))]
        let mut check = |_: &[f32]| true;
        for s in seg.push_with(block, &mut check) {
            closed.push((s, seg.closed_after));
        }
    }
    closed.extend(seg.flush().map(|s| (s, 0)));
    closed
        .into_iter()
        .map(|(s, silence)| {
            let secs = s.len() as f32 / SAMPLE_RATE as f32;
            let t0 = std::time::Instant::now();
            let text = engine
                .transcribe_samples(s, SAMPLE_RATE, 1, None)
                .map(|r| r.text)
                .unwrap_or_default();
            (
                secs,
                silence as u32 * FRAME_MS,
                t0.elapsed().as_millis(),
                text,
            )
        })
        .collect()
}

/// One line per transcribed segment: `~/.supervox/live/asr.log` (`asr_ms`, segment length,
/// silence waited before the cut).
fn log_asr(seg_secs: f32, asr_ms: u128, silence_secs: f64) {
    use std::io::Write;
    let path = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".supervox/live/asr.log"))
        .unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "asr_ms={asr_ms} secs={seg_secs:.1} wait_ms={:.0}",
            silence_secs * 1000.0
        );
    }
}

/// Parakeet input sample rate.
const SAMPLE_RATE: u32 = 16000;
/// Energy-VAD frame length.
const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (SAMPLE_RATE / 1000 * FRAME_MS) as usize;
/// Default end-of-utterance pause.
pub const DEFAULT_PAUSE_MS: u32 = 1200;
/// Default forced-cut length.
pub const DEFAULT_MAX_SEGMENT_SECS: f32 = 15.0;
/// Pre-roll kept before the first speech frame (so onsets are not clipped).
const PRE_ROLL_FRAMES: usize = 15; // 300 ms
/// Silence kept after the last speech frame inside an emitted segment.
const TAIL_FRAMES: usize = 15; // 300 ms
/// Cosine threshold (on mean-centered embeddings) for joining an existing speaker cluster.
#[cfg(feature = "speaker")]
const SPEAKER_THRESHOLD: f32 = 0.2;

/// Energy-based utterance segmenter: cuts on a pause, not on a timer.
///
/// Keeps a short pre-roll of silence before speech, drops long stretches of
/// pure silence (Parakeet hallucinates on them), and force-cuts a segment that
/// exceeds `max_samples`.
struct Segmenter {
    pause_frames: usize,
    /// Short pause (frames) at which the end-of-turn check runs; 0 = never (fixed pause only).
    early_frames: usize,
    /// Trailing silence (frames) of the segment closed last.
    closed_after: usize,
    max_samples: usize,
    buf: Vec<f32>,
    partial: Vec<f32>,
    has_speech: bool,
    trailing_silence: usize,
}

impl Segmenter {
    fn new(pause_frames: usize, max_samples: usize) -> Self {
        Self {
            pause_frames,
            early_frames: 0,
            closed_after: 0,
            max_samples,
            buf: Vec::new(),
            partial: Vec::new(),
            has_speech: false,
            trailing_silence: 0,
        }
    }

    /// Close early: after `early_frames` of silence ask `turn` (given the speech so far)
    /// whether the speaker is done; if not, wait for the full `pause_frames`.
    fn with_early_check(mut self, early_frames: usize) -> Self {
        self.early_frames = early_frames.min(self.pause_frames);
        self
    }

    /// Fixed-pause push (no end-of-turn check).
    #[cfg(test)]
    fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.push_with(samples, &mut |_| true)
    }

    /// Feed 16 kHz samples; returns zero or more finished utterances.
    fn push_with(
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
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
            let is_speech = rms >= SILENCE_THRESHOLD;
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
                    self.buf.drain(..self.buf.len() - keep);
                }
                continue;
            }
            let early = self.early_frames > 0
                && self.early_frames < self.pause_frames
                && self.trailing_silence == self.early_frames
                && turn(&self.buf);
            if early || self.trailing_silence >= self.pause_frames {
                self.closed_after = self.trailing_silence;
                // Utterance ended: emit speech + a short tail, keep the rest as pre-roll.
                let drop_tail = (self.trailing_silence - TAIL_FRAMES.min(self.trailing_silence))
                    * FRAME_SAMPLES;
                let cut = self.buf.len().saturating_sub(drop_tail);
                let segment: Vec<f32> = self.buf.drain(..cut).collect();
                let keep = PRE_ROLL_FRAMES * FRAME_SAMPLES;
                if self.buf.len() > keep {
                    self.buf.drain(..self.buf.len() - keep);
                }
                self.has_speech = false;
                self.trailing_silence = 0;
                out.push(segment);
            } else if self.buf.len() >= self.max_samples {
                self.closed_after = 0;
                out.push(std::mem::take(&mut self.buf));
                self.has_speech = false;
                self.trailing_silence = 0;
            }
        }
        self.partial.drain(..offset);
        out
    }

    /// Return whatever speech is still buffered (on stop).
    fn flush(&mut self) -> Option<Vec<f32>> {
        if !self.has_speech || self.buf.len() < FRAME_SAMPLES * 10 {
            self.buf.clear();
            return None;
        }
        self.has_speech = false;
        self.trailing_silence = 0;
        Some(std::mem::take(&mut self.buf))
    }
}

/// RMS threshold below which audio is considered silence.
/// Parakeet hallucinates on silence — must skip quiet chunks.
const SILENCE_THRESHOLD: f32 = 0.005;

/// Check that parakeet model files exist in the given directory.
pub fn model_exists(model_dir: &Path) -> bool {
    model_dir.join("encoder-model.onnx").exists()
        && model_dir.join("decoder_joint-model.onnx").exists()
        && model_dir.join("vocab.txt").exists()
}

/// Default model directory: ~/.supervox/models/parakeet-tdt/
pub fn default_model_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".supervox/models/parakeet-tdt"))
        .unwrap_or_else(|| PathBuf::from("models/parakeet-tdt"))
}

#[cfg(test)]
mod segmenter_tests {
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
    fn language_filter_drops_foreign_gibberish() {
        let ru_en = vec!["ru".to_string(), "en".to_string()];
        assert!(text_in_languages("Слушай, что у нас по продажам?", &ru_en));
        assert!(text_in_languages("Open the pull request, Рустам", &ru_en));
        assert!(!text_in_languages("Você não brau. Um bravo", &ru_en));
        assert!(text_in_languages("Você não", &["pt".to_string()]));
        assert!(!text_in_languages("Привет", &["en".to_string()]));
        assert!(text_in_languages("anything é", &[]));
        assert!(text_in_languages("123 ...", &ru_en));
    }

    #[test]
    fn flush_returns_trailing_speech() {
        let mut seg = Segmenter::new(60, 15 * SAMPLE_RATE as usize);
        assert!(seg.push(&tone(1.0)).is_empty());
        assert!(seg.flush().is_some());
    }
}
