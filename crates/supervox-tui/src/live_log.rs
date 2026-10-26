//! Live JSONL stream for outside agents.
//!
//! Every final transcript line (and every summary shown) is appended to
//! `~/.supervox/live/current.jsonl` while recording. An agent (Claude Code,
//! a script) tails that file and may write `~/.supervox/live/summary.md`,
//! which supervox displays in the summary panel when `summary = "external"`.
//! The next recording archives all of it into one folder, `live/sessions/<start>/`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STREAM_FILE: &str = "current.jsonl";
pub const SUMMARY_FILE: &str = "summary.md";
/// Every version of `summary.md` the watcher saw this session, `<HHMMSS>.md`.
pub const HISTORY_DIR: &str = "summary-history";
const STAMP: &str = "%Y%m%d-%H%M%S";

/// `~/.supervox/live`
pub fn live_dir() -> PathBuf {
    supervox_agent::storage::default_config_path()
        .parent()
        .map(|p| p.join("live"))
        .unwrap_or_else(|| PathBuf::from(".supervox/live"))
}

pub fn summary_path() -> PathBuf {
    live_dir().join(SUMMARY_FILE)
}

/// Appends events to the live stream. Silent on IO errors: the stream is a
/// convenience for agents, never a reason to break recording.
#[derive(Default)]
pub struct LiveLog {
    file: Option<File>,
    path: PathBuf,
    last_beat: Option<std::time::Instant>,
}

/// How often a recording writes a `heartbeat` line, so a reader can tell
/// "quiet room" from "recording stopped".
pub const HEARTBEAT_SECS: u64 = 30;

impl LiveLog {
    /// Truncate the stream and write a `start` event.
    pub fn start(&mut self, stt_backend: &str, mic_device: Option<&str>) {
        let dir = live_dir();
        let _ = fs::create_dir_all(&dir);
        self.path = dir.join(STREAM_FILE);
        // A fresh session: the old one moves to its folder in `sessions/`, never deleted.
        archive_session(&dir);
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
            .ok();
        self.write(serde_json::json!({
            "type": "start",
            "ts": chrono::Local::now().to_rfc3339(),
            "stt": stt_backend,
            "mic": mic_device,
        }));
    }

    pub fn transcript(&mut self, t_secs: u64, speaker: &str, text: &str) {
        self.write(serde_json::json!({
            "type": "transcript",
            "t": t_secs,
            "ts": chrono::Local::now().to_rfc3339(),
            "speaker": speaker,
            "text": text,
        }));
    }

    pub fn summary(&mut self, text: &str) {
        self.write(serde_json::json!({
            "type": "summary",
            "ts": chrono::Local::now().to_rfc3339(),
            "text": text,
        }));
    }

    /// Write a `heartbeat` line if the stream is open and the last one is older than
    /// [`HEARTBEAT_SECS`]. Cheap to call on every UI tick.
    pub fn heartbeat_if_due(&mut self) {
        if self.file.is_none() {
            return;
        }
        let due = self
            .last_beat
            .is_none_or(|t| t.elapsed().as_secs() >= HEARTBEAT_SECS);
        if due {
            self.last_beat = Some(std::time::Instant::now());
            self.write(serde_json::json!({
                "type": "heartbeat",
                "ts": chrono::Local::now().to_rfc3339(),
            }));
        }
    }

    /// `wav` is the saved call's recording; the archive links it into the session folder.
    pub fn stop(&mut self, call_id: Option<&str>, wav: Option<&str>) {
        self.write(serde_json::json!({
            "type": "stop",
            "ts": chrono::Local::now().to_rfc3339(),
            "call_id": call_id,
            "wav": wav,
        }));
        self.file = None;
    }

    /// Link the finished session's stream and summary next to the saved call as
    /// `<call stem>.live.jsonl` / `<call stem>.summary.md`. A hard link costs nothing
    /// and survives the later archive rename; copy only across volumes.
    pub fn keep_with_call(&self, calls_dir: &Path, call_id: &str) -> Vec<PathBuf> {
        let Some(dir) = self.path.parent() else {
            return Vec::new();
        };
        let stem = supervox_agent::storage::call_stem(calls_dir, call_id)
            .unwrap_or_else(|| call_id.to_string());
        let mut kept = Vec::new();
        for (name, suffix) in [(STREAM_FILE, "live.jsonl"), (SUMMARY_FILE, "summary.md")] {
            let (src, dest) = (dir.join(name), calls_dir.join(format!("{stem}.{suffix}")));
            if fs::hard_link(&src, &dest).is_ok() || fs::copy(&src, &dest).is_ok() {
                kept.push(dest);
            }
        }
        kept
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write(&mut self, v: serde_json::Value) {
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{v}");
            let _ = f.flush();
        }
    }
}

/// Move the previous session into one folder, `<dir>/sessions/<start stamp>/`:
/// `stream.jsonl`, `summary.md`, `summaries/` (every version the agent wrote) and
/// `recording.wav` (a link to the call's WAV, which stays in `calls/`).
///
/// `summary.md` goes along only if it was written after the stream started and the
/// stream has speech; otherwise it is about an earlier talk and is kept as
/// `sessions/<its own stamp>.summary.md`. A stream with no speech gets `-empty`.
/// A folder an agent already made for the session (`<stamp>-<slug>/`) is reused.
pub fn archive_session(dir: &Path) -> Option<PathBuf> {
    let stream = dir.join(STREAM_FILE);
    let meta = fs::metadata(&stream).ok()?;
    if meta.len() == 0 {
        return None;
    }
    let facts = read_facts(&stream);
    let start = facts.start.or_else(|| {
        meta.modified()
            .ok()
            .map(chrono::DateTime::<chrono::Local>::from)
    });
    let stamp = start
        .map(|t| t.format(STAMP).to_string())
        .unwrap_or_else(|| "unknown".into());
    let sessions = dir.join("sessions");
    let out = session_folder(&sessions, &stamp, facts.spoke);
    fs::create_dir_all(&out).ok()?;
    fs::rename(&stream, out.join("stream.jsonl")).ok()?;

    let summary = dir.join(SUMMARY_FILE);
    if let Some(written) = modified_local(&summary) {
        let fresh = facts.spoke && start.is_some_and(|s| written >= s);
        let dest = if fresh {
            out.join(SUMMARY_FILE)
        } else {
            sessions.join(format!("{}.summary.md", written.format(STAMP)))
        };
        let _ = fs::rename(&summary, dest);
    }
    move_all(&dir.join(HISTORY_DIR), &out.join("summaries"));
    if let Some(wav) = facts.wav.as_deref().map(Path::new).filter(|w| w.exists()) {
        link_wav(wav, &out.join("recording.wav"));
    }
    Some(out)
}

/// What the archive needs from a stream, read from its own lines.
#[derive(Default)]
struct StreamFacts {
    start: Option<chrono::DateTime<chrono::Local>>,
    spoke: bool,
    wav: Option<String>,
}

fn read_facts(stream: &Path) -> StreamFacts {
    let mut facts = StreamFacts::default();
    let body = fs::read_to_string(stream).unwrap_or_default();
    for (i, line) in body.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("start") if i == 0 => {
                facts.start = v["ts"]
                    .as_str()
                    .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                    .map(|t| t.with_timezone(&chrono::Local));
            }
            Some("transcript") => facts.spoke = true,
            Some("stop") => facts.wav = v["wav"].as_str().map(String::from),
            _ => {}
        }
    }
    facts
}

/// `sessions/<stamp>/`, unless an agent already made `<stamp>-<slug>/` for frames and
/// notes mid-session: then that one, so one recording ends up in one place.
fn session_folder(sessions: &Path, stamp: &str, spoke: bool) -> PathBuf {
    let slugged = format!("{stamp}-");
    fs::read_dir(sessions)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            p.is_dir() && (name == stamp || name.starts_with(&slugged))
        })
        .unwrap_or_else(|| match spoke {
            true => sessions.join(stamp),
            false => sessions.join(format!("{stamp}-empty")),
        })
}

fn modified_local(path: &Path) -> Option<chrono::DateTime<chrono::Local>> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(Into::into)
}

/// Move every entry of `from` into `to` (merging with what is there), then drop `from`.
fn move_all(from: &Path, to: &Path) {
    let entries: Vec<_> = fs::read_dir(from).into_iter().flatten().flatten().collect();
    if entries.is_empty() {
        let _ = fs::remove_dir(from);
        return;
    }
    let _ = fs::create_dir_all(to);
    for e in entries {
        let _ = fs::rename(e.path(), to.join(e.file_name()));
    }
    let _ = fs::remove_dir(from);
}

#[cfg(unix)]
fn link_wav(wav: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(wav, dest);
}

#[cfg(not(unix))]
fn link_wav(wav: &Path, dest: &Path) {
    let _ = fs::write(dest.with_extension("txt"), wav.to_string_lossy().as_bytes());
}

/// Keep a copy of a summary the watcher just showed as `summary-history/<HHMMSS>.md`
/// (its write time), so the next overwrite does not lose it. Archived as `summaries/`.
pub fn snapshot_summary(dir: &Path, text: &str, written: std::time::SystemTime) -> Option<PathBuf> {
    let out = dir.join(HISTORY_DIR);
    fs::create_dir_all(&out).ok()?;
    let name = chrono::DateTime::<chrono::Local>::from(written).format("%H%M%S");
    let dest = out.join(format!("{name}.md"));
    fs::write(&dest, text).ok()?;
    Some(dest)
}

/// Poll `summary.md`; emit its content whenever it changes. Stops when `stop` is set.
pub fn watch_external_summary(
    event_tx: tokio::sync::mpsc::UnboundedSender<crate::audio::AudioEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    tokio::spawn(async move {
        let path = summary_path();
        let mut last: Option<std::time::SystemTime> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            let modified = meta.modified().ok();
            if modified == last {
                continue;
            }
            last = modified;
            if let Ok(text) = fs::read_to_string(&path)
                && !text.trim().is_empty()
            {
                if let (Some(dir), Some(written)) = (path.parent(), modified) {
                    snapshot_summary(dir, &text, written);
                }
                let _ = event_tx.send(crate::audio::AudioEvent::Summary(text));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_one_json_object_per_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LiveLog::default();
        log.path = dir.path().join(STREAM_FILE);
        log.file = File::create(&log.path).ok();
        log.transcript(12, "S1", "hello");
        log.summary("- point");
        log.stop(Some("abc"), Some("/calls/20261025-abc.wav"));
        let body = fs::read_to_string(&log.path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3);
        for l in &lines {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert!(v.get("type").is_some());
        }
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0]).unwrap()["speaker"],
            "S1"
        );
        let stop: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(stop["wav"], "/calls/20261025-abc.wav");
    }

    #[test]
    fn heartbeat_writes_once_per_interval_and_never_when_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LiveLog::default();
        log.heartbeat_if_due(); // closed: nothing to write to, no panic
        log.path = dir.path().join(STREAM_FILE);
        log.file = File::create(&log.path).ok();
        log.heartbeat_if_due();
        log.heartbeat_if_due(); // within the interval: no second line
        let body = fs::read_to_string(&log.path).unwrap();
        assert_eq!(body.lines().count(), 1);
        assert!(body.contains("\"type\":\"heartbeat\""));
    }

    /// A stream that started at `start`, optionally with speech and a WAV in its stop line.
    fn write_stream(
        dir: &Path,
        start: chrono::DateTime<chrono::Local>,
        speech: bool,
        wav: Option<&Path>,
    ) {
        let mut log = LiveLog {
            path: dir.join(STREAM_FILE),
            ..LiveLog::default()
        };
        log.file = File::create(&log.path).ok();
        log.write(serde_json::json!({"type": "start", "ts": start.to_rfc3339()}));
        if speech {
            log.transcript(3, "S1", "hello");
        }
        log.stop(Some("abc"), wav.and_then(Path::to_str));
    }

    fn set_mtime(path: &Path, t: chrono::DateTime<chrono::Local>) {
        let f = File::options().write(true).open(path).unwrap();
        f.set_modified(t.into()).unwrap();
    }

    fn stamp(t: chrono::DateTime<chrono::Local>) -> String {
        t.format("%Y%m%d-%H%M%S").to_string()
    }

    #[test]
    fn a_session_is_archived_into_one_folder_named_by_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let start = chrono::Local::now() - chrono::Duration::minutes(5);
        write_stream(dir.path(), start, true, None);
        fs::write(dir.path().join(SUMMARY_FILE), "## Сейчас\n").unwrap();
        let out = archive_session(dir.path()).expect("archived");
        assert_eq!(out, dir.path().join("sessions").join(stamp(start)));
        assert!(!dir.path().join(STREAM_FILE).exists());
        assert!(!dir.path().join(SUMMARY_FILE).exists());
        assert!(
            fs::read_to_string(out.join("stream.jsonl"))
                .unwrap()
                .contains("hello")
        );
        assert_eq!(
            fs::read_to_string(out.join(SUMMARY_FILE)).unwrap(),
            "## Сейчас\n"
        );
        // nothing to archive: an empty or missing stream is left alone
        assert!(archive_session(dir.path()).is_none());
    }

    #[test]
    fn a_summary_older_than_the_stream_is_not_glued_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let start = chrono::Local::now() - chrono::Duration::minutes(5);
        write_stream(dir.path(), start, true, None);
        let summary = dir.path().join(SUMMARY_FILE);
        fs::write(&summary, "earlier talk\n").unwrap();
        let written = start - chrono::Duration::hours(2);
        set_mtime(&summary, written);
        let out = archive_session(dir.path()).expect("archived");
        assert!(!out.join(SUMMARY_FILE).exists());
        // kept under its own time, out of the way of the next session's agent
        assert!(!summary.exists());
        let orphan = dir
            .path()
            .join("sessions")
            .join(format!("{}.summary.md", stamp(written)));
        assert_eq!(fs::read_to_string(orphan).unwrap(), "earlier talk\n");
    }

    #[test]
    fn a_stream_without_speech_is_marked_empty_and_takes_no_summary() {
        let dir = tempfile::tempdir().unwrap();
        let start = chrono::Local::now() - chrono::Duration::minutes(5);
        write_stream(dir.path(), start, false, None);
        fs::write(dir.path().join(SUMMARY_FILE), "about something else\n").unwrap();
        let out = archive_session(dir.path()).expect("archived");
        assert_eq!(
            out.file_name().unwrap().to_string_lossy(),
            format!("{}-empty", stamp(start))
        );
        assert!(out.join("stream.jsonl").exists());
        assert!(!out.join(SUMMARY_FILE).exists());
    }

    #[test]
    fn every_summary_version_and_the_wav_land_in_the_session_folder() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("20261025-abc.wav");
        fs::write(&wav, b"RIFF").unwrap();
        let start = chrono::Local::now() - chrono::Duration::minutes(5);
        write_stream(dir.path(), start, true, Some(&wav));
        let first = start + chrono::Duration::seconds(60);
        let second = start + chrono::Duration::seconds(120);
        let a = snapshot_summary(dir.path(), "v1\n", first.into()).expect("snapshot");
        assert_eq!(
            a,
            dir.path()
                .join("summary-history")
                .join(format!("{}.md", first.format("%H%M%S")))
        );
        snapshot_summary(dir.path(), "v2\n", second.into()).expect("snapshot");
        let out = archive_session(dir.path()).expect("archived");
        let read = |n: String| fs::read_to_string(out.join("summaries").join(n)).unwrap();
        assert_eq!(read(format!("{}.md", first.format("%H%M%S"))), "v1\n");
        assert_eq!(read(format!("{}.md", second.format("%H%M%S"))), "v2\n");
        assert!(!dir.path().join("summary-history").exists());
        // the WAV stays with the call; the folder only points at it
        assert!(wav.exists());
        assert_eq!(fs::read(out.join("recording.wav")).unwrap(), b"RIFF");
    }

    #[test]
    fn a_folder_an_agent_already_made_for_the_session_is_reused() {
        let dir = tempfile::tempdir().unwrap();
        let start = chrono::Local::now() - chrono::Duration::minutes(5);
        write_stream(dir.path(), start, true, None);
        let named = dir
            .path()
            .join("sessions")
            .join(format!("{}-dell-nvidia", stamp(start)));
        fs::create_dir_all(named.join("summaries")).unwrap();
        fs::write(named.join("judges.md"), "notes\n").unwrap();
        fs::write(named.join("summaries").join("010101.md"), "agent's\n").unwrap();
        fs::write(named.join("stream.jsonl"), "partial copy\n").unwrap();
        snapshot_summary(
            dir.path(),
            "v1\n",
            (start + chrono::Duration::seconds(60)).into(),
        );
        let out = archive_session(dir.path()).expect("archived");
        assert_eq!(out, named);
        assert_eq!(
            fs::read_dir(dir.path().join("sessions")).unwrap().count(),
            1
        );
        assert_eq!(
            fs::read_to_string(named.join("judges.md")).unwrap(),
            "notes\n"
        );
        assert_eq!(fs::read_dir(named.join("summaries")).unwrap().count(), 2);
        // the full stream replaces an agent's mid-session copy of it
        assert!(
            fs::read_to_string(named.join("stream.jsonl"))
                .unwrap()
                .contains("hello")
        );
    }

    #[test]
    fn the_call_folder_gets_the_stream_and_the_summary() {
        let live = tempfile::tempdir().unwrap();
        let calls = tempfile::tempdir().unwrap();
        fs::write(live.path().join(STREAM_FILE), "x\n").unwrap();
        fs::write(live.path().join(SUMMARY_FILE), "y\n").unwrap();
        fs::write(calls.path().join("2026-10-24-abc.json"), "{}").unwrap();
        let log = LiveLog {
            path: live.path().join(STREAM_FILE),
            ..LiveLog::default()
        };
        let kept = log.keep_with_call(calls.path(), "abc");
        assert_eq!(kept.len(), 2);
        // named after the call's own stem, so the files sort and group with it
        let read = |n: &str| fs::read_to_string(calls.path().join(n)).unwrap();
        assert_eq!(read("2026-10-24-abc.live.jsonl"), "x\n");
        assert_eq!(read("2026-10-24-abc.summary.md"), "y\n");
    }
}
