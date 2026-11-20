//! Local Parakeet TDT STT backend using parakeet-rs (ONNX Runtime).
//!
//! NVIDIA Parakeet TDT 0.6B — fast, accurate, offline speech-to-text.
//! ~5x faster than Whisper base on Apple Silicon, better accuracy.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use parakeet_rs::Transcriber;

use crate::segmenter::*;
pub use crate::segmenter::{DEFAULT_MAX_SEGMENT_SECS, DEFAULT_PAUSE_MS};
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
                        let mut check = |buf: &[f32]| turn_finished(turn_model.as_mut(), buf);
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

/// Cosine threshold (on mean-centered embeddings) for joining an existing speaker cluster.
#[cfg(feature = "speaker")]
const SPEAKER_THRESHOLD: f32 = 0.2;

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
mod tests {
    use super::*;

    #[test]
    fn unix_span_is_stream_start_plus_audio_span() {
        let (a, b) = unix_span(1_000.0, (2.0, 5.5));
        assert_eq!((a, b), (1_002.0, 1_005.5));
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
}
