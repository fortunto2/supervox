//! The agent's own voice: spoken pieces logged by the voice channel in
//! `~/.supervox/live/speaking.jsonl` as `{"start","end","text"}` (unix seconds).
//!
//! A transcript segment that overlaps a spoken piece is loudspeaker audio: it
//! is labelled as the agent with the exact spoken text, and kept out of speaker
//! clustering (it dragged every far-field embedding toward one cluster).

use std::path::{Path, PathBuf};

/// Speaker id reserved for the agent (diarized humans are 1-based).
pub const AGENT_SPEAKER: usize = 0;
/// Label shown for [`AGENT_SPEAKER`] unless [`set_label`] chose another.
pub const DEFAULT_LABEL: &str = "Алина";

static LABEL: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Name shown for the agent's own speech (config `agent_name`); blank keeps the default.
pub fn set_label(name: &str) {
    if let Ok(mut g) = LABEL.write() {
        *g = name.trim().to_string();
    }
}

/// Label shown for [`AGENT_SPEAKER`].
pub fn label() -> String {
    match LABEL.read() {
        Ok(g) if !g.is_empty() => g.clone(),
        _ => DEFAULT_LABEL.to_string(),
    }
}

/// Share of a segment that must overlap spoken pieces to count as the agent.
const MIN_OVERLAP: f64 = 0.3;

#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn default_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".supervox/live/speaking.jsonl"))
        .unwrap_or_default()
}

pub fn parse(content: &str) -> Vec<Spoken> {
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            Some(Spoken {
                start: v.get("start")?.as_f64()?,
                end: v.get("end")?.as_f64()?,
                text: v.get("text")?.as_str()?.trim().to_string(),
            })
        })
        .filter(|s| !s.text.is_empty() && s.end > s.start)
        .collect()
}

/// Reads the log, remembers which pieces were already attributed to a segment.
pub struct AgentVoice {
    path: PathBuf,
    spoken: Vec<Spoken>,
    used: Vec<bool>,
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
            spoken: Vec::new(),
            used: Vec::new(),
        }
    }

    fn reload(&mut self) {
        let Ok(c) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let all = parse(&c);
        if all.len() > self.spoken.len() {
            self.used.resize(all.len(), false);
            self.spoken = all;
        }
    }

    /// `seg_start`/`seg_end`: the segment's wall-clock span, unix seconds.
    pub fn classify(&mut self, seg_start: f64, seg_end: f64) -> Verdict {
        self.reload();
        let dur = (seg_end - seg_start).max(1e-3);
        let mut overlap = 0.0;
        let mut hits = Vec::new();
        for (i, s) in self.spoken.iter().enumerate() {
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
            .filter(|&&i| !self.used[i])
            .map(|&i| self.spoken[i].text.as_str())
            .collect();
        if fresh.is_empty() {
            return Verdict::Duplicate;
        }
        let text = fresh.join(" ");
        for i in hits {
            self.used[i] = true;
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
    fn small_overlap_with_a_human_is_human() {
        let mut a = av("{\"start\":100.0,\"end\":100.5,\"text\":\"Да.\"} \n");
        assert!(matches!(a.classify(98.0, 104.0), Verdict::Human));
    }
}
