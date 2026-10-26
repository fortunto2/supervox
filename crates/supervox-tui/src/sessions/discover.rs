//! Every recording on disk, found from names and file metadata alone.
//!
//! Three layouts, newest first:
//! - `live/sessions/<stamp>[-slug]/`: `stream.jsonl` (or an agent's `stream-part*.jsonl`),
//!   `summary.md`, `summaries/<HHMMSS>.md`
//! - the flat layout before that: `live/sessions/<stamp>.jsonl` + `<stamp>.summary.md`
//! - saved calls, `calls/<date>-<id>.json`, whose transcript is one string
//!
//! plus `live/current.jsonl` when an older supervox is writing it in place. Discovery reads
//! a summary's first lines for its title and a stream's last 32 KB for its length, never a
//! whole stream and never a WAV. A call whose `.live.jsonl` is a hard link to a session's
//! stream is that session, so it is listed once.

use chrono::NaiveDateTime;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `live/sessions/<stamp>/`
    Session,
    /// `live/sessions/<stamp>.jsonl` (+ `.summary.md`)
    Flat,
    /// `calls/<stem>.json`
    Call,
    /// `live/current.jsonl` written in place by an older supervox
    Current,
}

/// One recording, ready to list. Text is loaded later, from `streams`/`call`/`summary`.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    /// Folder name, file stem or call stem.
    pub name: String,
    pub kind: Kind,
    /// Local start time.
    pub started: Option<NaiveDateTime>,
    pub duration_secs: Option<u64>,
    /// The summary's first heading, or a call's first line.
    pub title: Option<String>,
    pub streams: Vec<PathBuf>,
    pub summary: Option<PathBuf>,
    pub call: Option<PathBuf>,
    /// Bytes of transcript source (streams or call JSON).
    pub bytes: u64,
    /// Being written right now.
    pub live: bool,
}

impl SessionEntry {
    fn new(name: String, kind: Kind) -> Self {
        Self {
            started: stamp_of(&name),
            name,
            kind,
            duration_secs: None,
            title: None,
            streams: Vec::new(),
            summary: None,
            call: None,
            bytes: 0,
            live: false,
        }
    }

    /// What the fuzzy filter matches: date, name and title.
    pub fn haystack(&self) -> String {
        let date = self
            .started
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        format!(
            "{date} {} {}",
            self.name,
            self.title.as_deref().unwrap_or("")
        )
    }

    /// `10-25 18:22`
    pub fn date_label(&self) -> String {
        self.started
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "--".into())
    }

    /// The title, or the name when there is no summary yet.
    pub fn label(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// A stream written this recently belongs to a recording in progress (heartbeat is 30 s).
const LIVE_WINDOW: Duration = Duration::from_secs(90);
const TAIL_BYTES: u64 = 32 * 1024;
const HEAD_BYTES: usize = 4 * 1024;
/// Below this a stream may be just start/heartbeat/stop: worth one read to check for speech.
const TINY_STREAM: u64 = 2048;

/// Every recording under `live` (`~/.supervox/live`) and `calls`, newest first.
pub fn discover(live: &Path, calls: &Path) -> Vec<SessionEntry> {
    let sessions = live.join(crate::live_log::SESSIONS_DIR);
    let linked = linked_session(live);
    let mut out = folder_sessions(&sessions, linked.as_deref());
    out.extend(flat_sessions(&sessions));
    out.extend(current_recording(live));
    let known: HashSet<(u64, u64)> = out
        .iter()
        .flat_map(|e| e.streams.iter())
        .filter_map(|p| file_id(p))
        .collect();
    out.extend(saved_calls(calls, &known));
    out.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| b.name.cmp(&a.name)));
    out
}

/// `20261025-182225-dell-nvidia` → 2026-10-25 18:22:25.
fn stamp_of(name: &str) -> Option<NaiveDateTime> {
    let stamp = name.get(..15)?;
    NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").ok()
}

/// The folder `live/current.jsonl` points into, when it is a link.
fn linked_session(live: &Path) -> Option<PathBuf> {
    let target = fs::read_link(live.join(crate::live_log::STREAM_FILE)).ok()?;
    Some(live.join(target).parent()?.to_path_buf())
}

fn sorted_dir(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn modified(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn recently_written(p: &Path) -> bool {
    modified(p)
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < LIVE_WINDOW)
}

fn size(p: &Path) -> u64 {
    fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// `stream.jsonl`, else an agent's `stream*.jsonl` parts, else any `*.jsonl`, in name order.
fn folder_streams(files: &[PathBuf]) -> Vec<PathBuf> {
    let jsonl: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl") && p.is_file())
        .collect();
    let named = |pred: &dyn Fn(&str) -> bool| -> Vec<PathBuf> {
        jsonl
            .iter()
            .filter(|p| pred(&file_name(p)))
            .map(|p| p.to_path_buf())
            .collect()
    };
    let main = named(&|n| n == crate::live_log::SESSION_STREAM);
    if !main.is_empty() {
        return main;
    }
    let parts = named(&|n| n.starts_with("stream"));
    if !parts.is_empty() {
        return parts;
    }
    jsonl.into_iter().cloned().collect()
}

/// `summary.md`, else the newest of `summary*.md` and `summaries/*.md`.
fn folder_summary(dir: &Path, files: &[PathBuf]) -> Option<PathBuf> {
    let main = dir.join(crate::live_log::SUMMARY_FILE);
    if size(&main) > 0 {
        return Some(main);
    }
    let parts = files.iter().filter(|p| {
        let n = file_name(p);
        n.starts_with("summary") && n.ends_with(".md")
    });
    let versions = sorted_dir(&dir.join(crate::live_log::SUMMARIES_DIR));
    parts
        .chain(versions.iter())
        .filter(|p| size(p) > 0)
        .max_by_key(|p| modified(p))
        .cloned()
}

fn folder_sessions(sessions: &Path, linked: Option<&Path>) -> Vec<SessionEntry> {
    let mut out = Vec::new();
    for dir in sorted_dir(sessions).into_iter().filter(|p| p.is_dir()) {
        let name = file_name(&dir);
        if name.ends_with("-empty") {
            continue;
        }
        let files = sorted_dir(&dir);
        let mut e = SessionEntry::new(name, Kind::Session);
        e.streams = folder_streams(&files);
        e.summary = folder_summary(&dir, &files);
        e.live = linked.is_some_and(|l| l == dir) && e.streams.iter().any(|s| recently_written(s));
        if fill_stream_facts(&mut e) {
            out.push(e);
        }
    }
    out
}

/// Size, length and title of an entry with streams. False if there is nothing to read:
/// no summary and no speech.
fn fill_stream_facts(e: &mut SessionEntry) -> bool {
    e.bytes = e.streams.iter().map(|p| size(p)).sum();
    e.duration_secs = e.streams.iter().rev().find_map(|p| last_t(p));
    e.title = e.summary.as_deref().and_then(summary_title);
    let silent = e.bytes < TINY_STREAM
        && !e.live
        && !e.streams.iter().any(|p| super::transcript::has_speech(p));
    !(silent && e.summary.is_none())
}

/// `sessions/<stamp>.jsonl` and `sessions/<stamp>.summary.md`, grouped by stamp.
fn flat_sessions(sessions: &Path) -> Vec<SessionEntry> {
    let mut out: Vec<SessionEntry> = Vec::new();
    for path in sorted_dir(sessions).into_iter().filter(|p| p.is_file()) {
        let name = file_name(&path);
        let (stem, is_stream) = if let Some(s) = name.strip_suffix(".summary.md") {
            (s.to_string(), false)
        } else if let Some(s) = name.strip_suffix(".jsonl") {
            (s.to_string(), true)
        } else {
            continue;
        };
        let pos = match out.iter().position(|e| e.name == stem) {
            Some(i) => i,
            None => {
                out.push(SessionEntry::new(stem, Kind::Flat));
                out.len() - 1
            }
        };
        if is_stream {
            out[pos].streams.push(path);
        } else if size(&path) > 0 {
            out[pos].summary = Some(path);
        }
    }
    out.retain_mut(fill_stream_facts);
    out
}

/// `live/current.jsonl` as a regular file: an older supervox recording in place.
fn current_recording(live: &Path) -> Option<SessionEntry> {
    let stream = live.join(crate::live_log::STREAM_FILE);
    let meta = fs::symlink_metadata(&stream).ok()?;
    if !meta.file_type().is_file() || meta.len() == 0 {
        return None;
    }
    let mut e = SessionEntry::new("current.jsonl".into(), Kind::Current);
    e.started = first_ts(&stream).or_else(|| {
        modified(&stream).map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local())
    });
    e.live = recently_written(&stream);
    e.streams = vec![stream];
    let summary = live.join(crate::live_log::SUMMARY_FILE);
    e.summary = (size(&summary) > 0).then_some(summary);
    fill_stream_facts(&mut e).then_some(e)
}

/// `calls/<stem>.json`, minus the ones a session already stands for.
fn saved_calls(calls: &Path, known: &HashSet<(u64, u64)>) -> Vec<SessionEntry> {
    let mut out = Vec::new();
    for path in sorted_dir(calls) {
        let name = file_name(&path);
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if stem.contains('.') {
            continue; // `.analysis.json`
        }
        let live_copy = calls.join(format!("{stem}.live.jsonl"));
        if file_id(&live_copy).is_some_and(|id| known.contains(&id))
            && super::transcript::has_speech(&live_copy)
        {
            continue;
        }
        if let Some(e) = call_entry(calls, stem, &path) {
            out.push(e);
        }
    }
    out
}

fn call_entry(calls: &Path, stem: &str, path: &Path) -> Option<SessionEntry> {
    let text = fs::read_to_string(path).ok()?;
    let call: supervox_agent::types::Call = serde_json::from_str(&text).ok()?;
    if call.transcript.trim().is_empty() {
        return None;
    }
    let mut e = SessionEntry::new(stem.to_string(), Kind::Call);
    e.started = Some(call.created_at.with_timezone(&chrono::Local).naive_local());
    e.duration_secs = Some(call.duration_secs.max(0.0) as u64);
    e.bytes = text.len() as u64;
    e.call = Some(path.to_path_buf());
    e.summary = [
        format!("{stem}.summary.md"),
        format!("{}.summary.md", call.id),
    ]
    .into_iter()
    .map(|n| calls.join(n))
    .find(|p| size(p) > 0);
    e.title = e.summary.as_deref().and_then(summary_title).or_else(|| {
        call.transcript
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(clip)
    });
    Some(e)
}

/// One line, at most 90 chars.
fn clip(s: &str) -> String {
    let s = s.trim();
    match s.char_indices().nth(90) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// The first heading of a summary, else its first line, without Markdown marks.
pub fn summary_title(path: &Path) -> Option<String> {
    let mut head = vec![0u8; HEAD_BYTES];
    let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = String::from_utf8_lossy(&head[..n]);
    let lines: Vec<&str> = head
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|l| l.starts_with('#'))
        .or_else(|| lines.first())?;
    let bare = line.trim_start_matches('#').replace("**", "");
    let bare = clip(&bare);
    (!bare.is_empty()).then_some(bare)
}

/// `t` of the last transcript line within the stream's last 32 KB.
fn last_t(path: &Path) -> Option<u64> {
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    tail.split(|b| *b == b'\n').rev().find_map(|line| {
        let mut out = Vec::new();
        super::transcript::read_stream(line, &mut out);
        out.pop().and_then(|u| u.t)
    })
}

/// The `ts` of a stream's `start` line, as local time.
fn first_ts(path: &Path) -> Option<NaiveDateTime> {
    let mut head = vec![0u8; 1024];
    let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
    let first = String::from_utf8_lossy(&head[..n]);
    let v: serde_json::Value = serde_json::from_str(first.lines().next()?).ok()?;
    let ts = chrono::DateTime::parse_from_rfc3339(v["ts"].as_str()?).ok()?;
    Some(ts.with_timezone(&chrono::Local).naive_local())
}

#[cfg(unix)]
fn file_id(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn file_id(_: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(v: serde_json::Value) -> String {
        format!("{v}\n")
    }

    fn speech(t: u64, text: &str) -> String {
        line(serde_json::json!({"type": "transcript", "t": t, "speaker": "S1", "text": text}))
    }

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn call_json(id: &str, created: &str, transcript: &str) -> String {
        serde_json::json!({
            "id": id, "created_at": created, "duration_secs": 61.5,
            "transcript": transcript, "participants": [], "language": null,
            "translation": null, "tags": []
        })
        .to_string()
    }

    struct Home {
        _tmp: tempfile::TempDir,
        live: PathBuf,
        calls: PathBuf,
    }

    fn home() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("live");
        let calls = tmp.path().join("calls");
        fs::create_dir_all(live.join("sessions")).unwrap();
        fs::create_dir_all(&calls).unwrap();
        Home {
            _tmp: tmp,
            live,
            calls,
        }
    }

    fn names(entries: &[SessionEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn all_three_layouts_are_found_newest_first() {
        let h = home();
        let s = h.live.join("sessions");
        write(
            &s.join("20261025-182225-dell-nvidia/stream.jsonl"),
            &(speech(5, "gb10") + &speech(2516, "agent")),
        );
        write(
            &s.join("20261025-182225-dell-nvidia/summary.md"),
            "\n# Dell × NVIDIA — **Top 8**\n\nbody\n",
        );
        write(&s.join("20261025-163736.jsonl"), &speech(17, "levine"));
        write(&s.join("20261025-163736.summary.md"), "# Sergey Levine\n");
        write(
            &h.calls.join("20261024-abc.json"),
            &call_json("abc", "2026-10-24T04:48:00Z", "S1: the hackathon\nS1: more"),
        );
        let all = discover(&h.live, &h.calls);
        assert_eq!(
            names(&all),
            [
                "20261025-182225-dell-nvidia",
                "20261025-163736",
                "20261024-abc"
            ]
        );
        let dell = &all[0];
        assert_eq!(dell.kind, Kind::Session);
        assert_eq!(dell.title.as_deref(), Some("Dell × NVIDIA — Top 8"));
        assert_eq!(dell.duration_secs, Some(2516));
        assert_eq!(dell.date_label(), "10-25 18:22");
        assert!(dell.haystack().contains("2026-10-25 18:22"));
        assert_eq!(all[1].kind, Kind::Flat);
        assert_eq!(all[1].streams.len(), 1);
        assert!(all[1].summary.is_some());
        let call = &all[2];
        assert_eq!(call.kind, Kind::Call);
        assert_eq!(call.title.as_deref(), Some("S1: the hackathon"));
        assert_eq!(call.duration_secs, Some(61));
    }

    #[test]
    fn an_agents_stream_parts_and_newest_summary_version_are_used() {
        let h = home();
        let dir = h.live.join("sessions/20261025-182225-dell-nvidia");
        write(&dir.join("stream-part1.jsonl"), &speech(1, "a"));
        write(&dir.join("stream-part2.jsonl"), &speech(2, "b"));
        write(&dir.join("summary-part1.md"), "# Old\n");
        write(&dir.join("summaries/184651.md"), "# Newest\n");
        let old = SystemTime::now() - Duration::from_secs(600);
        fs::File::options()
            .write(true)
            .open(dir.join("summary-part1.md"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        write(&dir.join("frames/f-1.jpg"), "jpg");
        let all = discover(&h.live, &h.calls);
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0]
                .streams
                .iter()
                .map(|p| file_name(p))
                .collect::<Vec<_>>(),
            ["stream-part1.jsonl", "stream-part2.jsonl"]
        );
        assert_eq!(all[0].title.as_deref(), Some("Newest"));
    }

    #[test]
    fn empty_and_silent_recordings_are_left_out() {
        let h = home();
        let s = h.live.join("sessions");
        write(
            &s.join("20261025-100000-empty/stream.jsonl"),
            &speech(1, "x"),
        );
        write(
            &s.join("20261025-182155.jsonl"),
            &line(serde_json::json!({"type": "start"})),
        );
        write(&s.join("20261025-110000/stream.jsonl"), "");
        // silent, but an agent wrote a summary: worth keeping
        write(&s.join("20261025-120000/stream.jsonl"), "");
        write(&s.join("20261025-120000/summary.md"), "# Notes\n");
        let all = discover(&h.live, &h.calls);
        assert_eq!(names(&all), ["20261025-120000"]);
    }

    #[test]
    fn a_call_hard_linked_to_a_session_is_listed_once() {
        let h = home();
        let stream = h.live.join("sessions/20261025-184529/stream.jsonl");
        write(&stream, &speech(3, "hello"));
        write(
            &h.calls.join("20261026-xyz.json"),
            &call_json("xyz", "2026-10-26T01:45:00Z", "S1: hello"),
        );
        fs::hard_link(&stream, h.calls.join("20261026-xyz.live.jsonl")).unwrap();
        // a call linked to a silent session (an old bug) still shows: its JSON has the words
        let silent = h.live.join("sessions/20261025-182155.jsonl");
        write(&silent, &line(serde_json::json!({"type": "start"})));
        write(
            &h.calls.join("20261024-abc.json"),
            &call_json("abc", "2026-10-24T04:48:00Z", "S1: the hackathon"),
        );
        fs::hard_link(&silent, h.calls.join("20261024-abc.live.jsonl")).unwrap();
        let all = discover(&h.live, &h.calls);
        assert_eq!(names(&all), ["20261025-184529", "20261024-abc"]);
    }

    #[test]
    fn a_call_finds_its_summary_by_stem_or_by_id() {
        let h = home();
        write(
            &h.calls.join("20261025-01a1.json"),
            &call_json("01a1", "2026-10-25T17:00:00Z", "S1: x"),
        );
        write(&h.calls.join("01a1.summary.md"), "# Cal Hacks judges\n");
        write(
            &h.calls.join("20261025-01a1.analysis.json"),
            "{\"summary\": \"\"}",
        );
        let all = discover(&h.live, &h.calls);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title.as_deref(), Some("Cal Hacks judges"));
    }

    #[test]
    fn an_older_supervox_recording_in_place_is_listed_as_live() {
        let h = home();
        let start = line(serde_json::json!({
            "type": "start", "ts": "2026-10-25T18:46:32.887520-07:00"
        }));
        write(
            &h.live.join("current.jsonl"),
            &(start + &speech(2516, "the agent")),
        );
        write(&h.live.join("summary.md"), "# Dell × NVIDIA\n");
        let all = discover(&h.live, &h.calls);
        assert_eq!(all.len(), 1);
        let cur = &all[0];
        assert_eq!(cur.kind, Kind::Current);
        assert!(cur.live);
        assert!(cur.started.is_some());
        assert_eq!(cur.title.as_deref(), Some("Dell × NVIDIA"));
    }

    #[cfg(unix)]
    #[test]
    fn the_session_current_jsonl_links_to_is_live_while_written() {
        let h = home();
        write(
            &h.live.join("sessions/20261025-190000/stream.jsonl"),
            &speech(1, "now"),
        );
        std::os::unix::fs::symlink(
            "sessions/20261025-190000/stream.jsonl",
            h.live.join("current.jsonl"),
        )
        .unwrap();
        let all = discover(&h.live, &h.calls);
        assert_eq!(all.len(), 1, "the link is not a second recording");
        assert!(all[0].live);
    }

    #[test]
    fn last_t_reads_only_the_tail_and_skips_a_trailing_summary() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.jsonl");
        let big = "x".repeat(10_000);
        let body = speech(10, "a")
            + &speech(4242, "b")
            + &line(serde_json::json!({"type": "summary", "text": big}))
            + &line(serde_json::json!({"type": "heartbeat"}));
        write(&p, &body);
        assert_eq!(last_t(&p), Some(4242));
    }

    #[test]
    fn summary_title_falls_back_to_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.md");
        write(&p, "**Сейчас** зал собирается\n- bullet\n");
        assert_eq!(summary_title(&p).as_deref(), Some("Сейчас зал собирается"));
        write(&p, "\n\n");
        assert_eq!(summary_title(&p), None);
    }
}
