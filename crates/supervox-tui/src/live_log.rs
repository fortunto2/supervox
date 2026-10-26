//! Live JSONL stream for outside agents.
//!
//! Every recording lives in its own folder from its first second,
//! `~/.supervox/live/sessions/<YYYYMMDD-HHMMSS>/`: `stream.jsonl` (every final transcript
//! line and every summary shown), `summary.md` (what an agent writes; shown in the summary
//! panel when `summary = "external"`), `summaries/<HHMMSS>.md` (every version the panel
//! showed) and `recording.wav` (a link to the call's WAV). Agents keep using the fixed
//! names: `live/current.jsonl` and `live/summary.md` are symlinks into the folder.
//!
//! A tool that saves `summary.md` by rename replaces the symlink with a regular file. Nothing
//! ever writes back to that path: the watcher reads `live/summary.md` whatever it is and copies
//! each new version into the folder, and the next start adopts the file and relinks.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// `live/current.jsonl`: a link to the stream of the session being recorded.
pub const STREAM_FILE: &str = "current.jsonl";
/// `live/summary.md` (a link) and `<session>/summary.md` (the file).
pub const SUMMARY_FILE: &str = "summary.md";
pub const SESSIONS_DIR: &str = "sessions";
/// The stream inside a session folder.
pub const SESSION_STREAM: &str = "stream.jsonl";
/// Every version of the summary, `<HHMMSS>.md`, inside a session folder.
pub const SUMMARIES_DIR: &str = "summaries";
pub const RECORDING: &str = "recording.wav";
/// Where supervox before the folder layout staged summary versions; read by the legacy archive only.
pub const HISTORY_DIR: &str = "summary-history";
const EMPTY: &str = "-empty";
const STAMP: &str = "%Y%m%d-%H%M%S";

/// `~/.supervox/live`
pub fn live_dir() -> PathBuf {
    supervox_agent::storage::default_config_path()
        .parent()
        .map(|p| p.join("live"))
        .unwrap_or_else(|| PathBuf::from(".supervox/live"))
}

/// Appends events to the live stream. Silent on IO errors: the stream is a
/// convenience for agents, never a reason to break recording.
#[derive(Default)]
pub struct LiveLog {
    file: Option<File>,
    /// The stream being written, `<session>/stream.jsonl`.
    path: PathBuf,
    live: PathBuf,
    session: Option<PathBuf>,
    last_beat: Option<std::time::Instant>,
}

/// How often a recording writes a `heartbeat` line, so a reader can tell
/// "quiet room" from "recording stopped".
pub const HEARTBEAT_SECS: u64 = 30;

impl LiveLog {
    /// Open a fresh session folder and write a `start` event.
    pub fn start(&mut self, stt_backend: &str, mic_device: Option<&str>) {
        self.start_at(&live_dir(), chrono::Local::now(), stt_backend, mic_device);
    }

    fn start_at(
        &mut self,
        live: &Path,
        now: chrono::DateTime<chrono::Local>,
        stt_backend: &str,
        mic_device: Option<&str>,
    ) {
        self.live = live.to_path_buf();
        self.last_beat = None;
        self.session = begin_session(live, now);
        self.path = match &self.session {
            Some(session) => session.join(SESSION_STREAM),
            None => {
                // No folder (a broken disk): the old flat file, never through a stale link
                let flat = live.join(STREAM_FILE);
                let _ = fs::remove_file(&flat);
                flat
            }
        };
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.write(serde_json::json!({
            "type": "start",
            "ts": now.to_rfc3339(),
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

    /// Close the stream. `wav` is the saved call's recording, linked into the session folder.
    /// A session without speech is removed, or renamed `-empty` if an agent left files in it.
    pub fn stop(&mut self, call_id: Option<&str>, wav: Option<&str>) {
        self.write(serde_json::json!({
            "type": "stop",
            "ts": chrono::Local::now().to_rfc3339(),
            "call_id": call_id,
            "wav": wav,
        }));
        self.file = None;
        let Some(session) = self.session.take() else {
            return;
        };
        if let Some(wav) = wav.map(Path::new).filter(|w| w.exists()) {
            let _ = symlink(wav, &session.join(RECORDING));
        }
        adopt_summary(&self.live.join(SUMMARY_FILE), &session);
        self.session = finish_session(&self.live, &session);
        if let Some(session) = &self.session {
            self.path = session.join(SESSION_STREAM);
        }
    }

    /// Link the finished session's stream and summary next to the saved call as
    /// `<call stem>.live.jsonl` / `<call stem>.summary.md`. A hard link costs nothing
    /// and sees later in-place edits; copy only across volumes.
    pub fn keep_with_call(&self, calls_dir: &Path, call_id: &str) -> Vec<PathBuf> {
        let Some(session) = self.session.as_deref().filter(|_| !call_id.is_empty()) else {
            return Vec::new();
        };
        let stem = supervox_agent::storage::call_stem(calls_dir, call_id)
            .unwrap_or_else(|| call_id.to_string());
        let mut kept = Vec::new();
        for (name, suffix) in [(SESSION_STREAM, "live.jsonl"), (SUMMARY_FILE, "summary.md")] {
            let (src, dest) = (
                session.join(name),
                calls_dir.join(format!("{stem}.{suffix}")),
            );
            if fs::hard_link(&src, &dest).is_ok() || fs::copy(&src, &dest).is_ok() {
                kept.push(dest);
            }
        }
        kept
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn live(&self) -> &Path {
        &self.live
    }

    /// The current (or just stopped) session folder; `None` once a silent one was removed.
    pub fn session(&self) -> Option<&Path> {
        self.session.as_deref()
    }

    /// `20261025-184632`: the session folder's name, for the panel title.
    pub fn session_name(&self) -> Option<String> {
        self.session
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
    }

    fn write(&mut self, v: serde_json::Value) {
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{v}");
            let _ = f.flush();
        }
    }
}

/// Close out the previous session, make `sessions/<stamp>/` with an empty `stream.jsonl`, and
/// point `live/current.jsonl` and `live/summary.md` at it. `summary.md` is a dangling link
/// until an agent writes it, so the panel never starts on the last talk's summary.
fn begin_session(live: &Path, now: chrono::DateTime<chrono::Local>) -> Option<PathBuf> {
    fs::create_dir_all(live).ok()?;
    settle_previous(live);
    let session = fresh_dir(&live.join(SESSIONS_DIR), &now.format(STAMP).to_string())?;
    File::create(session.join(SESSION_STREAM)).ok()?;
    point_links(live, Some(&session));
    Some(session)
}

/// Whatever the last start left in `live/`: a session folder gets the summary an agent saved
/// by rename and the empty check (it may have crashed before `stop`); a regular
/// `current.jsonl` from an older supervox is archived the old way; a summary no session
/// claims is kept as `sessions/<its stamp>.summary.md`, out of the next agent's way.
fn settle_previous(live: &Path) {
    let summary = live.join(SUMMARY_FILE);
    let mut adopted = false;
    match fs::symlink_metadata(live.join(STREAM_FILE)) {
        Ok(m) if m.file_type().is_symlink() => {
            if let Some(prev) = linked_session(live) {
                adopted = adopt_summary(&summary, &prev);
                finish_session(live, &prev);
            }
        }
        Ok(_) => {
            archive_session(live);
        }
        Err(_) => {}
    }
    let Ok(meta) = fs::symlink_metadata(&summary) else {
        return;
    };
    let blank = fs::read_to_string(&summary).is_ok_and(|t| t.trim().is_empty());
    if meta.file_type().is_symlink() || adopted || blank {
        let _ = fs::remove_file(&summary);
    } else if let Some(written) = modified_local(&summary) {
        let sessions = live.join(SESSIONS_DIR);
        let _ = fs::create_dir_all(&sessions);
        let name = format!("{}.summary.md", written.format(STAMP));
        let _ = fs::rename(&summary, sessions.join(name));
    }
}

/// The folder `live/current.jsonl` points into.
fn linked_session(live: &Path) -> Option<PathBuf> {
    let target = fs::read_link(live.join(STREAM_FILE)).ok()?;
    let dir = live.join(target).parent()?.to_path_buf();
    dir.is_dir().then_some(dir)
}

/// `sessions/<stamp>/`, or `<stamp>-2/` and on if a restart landed in the same second.
fn fresh_dir(sessions: &Path, stamp: &str) -> Option<PathBuf> {
    fs::create_dir_all(sessions).ok()?;
    (1..100)
        .map(|n| match n {
            1 => sessions.join(stamp),
            n => sessions.join(format!("{stamp}-{n}")),
        })
        .find(|p| fs::create_dir(p).is_ok())
}

/// `live/current.jsonl` and `live/summary.md` → that session's files (relative links), or
/// gone. A regular `summary.md` is an agent's save by rename that has not been adopted yet:
/// left alone.
fn point_links(live: &Path, session: Option<&Path>) {
    for (name, file) in [(STREAM_FILE, SESSION_STREAM), (SUMMARY_FILE, SUMMARY_FILE)] {
        let link = live.join(name);
        let regular = fs::symlink_metadata(&link).is_ok_and(|m| !m.file_type().is_symlink());
        if regular && name == SUMMARY_FILE {
            continue;
        }
        match session.and_then(Path::file_name) {
            Some(dir) => replace_link(&Path::new(SESSIONS_DIR).join(dir).join(file), &link),
            None => {
                let _ = fs::remove_file(&link);
            }
        }
    }
}

/// Swap `link` to point at `target` in one rename, so a reader never sees it missing.
fn replace_link(target: &Path, link: &Path) {
    let tmp = link.with_extension("link-tmp");
    let _ = fs::remove_file(&tmp);
    if symlink(target, &tmp).is_ok() {
        let _ = fs::rename(&tmp, link);
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

/// A stream without one transcript line is junk: the folder is removed if it holds only
/// the stream and a blank summary, renamed `<name>-empty` if an agent left anything else.
/// Returns where the session is now; idempotent.
fn finish_session(live: &Path, session: &Path) -> Option<PathBuf> {
    let name = session.file_name()?.to_string_lossy().into_owned();
    let spoke = fs::read_to_string(session.join(SESSION_STREAM)).map(|b| facts_of(&b).spoke);
    if name.ends_with(EMPTY) || !matches!(spoke, Ok(false)) {
        return Some(session.to_path_buf());
    }
    let linked = linked_session(live).is_some_and(|p| p.file_name() == session.file_name());
    let now = if only_scaffolding(session) {
        if fs::remove_dir_all(session).is_err() {
            return Some(session.to_path_buf());
        }
        None
    } else {
        let renamed = session.with_file_name(format!("{name}{EMPTY}"));
        if fs::rename(session, &renamed).is_err() {
            return Some(session.to_path_buf());
        }
        Some(renamed)
    };
    if linked {
        point_links(live, now.as_deref());
    }
    now
}

/// Only what `begin_session` made: the stream, maybe a blank summary.
fn only_scaffolding(session: &Path) -> bool {
    fs::read_dir(session)
        .into_iter()
        .flatten()
        .flatten()
        .all(|e| {
            let name = e.file_name();
            name == SESSION_STREAM
                || (name == SUMMARY_FILE
                    && fs::read_to_string(e.path()).is_ok_and(|t| t.trim().is_empty()))
        })
}

/// A tool that saves by rename turned the `live/summary.md` link into a regular file; its
/// text belongs to the session. Copied in, never moved: the agent may still write. True if
/// there was such a file and it was read.
fn adopt_summary(link: &Path, session: &Path) -> bool {
    if !fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_file()) {
        return false;
    }
    let Ok(text) = fs::read_to_string(link) else {
        return false;
    };
    if !text.trim().is_empty() {
        let written = modified(link).unwrap_or_else(SystemTime::now);
        keep_version(session, &text, written);
    }
    true
}

/// `<session>/summary.md` gets `text` unless it already is it (a write through the link),
/// and `summaries/<HHMMSS>.md` keeps it past the next overwrite.
fn keep_version(session: &Path, text: &str, written: SystemTime) {
    let dest = session.join(SUMMARY_FILE);
    if fs::read_to_string(&dest).ok().as_deref() != Some(text) {
        let _ = fs::write(&dest, text);
    }
    snapshot_summary(session, text, written);
}

/// Keep a copy of a summary as `<session>/summaries/<HHMMSS>.md` (its write time).
pub fn snapshot_summary(session: &Path, text: &str, written: SystemTime) -> Option<PathBuf> {
    let out = session.join(SUMMARIES_DIR);
    fs::create_dir_all(&out).ok()?;
    let name = chrono::DateTime::<chrono::Local>::from(written).format("%H%M%S");
    let dest = out.join(format!("{name}.md"));
    fs::write(&dest, text).ok()?;
    Some(dest)
}

/// One look at `live/summary.md`, whatever it is now: the link (the write already landed in
/// the session folder) or a regular file (a tool saved by rename; its text is copied in).
/// A new non-blank version is kept in `summaries/` and returned for the panel.
pub fn sync_summary(live: &Path, session: &Path, last: &mut Option<String>) -> Option<String> {
    let path = live.join(SUMMARY_FILE);
    let written = modified(&path).unwrap_or_else(SystemTime::now);
    let text = fs::read_to_string(&path).ok()?;
    if text.trim().is_empty() || last.as_deref() == Some(text.as_str()) {
        return None;
    }
    keep_version(session, &text, written);
    *last = Some(text.clone());
    Some(text)
}

/// Every 2 s, [`sync_summary`] into the panel. Stops when `stop` is set.
pub fn watch_external_summary(
    event_tx: tokio::sync::mpsc::UnboundedSender<crate::audio::AudioEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    live: PathBuf,
    session: PathBuf,
) {
    tokio::spawn(async move {
        let mut last: Option<String> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tick.tick().await;
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if let Some(text) = sync_summary(&live, &session, &mut last) {
                let _ = event_tx.send(crate::audio::AudioEvent::Summary(text));
            }
        }
    });
}

/// The layout of supervox before session folders: a regular `live/current.jsonl`, a regular
/// `summary.md` and `summary-history/`. Moved into `<dir>/sessions/<start stamp>/` as
/// `stream.jsonl`, `summary.md`, `summaries/` and `recording.wav` (a link to the call's WAV).
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
    let facts = facts_of(&fs::read_to_string(&stream).unwrap_or_default());
    let start = facts.start.or_else(|| {
        meta.modified()
            .ok()
            .map(chrono::DateTime::<chrono::Local>::from)
    });
    let stamp = start
        .map(|t| t.format(STAMP).to_string())
        .unwrap_or_else(|| "unknown".into());
    let sessions = dir.join(SESSIONS_DIR);
    let out = session_folder(&sessions, &stamp, facts.spoke);
    fs::create_dir_all(&out).ok()?;
    fs::rename(&stream, out.join(SESSION_STREAM)).ok()?;

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
    move_all(&dir.join(HISTORY_DIR), &out.join(SUMMARIES_DIR));
    if let Some(wav) = facts.wav.as_deref().map(Path::new).filter(|w| w.exists()) {
        let _ = symlink(wav, &out.join(RECORDING));
    }
    Some(out)
}

/// What a session needs from a stream, read from its own lines.
#[derive(Default)]
struct StreamFacts {
    start: Option<chrono::DateTime<chrono::Local>>,
    spoke: bool,
    wav: Option<String>,
}

fn facts_of(body: &str) -> StreamFacts {
    let mut facts = StreamFacts::default();
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

/// Legacy archive: `sessions/<stamp>/`, unless an agent already made `<stamp>-<slug>/` for
/// frames and notes mid-session: then that one, so one recording ends up in one place.
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
            false => sessions.join(format!("{stamp}{EMPTY}")),
        })
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn modified_local(path: &Path) -> Option<chrono::DateTime<chrono::Local>> {
    modified(path).map(Into::into)
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

#[cfg(test)]
mod tests {
    use super::*;

    type Local = chrono::DateTime<chrono::Local>;

    fn minutes_ago(m: i64) -> Local {
        chrono::Local::now() - chrono::Duration::minutes(m)
    }

    fn stamp(t: Local) -> String {
        t.format("%Y%m%d-%H%M%S").to_string()
    }

    fn started(live: &Path, at: Local) -> LiveLog {
        let mut log = LiveLog::default();
        log.start_at(live, at, "parakeet", None);
        log
    }

    /// What an editor that saves atomically does: a temp file renamed over the path.
    fn save_by_rename(path: &Path, text: &str) {
        let tmp = path.with_extension("tmp-save");
        fs::write(&tmp, text).unwrap();
        fs::rename(&tmp, path).unwrap();
    }

    fn is_link(p: &Path) -> bool {
        fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    }

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

    // --- session folders (the layout since the folder is the home from the first second)

    #[test]
    fn start_writes_into_the_session_folder_through_the_live_names() {
        let live = tempfile::tempdir().unwrap();
        let at = minutes_ago(1);
        let mut log = started(live.path(), at);
        log.transcript(3, "S1", "hello");
        let session = live.path().join(SESSIONS_DIR).join(stamp(at));
        assert_eq!(log.session(), Some(session.as_path()));
        assert_eq!(log.session_name().unwrap(), stamp(at));
        let stream = fs::read_to_string(session.join(SESSION_STREAM)).unwrap();
        assert!(stream.starts_with("{") && stream.contains("hello"));
        // agents tail the old name: it is a relative link to the folder's stream
        let current = live.path().join(STREAM_FILE);
        assert!(is_link(&current));
        assert!(fs::read_link(&current).unwrap().is_relative());
        assert_eq!(fs::read_to_string(&current).unwrap(), stream);
        // summary.md waits as a dangling link: the panel starts blank
        let summary = live.path().join(SUMMARY_FILE);
        assert!(is_link(&summary) && !summary.exists());
        fs::write(&summary, "## Сейчас\n").unwrap();
        assert_eq!(
            fs::read_to_string(session.join(SUMMARY_FILE)).unwrap(),
            "## Сейчас\n"
        );
    }

    #[test]
    fn every_summary_version_is_kept_whether_written_through_the_link_or_by_rename() {
        let live = tempfile::tempdir().unwrap();
        let log = started(live.path(), minutes_ago(1));
        let session = log.session().unwrap().to_path_buf();
        let link = live.path().join(SUMMARY_FILE);
        let mut last = None;
        assert_eq!(sync_summary(live.path(), &session, &mut last), None);

        fs::write(&link, "v1\n").unwrap();
        let first = SystemTime::now() - std::time::Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&link)
            .unwrap()
            .set_modified(first)
            .unwrap();
        assert_eq!(
            sync_summary(live.path(), &session, &mut last).unwrap(),
            "v1\n"
        );
        assert_eq!(sync_summary(live.path(), &session, &mut last), None); // unchanged

        // an editor's atomic save replaces the link with a regular file
        save_by_rename(&link, "v2\n");
        assert!(!is_link(&link));
        assert_eq!(
            sync_summary(live.path(), &session, &mut last).unwrap(),
            "v2\n"
        );
        assert_eq!(
            fs::read_to_string(session.join(SUMMARY_FILE)).unwrap(),
            "v2\n"
        );
        // the agent's path is never written back to
        assert!(!is_link(&link));
        let versions: Vec<String> = fs::read_dir(session.join(SUMMARIES_DIR))
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        assert_eq!(versions.len(), 2);
        assert!(versions.contains(&"v1\n".to_string()) && versions.contains(&"v2\n".to_string()));
    }

    #[test]
    fn the_next_start_adopts_a_summary_saved_by_rename_and_opens_a_blank_panel() {
        let live = tempfile::tempdir().unwrap();
        let a = minutes_ago(10);
        let mut log = started(live.path(), a);
        log.transcript(3, "S1", "hello");
        log.stop(Some("abc"), None);
        // the agent's final summary, after stop, saved by rename
        save_by_rename(&live.path().join(SUMMARY_FILE), "final\n");
        let b = minutes_ago(1);
        let log = started(live.path(), b);
        let first = live.path().join(SESSIONS_DIR).join(stamp(a));
        assert_eq!(
            fs::read_to_string(first.join(SUMMARY_FILE)).unwrap(),
            "final\n"
        );
        assert_eq!(fs::read_dir(first.join(SUMMARIES_DIR)).unwrap().count(), 1);
        let summary = live.path().join(SUMMARY_FILE);
        assert!(is_link(&summary) && !summary.exists());
        assert_eq!(log.session_name().unwrap(), stamp(b));
        assert!(
            fs::read_to_string(live.path().join(STREAM_FILE))
                .unwrap()
                .contains("\"type\":\"start\"")
        );
    }

    #[test]
    fn a_recording_without_speech_leaves_nothing_behind() {
        let live = tempfile::tempdir().unwrap();
        let mut log = started(live.path(), minutes_ago(1));
        log.heartbeat_if_due();
        log.stop(None, None);
        assert!(log.session().is_none());
        assert_eq!(
            fs::read_dir(live.path().join(SESSIONS_DIR))
                .unwrap()
                .count(),
            0
        );
        for name in [STREAM_FILE, SUMMARY_FILE] {
            assert!(fs::symlink_metadata(live.path().join(name)).is_err());
        }
        // a crash before stop is settled the same way on the next start
        let crashed = started(live.path(), minutes_ago(3));
        drop(crashed);
        started(live.path(), minutes_ago(2));
        assert_eq!(
            fs::read_dir(live.path().join(SESSIONS_DIR))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn a_silent_recording_an_agent_wrote_into_is_kept_as_empty() {
        let live = tempfile::tempdir().unwrap();
        let at = minutes_ago(1);
        let mut log = started(live.path(), at);
        fs::write(log.session().unwrap().join("notes.md"), "frames\n").unwrap();
        log.stop(None, None);
        let kept = live
            .path()
            .join(SESSIONS_DIR)
            .join(format!("{}-empty", stamp(at)));
        assert_eq!(log.session(), Some(kept.as_path()));
        assert_eq!(
            fs::read_to_string(kept.join("notes.md")).unwrap(),
            "frames\n"
        );
        // the live names follow the rename
        assert!(
            fs::read_to_string(live.path().join(STREAM_FILE))
                .unwrap()
                .contains("\"type\":\"stop\"")
        );
        // and a second look does not rename it again
        assert_eq!(finish_session(live.path(), &kept), Some(kept.clone()));
    }

    #[test]
    fn stop_links_the_wav_into_the_session_folder() {
        let live = tempfile::tempdir().unwrap();
        let wav = live.path().join("20261025-abc.wav");
        fs::write(&wav, b"RIFF").unwrap();
        let mut log = started(live.path(), minutes_ago(1));
        log.transcript(3, "S1", "hello");
        log.stop(Some("abc"), wav.to_str());
        let session = log.session().unwrap();
        // the WAV stays with the call; the folder only points at it
        assert!(is_link(&session.join(RECORDING)));
        assert_eq!(fs::read(session.join(RECORDING)).unwrap(), b"RIFF");
    }

    #[test]
    fn the_call_folder_gets_the_stream_and_the_summary() {
        let live = tempfile::tempdir().unwrap();
        let calls = tempfile::tempdir().unwrap();
        fs::write(calls.path().join("2026-10-24-abc.json"), "{}").unwrap();
        let mut log = started(live.path(), minutes_ago(1));
        log.transcript(3, "S1", "x");
        fs::write(live.path().join(SUMMARY_FILE), "y\n").unwrap();
        log.stop(Some("abc"), None);
        let kept = log.keep_with_call(calls.path(), "abc");
        assert_eq!(kept.len(), 2);
        // named after the call's own stem, so the files sort and group with it
        let read = |n: &str| fs::read_to_string(calls.path().join(n)).unwrap();
        assert!(read("2026-10-24-abc.live.jsonl").contains("\"text\":\"x\""));
        assert_eq!(read("2026-10-24-abc.summary.md"), "y\n");
        // no call, nothing to keep
        assert!(log.keep_with_call(calls.path(), "").is_empty());
    }

    #[test]
    fn a_stream_an_older_supervox_wrote_in_place_is_archived_before_the_new_session() {
        let live = tempfile::tempdir().unwrap();
        let old = minutes_ago(30);
        write_stream(live.path(), old, true, None);
        fs::write(live.path().join(SUMMARY_FILE), "old talk\n").unwrap();
        let now = minutes_ago(1);
        started(live.path(), now);
        let archived = live.path().join(SESSIONS_DIR).join(stamp(old));
        assert!(
            fs::read_to_string(archived.join(SESSION_STREAM))
                .unwrap()
                .contains("hello")
        );
        assert_eq!(
            fs::read_to_string(archived.join(SUMMARY_FILE)).unwrap(),
            "old talk\n"
        );
        assert!(is_link(&live.path().join(STREAM_FILE)));
        assert!(live.path().join(SESSIONS_DIR).join(stamp(now)).is_dir());
    }

    #[test]
    fn a_summary_no_session_claims_is_set_aside_not_shown_again() {
        let live = tempfile::tempdir().unwrap();
        let summary = live.path().join(SUMMARY_FILE);
        fs::write(&summary, "stray\n").unwrap();
        let written = minutes_ago(90);
        File::options()
            .write(true)
            .open(&summary)
            .unwrap()
            .set_modified(written.into())
            .unwrap();
        started(live.path(), minutes_ago(1));
        let aside = live
            .path()
            .join(SESSIONS_DIR)
            .join(format!("{}.summary.md", stamp(written)));
        assert_eq!(fs::read_to_string(aside).unwrap(), "stray\n");
        assert!(is_link(&summary) && !summary.exists());
    }

    #[test]
    fn two_starts_in_one_second_get_two_folders() {
        let live = tempfile::tempdir().unwrap();
        let at = minutes_ago(1);
        let mut first = started(live.path(), at);
        first.transcript(1, "S1", "a");
        first.stop(Some("x"), None);
        let second = started(live.path(), at);
        assert_eq!(second.session_name().unwrap(), format!("{}-2", stamp(at)));
    }

    // --- legacy: the flat layout an older supervox leaves behind

    /// A stream that started at `start`, optionally with speech and a WAV in its stop line.
    fn write_stream(dir: &Path, start: Local, speech: bool, wav: Option<&Path>) {
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

    fn set_mtime(path: &Path, t: Local) {
        let f = File::options().write(true).open(path).unwrap();
        f.set_modified(t.into()).unwrap();
    }

    /// What an older supervox's watcher staged: `summary-history/<HHMMSS>.md`.
    fn stage_history(dir: &Path, at: Local, text: &str) {
        let history = dir.join(HISTORY_DIR);
        fs::create_dir_all(&history).unwrap();
        fs::write(history.join(format!("{}.md", at.format("%H%M%S"))), text).unwrap();
    }

    #[test]
    fn a_session_is_archived_into_one_folder_named_by_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let start = minutes_ago(5);
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
        let start = minutes_ago(5);
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
        let start = minutes_ago(5);
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
        let start = minutes_ago(5);
        write_stream(dir.path(), start, true, Some(&wav));
        let first = start + chrono::Duration::seconds(60);
        let second = start + chrono::Duration::seconds(120);
        stage_history(dir.path(), first, "v1\n");
        stage_history(dir.path(), second, "v2\n");
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
        let start = minutes_ago(5);
        write_stream(dir.path(), start, true, None);
        let named = dir
            .path()
            .join("sessions")
            .join(format!("{}-dell-nvidia", stamp(start)));
        fs::create_dir_all(named.join("summaries")).unwrap();
        fs::write(named.join("judges.md"), "notes\n").unwrap();
        fs::write(named.join("summaries").join("010101.md"), "agent's\n").unwrap();
        fs::write(named.join("stream.jsonl"), "partial copy\n").unwrap();
        stage_history(dir.path(), start + chrono::Duration::seconds(60), "v1\n");
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
}
