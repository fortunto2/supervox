//! The agent's own voice: spoken pieces logged by the voice channel in
//! `~/.supervox/live/speaking.jsonl` as `{"start","end","text"}` (unix seconds).
//!
//! A transcript segment that overlaps a spoken piece is loudspeaker audio: it
//! is labelled as the agent with the exact spoken text, and kept out of speaker
//! clustering (it dragged every far-field embedding toward one cluster).

use std::path::{Path, PathBuf};

/// Share of a segment that must overlap spoken pieces to count as the agent.
const MIN_OVERLAP: f64 = 0.3;

/// One line of the speaking log.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Spoken {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn default_path() -> PathBuf {
    crate::paths::live_dir().join("speaking.jsonl")
}

/// Pieces that ended this long before a segment cannot overlap it or any later one.
const KEEP_SECS: f64 = 60.0;

/// Tails the log, remembers which pieces were already attributed to a segment.
pub struct AgentVoice {
    path: PathBuf,
    /// Bytes of the log already read (whole lines only).
    offset: u64,
    /// Recent pieces with their "already attributed" flag.
    spoken: Vec<(Spoken, bool)>,
}

pub enum Verdict {
    /// Not the agent: transcribe and diarize as usual.
    Human,
    /// The agent speaking: exact text from the log.
    Agent(String),
    /// Agent audio whose text was already emitted by an earlier segment: drop.
    Duplicate,
}

impl AgentVoice {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            offset: 0,
            spoken: Vec::new(),
        }
    }

    /// Read what was appended since the last call; forget pieces older than `horizon`.
    fn reload(&mut self, horizon: f64) {
        use std::io::{Read, Seek, SeekFrom};
        if let Ok(mut f) = std::fs::File::open(&self.path) {
            let len = f.metadata().map_or(0, |m| m.len());
            if len < self.offset {
                // Truncated or replaced: start over.
                self.offset = 0;
                self.spoken.clear();
            }
            let mut fresh = String::new();
            if len > self.offset
                && f.seek(SeekFrom::Start(self.offset)).is_ok()
                && f.read_to_string(&mut fresh).is_ok()
            {
                for line in fresh.split_inclusive('\n') {
                    let parsed = serde_json::from_str::<Spoken>(line.trim()).ok();
                    // A tail without its newline may be half written: wait for the rest.
                    if !line.ends_with('\n') && parsed.is_none() {
                        break;
                    }
                    self.offset += line.len() as u64;
                    if let Some(mut s) = parsed {
                        s.text = s.text.trim().to_string();
                        if !s.text.is_empty() && s.end > s.start {
                            self.spoken.push((s, false));
                        }
                    }
                }
            }
        }
        self.spoken.retain(|(s, _)| s.end >= horizon);
    }

    /// `seg_start`/`seg_end`: the segment's wall-clock span, unix seconds.
    pub fn classify(&mut self, seg_start: f64, seg_end: f64) -> Verdict {
        self.reload(seg_start - KEEP_SECS);
        let dur = (seg_end - seg_start).max(1e-3);
        let mut overlap = 0.0;
        let mut hits = Vec::new();
        for (i, (s, _)) in self.spoken.iter().enumerate() {
            let o = (s.end.min(seg_end) - s.start.max(seg_start)).max(0.0);
            if o > 0.0 {
                overlap += o;
                hits.push(i);
            }
        }
        if hits.is_empty() || overlap / dur < MIN_OVERLAP {
            return Verdict::Human;
        }
        let fresh: Vec<&str> = hits
            .iter()
            .filter(|&&i| !self.spoken[i].1)
            .map(|&i| self.spoken[i].0.text.as_str())
            .collect();
        if fresh.is_empty() {
            return Verdict::Duplicate;
        }
        let text = fresh.join(" ");
        for i in hits {
            self.spoken[i].1 = true;
        }
        Verdict::Agent(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av(lines: &str) -> AgentVoice {
        let dir = std::env::temp_dir().join(format!("agentvoice-{}", lines.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("speaking.jsonl");
        std::fs::write(&p, lines).unwrap();
        AgentVoice::new(&p)
    }

    #[test]
    fn overlapping_segment_takes_exact_text_once() {
        let mut a = av("{\"start\":100.0,\"end\":104.0,\"text\":\"Привет, Рустам.\"}\nbroken\n");
        assert!(matches!(a.classify(90.0, 95.0), Verdict::Human));
        match a.classify(100.2, 103.0) {
            Verdict::Agent(t) => assert_eq!(t, "Привет, Рустам."),
            _ => panic!("expected agent"),
        }
        // second ASR segment of the same utterance is dropped, not repeated
        assert!(matches!(a.classify(103.0, 104.5), Verdict::Duplicate));
    }

    #[test]
    fn appended_lines_are_picked_up_and_old_ones_dropped() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("agentvoice-incremental");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("speaking.jsonl");
        std::fs::write(&p, "{\"start\":10.0,\"end\":12.0,\"text\":\"Раз.\"}\n").unwrap();
        let mut a = AgentVoice::new(&p);
        assert!(matches!(a.classify(10.0, 12.0), Verdict::Agent(_)));
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        // half a line: not read yet
        write!(f, "{{\"start\":100.0,\"end\":102.0,").unwrap();
        assert!(matches!(a.classify(100.0, 102.0), Verdict::Human));
        writeln!(f, "\"text\":\"Два.\"}}").unwrap();
        match a.classify(100.0, 102.0) {
            Verdict::Agent(t) => assert_eq!(t, "Два."),
            _ => panic!("expected agent"),
        }
        assert_eq!(
            a.spoken.len(),
            1,
            "the piece from 90 s earlier is forgotten"
        );
    }

    #[test]
    fn small_overlap_with_a_human_is_human() {
        let mut a = av("{\"start\":100.0,\"end\":100.5,\"text\":\"Да.\"} \n");
        assert!(matches!(a.classify(98.0, 104.0), Verdict::Human));
    }
}
