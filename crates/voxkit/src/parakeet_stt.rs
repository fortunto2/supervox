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
    /// Speaker diarization: (embedding model path, max speakers).
    #[cfg(feature = "speaker")]
    diarize: Option<(PathBuf, usize)>,
    /// Voiceprint directory and match threshold; named speakers instead of S<n>.
    #[cfg(feature = "speaker")]
    voiceprints: Option<(PathBuf, f32)>,
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
            #[cfg(feature = "speaker")]
            diarize: None,
            #[cfg(feature = "speaker")]
            voiceprints: None,
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

    /// Label each segment with a speaker index (1..=max_speakers).
    #[cfg(feature = "speaker")]
    pub fn with_diarization(mut self, model_path: PathBuf, max_speakers: usize) -> Self {
        self.diarize = Some((model_path, max_speakers));
        self
    }

    /// Name segments whose raw embedding matches a `<Name>.wav` voiceprint in `dir` (cosine >= `threshold`).
    #[cfg(feature = "speaker")]
    pub fn with_voiceprints(mut self, dir: PathBuf, threshold: f32) -> Self {
        self.voiceprints = Some((dir, threshold));
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

        // The four loads are independent and each takes 100-500 ms: run them side by side on
        // blocking threads, never on the async worker that serves the UI.
        let loads = ModelLoads {
            model_dir: model_dir.clone(),
            #[cfg(feature = "smart-turn")]
            smart_turn: self.smart_turn.as_ref().map(|(p, _)| p.clone()),
            #[cfg(feature = "speaker")]
            diarize: self.diarize.clone(),
            #[cfg(feature = "speaker")]
            voiceprints: self.voiceprints.clone(),
        };
        let loaded = tokio::task::spawn_blocking(move || loads.load())
            .await
            .map_err(|e| SttStreamError::Connection(format!("model loading panicked: {e}")))??;
        let mut engine = loaded.engine;

        let languages = self.languages.clone();
        let early_frames = self.early_frames();
        #[cfg(feature = "smart-turn")]
        let mut turn_model = loaded.turn_model;
        #[cfg(feature = "smart-turn")]
        let early_frames = if turn_model.is_some() {
            early_frames
        } else {
            0
        };
        #[cfg(not(feature = "smart-turn"))]
        let early_frames = 0usize;

        #[cfg(feature = "speaker")]
        let mut diarizer = loaded.diarizer;

        let (input_tx, mut input_rx) = mpsc::channel::<SttInput>(64);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TranscriptEvent>(64);

        tokio::task::spawn_blocking(move || {
            let mut seg =
                Segmenter::new(pause_frames, max_segment_samples).with_early_check(early_frames);
            let mut agc = crate::agc::Agc::default();
            let mut item_counter = 0u64;
            // One chunk arrives every ~100 ms: convert and resample into these, never allocate.
            let mut f32_samples: Vec<f32> = Vec::new();
            let mut resampled: Vec<f32> = Vec::new();

            let mut agent =
                crate::agent_voice::AgentVoice::new(&crate::agent_voice::default_path());
            let mut asr_log = AsrLog::open();
            // Unix time of the first sample fed in; every span is on this clock.
            let mut t0: Option<f64> = None;
            let mut emit = |segment: Vec<f32>,
                            span: (f32, f32),
                            t0: f64,
                            item_counter: &mut u64,
                            silence_frames: usize| {
                let pause_secs = silence_frames as f64 * FRAME_MS as f64 / 1000.0;
                *item_counter += 1;
                let item_id = format!("parakeet_{item_counter}");
                // Loudspeaker audio of the agent's own voice: exact text from its
                // speaking log, never embedded (it collapses far-field clustering).
                let (seg_start, seg_end) = unix_span(t0, span);
                match agent.classify(seg_start, seg_end) {
                    crate::agent_voice::Verdict::Agent(text) => {
                        let _ = transcript_tx.blocking_send(TranscriptEvent::Final {
                            item_id,
                            text,
                            speaker: Some(crate::stt::SpeakerId::Agent),
                            span: Some(span),
                        });
                        return;
                    }
                    crate::agent_voice::Verdict::Duplicate => return,
                    crate::agent_voice::Verdict::Human => {}
                }
                #[allow(unused_mut)]
                let mut speaker: Option<crate::stt::SpeakerId> = None;
                let seg_secs = segment.len() as f32 / SAMPLE_RATE as f32;
                let t0 = std::time::Instant::now();
                // The speaker embedding and the recognition are independent: run them together.
                #[cfg(feature = "speaker")]
                let (res, asr_ms) = match diarizer.as_mut() {
                    Some((embedder, tracker)) => {
                        let (res, asr_ms, emb) = std::thread::scope(|sc| {
                            let emb = (seg_secs >= crate::speaker::MIN_EMBED_SECS)
                                .then(|| sc.spawn(|| embedder.embed(&segment)));
                            let res =
                                engine.transcribe_samples(segment.clone(), SAMPLE_RATE, 1, None);
                            (res, t0.elapsed().as_millis(), emb.map(|h| h.join()))
                        });
                        match emb {
                            Some(Ok(Ok(e))) => speaker = Some(tracker.assign_display(&e)),
                            Some(Ok(Err(e))) => tracing::warn!("speaker embedding failed: {e}"),
                            Some(Err(_)) => tracing::warn!("speaker embedding panicked"),
                            None => {}
                        }
                        if speaker.is_none() {
                            speaker = tracker.last();
                        }
                        (res, asr_ms)
                    }
                    None => (
                        engine.transcribe_samples(segment, SAMPLE_RATE, 1, None),
                        t0.elapsed().as_millis(),
                    ),
                };
                #[cfg(not(feature = "speaker"))]
                let (res, asr_ms) = (
                    engine.transcribe_samples(segment, SAMPLE_RATE, 1, None),
                    t0.elapsed().as_millis(),
                );
                asr_log.write(seg_secs, asr_ms, pause_secs);
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
                            span: Some(span),
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
                        if t0.is_none() {
                            // Capture hands over chunks as they are recorded: this one just ended.
                            t0 = Some(unix_now() - samples.len() as f64 / 24_000.0);
                        }
                        f32_samples.clear();
                        f32_samples.extend(samples.iter().map(|&s| s as f32 / 32768.0));
                        // Realtime input arrives at 24 kHz; Parakeet wants 16 kHz.
                        crate::types::resample_into(
                            &f32_samples,
                            24000,
                            SAMPLE_RATE,
                            &mut resampled,
                        );
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
                        let closed = seg.push_with(&resampled, &mut check);
                        let spans = seg.take_spans();
                        for (segment, span) in closed.into_iter().zip(spans) {
                            emit(
                                segment,
                                span,
                                t0.unwrap_or_default(),
                                &mut item_counter,
                                seg.closed_after,
                            );
                        }
                    }
                    SttInput::UpdatePrompt(_) => {}
                    SttInput::Close => {
                        if let Some(segment) = seg.flush() {
                            let span = seg.take_spans().pop().unwrap_or_default();
                            emit(segment, span, t0.unwrap_or_default(), &mut item_counter, 0);
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

/// What `connect()` loads, so the loads can run in parallel.
struct ModelLoads {
    model_dir: PathBuf,
    #[cfg(feature = "smart-turn")]
    smart_turn: Option<PathBuf>,
    #[cfg(feature = "speaker")]
    diarize: Option<(PathBuf, usize)>,
    #[cfg(feature = "speaker")]
    voiceprints: Option<(PathBuf, f32)>,
}

struct Loaded {
    engine: parakeet_rs::ParakeetTDT,
    #[cfg(feature = "smart-turn")]
    turn_model: Option<crate::smart_turn::SmartTurn>,
    #[cfg(feature = "speaker")]
    diarizer: Option<(
        crate::speaker::SpeakerEmbedder,
        crate::speaker::SpeakerTracker,
    )>,
}

impl ModelLoads {
    /// Parakeet, smart-turn and the speaker embedder (then its voiceprints) on their own threads.
    fn load(self) -> Result<Loaded, SttStreamError> {
        std::thread::scope(|sc| {
            let engine = sc.spawn(|| {
                load_engine(&self.model_dir)
                    .map_err(|e| SttStreamError::Connection(format!("Load Parakeet model: {e}")))
            });
            #[cfg(feature = "smart-turn")]
            let turn = sc.spawn(|| {
                self.smart_turn.as_ref().and_then(|path| {
                    match crate::smart_turn::SmartTurn::new(path) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            tracing::warn!("smart-turn off, fixed pause only: {e}");
                            None
                        }
                    }
                })
            });
            #[cfg(feature = "speaker")]
            let diarizer = sc.spawn(|| {
                let Some((path, max)) = &self.diarize else {
                    return Ok(None);
                };
                let mut embedder = crate::speaker::SpeakerEmbedder::new(path)
                    .map_err(|e| SttStreamError::Connection(format!("Load speaker model: {e}")))?;
                let mut tracker =
                    crate::speaker::SpeakerTracker::new(*max, SPEAKER_THRESHOLD).far_field();
                if let Some((dir, threshold)) = &self.voiceprints {
                    let vps = crate::speaker::load_voiceprints(dir, &mut embedder);
                    tracing::info!("{} voiceprint(s) loaded from {}", vps.len(), dir.display());
                    tracker = tracker.with_voiceprints(vps, *threshold);
                }
                Ok(Some((embedder, tracker)))
            });
            let panicked = |what: &str| SttStreamError::Connection(format!("{what} load panicked"));
            Ok(Loaded {
                engine: engine.join().map_err(|_| panicked("Parakeet"))??,
                #[cfg(feature = "smart-turn")]
                turn_model: turn.join().map_err(|_| panicked("smart-turn"))?,
                #[cfg(feature = "speaker")]
                diarizer: diarizer.join().map_err(|_| panicked("speaker"))??,
            })
        })
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Wall-clock span (unix seconds) of a segment: stream start plus its audio span, so it does
/// not depend on how long the pause wait, the embedding or the recognition took.
fn unix_span(t0: f64, span: (f32, f32)) -> (f64, f64) {
    (t0 + span.0 as f64, t0 + span.1 as f64)
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

/// The end-of-turn model only looks at the last 8 s of the speech so far.
const TURN_WINDOW_SAMPLES: usize = 8 * SAMPLE_RATE as usize;

/// Smart-turn probability that ends a turn at the early pause.
const SMART_TURN_THRESHOLD: f32 = 0.5;

fn load_engine(dir: &Path) -> Result<parakeet_rs::ParakeetTDT, parakeet_rs::Error> {
    parakeet_rs::ParakeetTDT::from_pretrained(dir.to_str().unwrap_or(""), None)
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
) -> Vec<(f32, u32, u128, String)> {
    let mut engine = load_engine(dir).expect("load model");
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

/// `~/.supervox/live/asr.log`: one line per transcribed segment (`asr_ms`, segment length,
/// silence waited before the cut). The file stays open for the whole stream.
struct AsrLog(Option<std::io::BufWriter<std::fs::File>>);

impl AsrLog {
    fn open() -> Self {
        let f = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(crate::paths::live_dir().join("asr.log"))
            .ok();
        Self(f.map(std::io::BufWriter::new))
    }

    fn write(&mut self, seg_secs: f32, asr_ms: u128, silence_secs: f64) {
        use std::io::Write;
        if let Some(w) = self.0.as_mut() {
            let _ = writeln!(
                w,
                "asr_ms={asr_ms} secs={seg_secs:.1} wait_ms={:.0}",
                silence_secs * 1000.0
            );
            let _ = w.flush();
        }
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
pub const DEFAULT_MAX_SEGMENT_SECS: f32 = 7.0; // a noisy room never goes silent: 15 s made text arrive 15 s late
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
    fn new(pause_frames: usize, max_samples: usize) -> Self {
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
    fn flush(&mut self) -> Option<Vec<f32>> {
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
    fn take_spans(&mut self) -> Vec<(f32, f32)> {
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

/// Check that parakeet model files exist in the given directory.
pub fn model_exists(model_dir: &Path) -> bool {
    model_dir.join("encoder-model.onnx").exists()
        && model_dir.join("decoder_joint-model.onnx").exists()
        && model_dir.join("vocab.txt").exists()
}

/// Default model directory: ~/.supervox/models/parakeet-tdt/
pub fn default_model_dir() -> PathBuf {
    crate::paths::models_dir().join("parakeet-tdt")
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
    fn unix_span_is_stream_start_plus_audio_span() {
        let (a, b) = unix_span(1_000.0, (2.0, 5.5));
        assert_eq!((a, b), (1_002.0, 1_005.5));
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
