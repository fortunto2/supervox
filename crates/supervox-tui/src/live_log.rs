//! Live JSONL stream for outside agents.
//!
//! Every final transcript line (and every summary shown) is appended to
//! `~/.supervox/live/current.jsonl` while recording. An agent (Claude Code,
//! a script) tails that file and may write `~/.supervox/live/summary.md`,
//! which supervox displays in the summary panel when `summary = "external"`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STREAM_FILE: &str = "current.jsonl";
pub const SUMMARY_FILE: &str = "summary.md";

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
}

impl LiveLog {
    /// Truncate the stream and write a `start` event.
    pub fn start(&mut self, stt_backend: &str, mic_device: Option<&str>) {
        let dir = live_dir();
        let _ = fs::create_dir_all(&dir);
        self.path = dir.join(STREAM_FILE);
        // A fresh session: the old summary belongs to the old call.
        let _ = fs::remove_file(summary_path());
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

    pub fn stop(&mut self, call_id: Option<&str>) {
        self.write(serde_json::json!({
            "type": "stop",
            "ts": chrono::Local::now().to_rfc3339(),
            "call_id": call_id,
        }));
        self.file = None;
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
        log.stop(Some("abc"));
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
    }
}
