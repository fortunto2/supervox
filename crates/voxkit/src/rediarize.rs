//! Post-stop re-diarization: turn "who spoke when" from an offline diarizer into
//! speaker labels for the transcript lines of a session, and name the speakers
//! that match a voiceprint.
//!
//! The diarizer itself is a sidecar process (FluidAudio, Apple Neural Engine); this
//! module is the pure part: parse its output, decide which turn a line belongs to,
//! find which turn is a known voice. The embedding model is the same CAM++ as live.

use std::collections::HashMap;

use ndarray::Array1;
use serde::Deserialize;

use crate::speaker::{
    SpeakerEmbedder, SpeakerError, VoicePrint, embed_voiceprint, normalize, raw_similarity,
};

/// Raw-cosine match of a speaker's pooled audio (up to 30 s) to a voiceprint.
/// Lower than the live per-segment threshold (0.65): a mean over many turns is steadier
/// than one 4-7 s window. Measured 19.11: his turns 0.67-0.76, others <= 0.48.
pub const NAME_THRESHOLD: f32 = 0.55;

/// Longest pooled audio per speaker used for naming, seconds.
const POOL_SECS: f32 = 30.0;
/// Turns shorter than this add little voice and much overlap noise.
const MIN_TURN_SECS: f32 = 1.0;

/// One stretch of one speaker, seconds in the recording.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Turn {
    #[serde(alias = "speakerId")]
    pub speaker: String,
    #[serde(alias = "startTimeSeconds")]
    pub start: f32,
    #[serde(alias = "endTimeSeconds")]
    pub end: f32,
}

/// What the sidecar writes: FluidAudio's `process --mode offline --output` JSON
/// (`{"segments":[{"speakerId":"S1","startTimeSeconds":0.5,"endTimeSeconds":3.2,...}]}`,
/// extra fields such as embeddings ignored); the short names are accepted too.
#[derive(Debug, Deserialize)]
struct SidecarOut {
    segments: Vec<Turn>,
}

/// Parse the sidecar's JSON. Empty or inverted spans are dropped.
pub fn parse_turns(json: &str) -> Result<Vec<Turn>, serde_json::Error> {
    let out: SidecarOut = serde_json::from_str(json)?;
    Ok(out
        .segments
        .into_iter()
        .filter(|t| t.end > t.start)
        .collect())
}

/// The speaker whose turns cover most of `[start, end]`, if any turn touches it.
pub fn majority_speaker(turns: &[Turn], start: f32, end: f32) -> Option<&str> {
    let mut by: Vec<(&str, f32)> = Vec::new();
    for t in turns {
        let shared = t.end.min(end) - t.start.max(start);
        if shared <= 0.0 {
            continue;
        }
        match by.iter_mut().find(|(s, _)| *s == t.speaker) {
            Some((_, d)) => *d += shared,
            None => by.push((&t.speaker, shared)),
        }
    }
    by.into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}

/// Speakers in order of first appearance.
pub fn speaker_order(turns: &[Turn]) -> Vec<String> {
    let mut sorted: Vec<&Turn> = turns.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out: Vec<String> = Vec::new();
    for t in sorted {
        if !out.contains(&t.speaker) {
            out.push(t.speaker.clone());
        }
    }
    out
}

/// A speaker's longest turns, concatenated up to [`POOL_SECS`], from 16 kHz `samples`.
pub fn pool_audio(turns: &[Turn], speaker: &str, samples: &[f32]) -> Vec<f32> {
    let mut mine: Vec<&Turn> = turns
        .iter()
        .filter(|t| t.speaker == speaker && t.end - t.start >= MIN_TURN_SECS)
        .collect();
    mine.sort_by(|a, b| (b.end - b.start).total_cmp(&(a.end - a.start)));
    let mut out = Vec::new();
    for t in mine {
        if out.len() as f32 / 16_000.0 >= POOL_SECS {
            break;
        }
        let lo = ((t.start * 16_000.0) as usize).min(samples.len());
        let hi = ((t.end * 16_000.0) as usize).min(samples.len());
        out.extend_from_slice(&samples[lo..hi]);
    }
    out
}

/// Which diarizer speaker is which voiceprint: best raw cosine first, one name per
/// speaker and one speaker per name. Voiceprints are unit length (see [`VoicePrint`]).
/// Returns `speaker -> name`.
pub fn match_voiceprints(
    pooled: &[(String, Array1<f32>)],
    voiceprints: &[VoicePrint],
    threshold: f32,
) -> HashMap<String, String> {
    let mut pairs = Vec::new();
    for (s, e) in pooled {
        let e = normalize(e.clone());
        for (v, vp) in voiceprints.iter().enumerate() {
            let sim = raw_similarity(&e, &vp.emb);
            tracing::debug!("rediarize: {s} vs {} raw sim {sim:.3}", vp.name);
            if sim >= threshold {
                pairs.push((sim, s.clone(), v));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut named: HashMap<String, String> = HashMap::new();
    let mut used = vec![false; voiceprints.len()];
    for (_, s, v) in pairs {
        if !named.contains_key(&s) && !used[v] {
            used[v] = true;
            named.insert(s, voiceprints[v].name.clone());
        }
    }
    named
}

/// Embed each speaker's pooled audio and match it to the voiceprints.
pub fn name_speakers(
    turns: &[Turn],
    samples: &[f32],
    embedder: &mut SpeakerEmbedder,
    voiceprints: &[VoicePrint],
    threshold: f32,
) -> Result<HashMap<String, String>, SpeakerError> {
    let mut pooled = Vec::new();
    for s in speaker_order(turns) {
        let audio = pool_audio(turns, &s, samples);
        if (audio.len() as f32) < crate::speaker::MIN_EMBED_SECS * 16_000.0 {
            continue;
        }
        pooled.push((s, embed_voiceprint(embedder, &audio)?));
    }
    Ok(match_voiceprints(&pooled, voiceprints, threshold))
}

/// Display labels for the diarizer's speakers: a voiceprint name, else `S<n>` by first appearance.
pub fn display_labels(turns: &[Turn], names: &HashMap<String, String>) -> HashMap<String, String> {
    let mut n = 0;
    speaker_order(turns)
        .into_iter()
        .map(|s| match names.get(&s) {
            Some(name) => (s, name.clone()),
            None => {
                n += 1;
                (s, format!("S{n}"))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str, a: f32, b: f32) -> Turn {
        Turn {
            speaker: s.into(),
            start: a,
            end: b,
        }
    }

    #[test]
    fn parses_sidecar_json_and_drops_empty_spans() {
        let j = r#"{"segments":[{"speaker":"A","start":0.0,"end":2.5},{"speaker":"B","start":3.0,"end":3.0}]}"#;
        assert_eq!(parse_turns(j).unwrap(), vec![t("A", 0.0, 2.5)]);
        let fluid = r#"{"durationSeconds":9,"segments":[{"speakerId":"S2","startTimeSeconds":1.5,"endTimeSeconds":4.0,"qualityScore":1,"embedding":[0.1,0.2]}]}"#;
        assert_eq!(parse_turns(fluid).unwrap(), vec![t("S2", 1.5, 4.0)]);
        assert!(parse_turns("nope").is_err());
    }

    #[test]
    fn line_goes_to_the_speaker_covering_most_of_it() {
        let turns = [t("A", 0.0, 3.0), t("B", 3.0, 10.0)];
        assert_eq!(majority_speaker(&turns, 2.0, 8.0), Some("B"));
        assert_eq!(majority_speaker(&turns, 0.0, 2.0), Some("A"));
        assert_eq!(majority_speaker(&turns, 20.0, 25.0), None);
    }

    #[test]
    fn labels_follow_first_appearance_and_names_win() {
        let turns = [t("x9", 5.0, 6.0), t("x2", 0.0, 1.0), t("x5", 2.0, 3.0)];
        let names = HashMap::from([("x5".to_string(), "Рустам".to_string())]);
        let l = display_labels(&turns, &names);
        assert_eq!(l["x2"], "S1");
        assert_eq!(l["x5"], "Рустам");
        assert_eq!(l["x9"], "S2");
    }

    #[test]
    fn pool_takes_the_longest_turns_of_one_speaker_up_to_30_s() {
        let samples = vec![0.1f32; 16_000 * 100];
        let turns = [
            t("A", 0.0, 20.0),
            t("A", 30.0, 45.0),
            t("A", 50.0, 50.5),
            t("B", 60.0, 90.0),
        ];
        let p = pool_audio(&turns, "A", &samples);
        assert_eq!(
            p.len(),
            16_000 * 35,
            "20 s + 15 s, stops once 30 s is reached"
        );
        assert!(pool_audio(&turns, "C", &samples).is_empty());
    }

    #[test]
    fn a_voiceprint_names_one_speaker_and_a_speaker_gets_one_name() {
        let vp = |n: &str, v: [f32; 3]| VoicePrint {
            name: n.into(),
            emb: Array1::from_vec(v.to_vec()),
        };
        let pooled = vec![
            ("A".to_string(), Array1::from_vec(vec![1.0, 0.1, 0.0])),
            ("B".to_string(), Array1::from_vec(vec![0.9, 0.2, 0.0])),
            ("C".to_string(), Array1::from_vec(vec![0.0, 0.0, 1.0])),
        ];
        let prints = [vp("Ann", [1.0, 0.0, 0.0])];
        let m = match_voiceprints(&pooled, &prints, 0.9);
        assert_eq!(
            m.len(),
            1,
            "the closer of two similar speakers takes the name"
        );
        assert_eq!(m["A"], "Ann");
    }
}
