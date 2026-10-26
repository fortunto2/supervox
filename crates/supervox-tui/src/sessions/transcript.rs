//! One recording's text: the transcript lines of a JSONL stream (or of a saved call, where
//! the transcript is one string) and its summary.
//!
//! Streams can be big: every `summary` event carries the whole summary again. A line is
//! parsed only if it is a transcript event, and a summary event written by supervox is
//! recognised from its last bytes, so most of the file is never UTF-8 checked or parsed.

use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// One spoken line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    /// Seconds since the recording started; `None` for saved calls (no timing kept).
    pub t: Option<u64>,
    pub speaker: String,
    pub text: String,
}

/// Every transcript event has this; a line without it is never parsed.
const TRANSCRIPT_MARK: &str = "\"transcript\"";
/// supervox's events serialize with sorted keys, so `type` is the last field: a summary
/// event, the bulk of a stream, is skipped from its last bytes. Spaced for other writers
/// (Python's `json.dumps`).
const SUMMARY_ENDS: [&[u8]; 2] = [b"\"type\":\"summary\"}", b"\"type\": \"summary\"}"];

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    t: Option<f64>,
    #[serde(default)]
    speaker: String,
    #[serde(default)]
    text: String,
}

fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

/// A transcript event, or `None` for every other line (and for broken ones).
fn parse_line(raw: &[u8]) -> Option<Utterance> {
    let line = trim_end(raw);
    if SUMMARY_ENDS.iter().any(|end| line.ends_with(end)) {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?;
    if !line.contains(TRANSCRIPT_MARK) {
        return None;
    }
    let ev: Event = serde_json::from_str(line).ok()?;
    if ev.kind != "transcript" || ev.text.trim().is_empty() {
        return None;
    }
    Some(Utterance {
        t: ev.t.filter(|t| *t >= 0.0).map(|t| t as u64),
        speaker: ev.speaker,
        text: ev.text,
    })
}

/// Append every transcript line of a stream to `out`.
pub fn read_stream(reader: impl BufRead, out: &mut Vec<Utterance>) {
    let mut reader = reader;
    let mut buf = Vec::with_capacity(4096);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => out.extend(parse_line(&buf)),
        }
    }
}

/// The transcript of a session written in several streams (an agent's `stream-part*.jsonl`).
pub fn load_streams(paths: &[PathBuf]) -> Vec<Utterance> {
    let mut out = Vec::new();
    for path in paths {
        if let Ok(f) = File::open(path) {
            read_stream(BufReader::with_capacity(1 << 16, f), &mut out);
        }
    }
    out
}

/// True if a stream has at least one transcript line.
pub fn has_speech(path: &Path) -> bool {
    let Ok(f) = File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(f);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return false,
            Ok(_) if parse_line(&buf).is_some() => return true,
            Ok(_) => {}
        }
    }
}

/// `S1: text` lines of a saved call's transcript string.
pub fn from_call_transcript(transcript: &str) -> Vec<Utterance> {
    transcript
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (speaker, text) = match l.split_once(": ") {
                Some((s, t)) if s.len() <= 16 && !s.contains(' ') => (s.to_string(), t),
                _ => (String::new(), l),
            };
            Utterance {
                t: None,
                speaker,
                text: text.trim().to_string(),
            }
        })
        .collect()
}

#[derive(Deserialize)]
struct CallText {
    #[serde(default)]
    transcript: String,
}

/// A call JSON's transcript (other fields are skipped, not kept).
pub fn load_call(path: &Path) -> Vec<Utterance> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<CallText>(&s).ok())
        .map(|c| from_call_transcript(&c.transcript))
        .unwrap_or_default()
}

/// `7:05`, or `1:02:03` past an hour.
pub fn clock(t: u64) -> String {
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(lines: &[serde_json::Value]) -> String {
        lines.iter().map(|v| format!("{v}\n")).collect()
    }

    #[test]
    fn only_transcript_events_become_lines() {
        let body = stream(&[
            serde_json::json!({"type": "start", "ts": "2026-10-25T18:46:32-07:00"}),
            serde_json::json!({"type": "transcript", "t": 83, "speaker": "S1", "text": "hello"}),
            serde_json::json!({"type": "summary", "text": "{\"type\":\"transcript\"} quoted"}),
            serde_json::json!({"type": "heartbeat"}),
            serde_json::json!({"type": "transcript", "t": 90, "speaker": "S2", "text": "привет"}),
            serde_json::json!({"type": "transcript", "t": 91, "speaker": "S2", "text": "  "}),
        ]);
        let mut out = Vec::new();
        read_stream(body.as_bytes(), &mut out);
        assert_eq!(
            out,
            vec![
                Utterance {
                    t: Some(83),
                    speaker: "S1".into(),
                    text: "hello".into()
                },
                Utterance {
                    t: Some(90),
                    speaker: "S2".into(),
                    text: "привет".into()
                },
            ]
        );
    }

    #[test]
    fn broken_lines_and_other_key_orders_are_handled() {
        let body = "{\"type\":\"transcript\",\"t\":5,\"speaker\":\"You\",\"text\":\"first\"}\n\
                    {not json\n\
                    \u{0}\u{ff}garbage\n\
                    {\"speaker\": \"S2\", \"t\": 9, \"text\": \"spaced\", \"type\": \"transcript\"}\n\
                    {\"text\": \"a transcript word\", \"type\": \"summary\"}\n\
                    {\"text\":\"no newline at end\",\"type\":\"transcript\",\"t\":6.7}";
        let mut out = Vec::new();
        read_stream(body.as_bytes(), &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].text, "spaced");
        assert_eq!(out[0].speaker, "You");
        assert_eq!(out[2].t, Some(6));
        assert_eq!(out[2].speaker, "");
    }

    #[test]
    fn several_streams_are_read_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("stream-part1.jsonl");
        let b = dir.path().join("stream-part2.jsonl");
        std::fs::write(
            &a,
            stream(&[serde_json::json!({"type": "transcript", "t": 1, "text": "a"})]),
        )
        .unwrap();
        std::fs::write(
            &b,
            stream(&[serde_json::json!({"type": "transcript", "t": 2, "text": "b"})]),
        )
        .unwrap();
        let lines = load_streams(&[a.clone(), b]);
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(has_speech(&a));
        let silent = dir.path().join("silent.jsonl");
        std::fs::write(&silent, stream(&[serde_json::json!({"type": "start"})])).unwrap();
        assert!(!has_speech(&silent));
        assert!(!has_speech(&dir.path().join("missing.jsonl")));
    }

    #[test]
    fn a_call_transcript_splits_speakers() {
        let lines = from_call_transcript("S1: hi there\nYou: Например, вот\n\nno speaker here\n");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].speaker, "S1");
        assert_eq!(lines[0].text, "hi there");
        assert_eq!(lines[1].speaker, "You");
        assert_eq!(lines[2].speaker, "");
        assert_eq!(lines[2].text, "no speaker here");
        assert!(lines.iter().all(|l| l.t.is_none()));
        // a colon inside a sentence is not a speaker label
        let l = from_call_transcript("An open source project: when you combine it");
        assert_eq!(l[0].speaker, "");
    }

    #[test]
    fn a_call_file_gives_only_its_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20261025-abc.json");
        std::fs::write(
            &path,
            r#"{"id":"abc","created_at":"2026-10-25T10:00:00Z","duration_secs":3.0,"transcript":"S1: one\nS1: two"}"#,
        )
        .unwrap();
        assert_eq!(load_call(&path).len(), 2);
        assert!(load_call(&dir.path().join("none.json")).is_empty());
    }

    #[test]
    fn clock_formats_minutes_and_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(83), "1:23");
        assert_eq!(clock(3723), "1:02:03");
    }
}
