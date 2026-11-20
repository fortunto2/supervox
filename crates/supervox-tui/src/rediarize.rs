//! Post-stop speaker re-labelling of a live session.
//!
//! Live labels (CAM++ online clustering) are decided one segment at a time with no hindsight.
//! After stop the whole recording is diarized offline by a sidecar, FluidAudio's
//! `fluidaudiocli` (pyannote segmentation + WeSpeaker + VBx on the Neural Engine, 15 s for 48
//! minutes, benchmarked 19.11 against CAM++ live and Sortformer), and every transcript line
//! that carries its audio span (`a`/`b` in `stream.jsonl`) gets the speaker covering most of
//! it. A speaker whose voice matches a voiceprint in `~/.supervox/voices` takes its name.
//!
//! Lines without a span (sessions recorded before spans existed) keep their live label: the
//! arrival time `t` cannot be matched to the WAV, it ran 100-170 s behind the audio.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use voxkit::rediarize::{self, Turn};

/// Env override for the sidecar binary.
pub const BIN_ENV: &str = "SUPERVOX_DIARIZER_BIN";
/// Kept next to `stream.jsonl`: the live labels as they were before the rewrite.
pub const ORIG_SUFFIX: &str = "orig";
/// What the diarizer decided, for audit: `{segments, names}`.
pub const REPORT_FILE: &str = "diarization.json";

/// `$SUPERVOX_DIARIZER_BIN`, else `~/.supervox/bin/fluidaudiocli`.
pub fn sidecar_path() -> PathBuf {
    if let Some(p) = std::env::var_os(BIN_ENV) {
        return PathBuf::from(p);
    }
    voxkit::paths::supervox_home()
        .join("bin")
        .join("fluidaudiocli")
}

/// What a run changed.
#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    pub relabelled: usize,
    pub unchanged: usize,
    pub no_span: usize,
    pub agent: usize,
}

/// New `stream.jsonl` text: every transcript line with a span takes the label of the speaker
/// covering most of it. Lines of the agent's own voice, lines without a span and every other
/// event pass through untouched.
pub fn relabel_stream(
    text: &str,
    turns: &[Turn],
    labels: &HashMap<String, String>,
    agent_name: &str,
) -> (String, Stats) {
    let mut out = String::with_capacity(text.len());
    let mut st = Stats::default();
    for line in text.lines() {
        let mut v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                out.push_str(line);
                out.push('\n');
                continue;
            }
        };
        if v["type"] == "transcript" {
            let span = v["a"].as_f64().zip(v["b"].as_f64());
            let now = v["speaker"].as_str().unwrap_or_default().to_string();
            if now == agent_name {
                st.agent += 1;
            } else if let Some((a, b)) = span {
                match rediarize::majority_speaker(turns, a as f32, b as f32)
                    .and_then(|s| labels.get(s))
                {
                    Some(new) if *new != now => {
                        v["speaker"] = serde_json::Value::String(new.clone());
                        st.relabelled += 1;
                    }
                    _ => st.unchanged += 1,
                }
            } else {
                st.no_span += 1;
            }
        }
        out.push_str(&v.to_string());
        out.push('\n');
    }
    (out, st)
}

/// The recording of a live session: `<session>/recording.wav` if the stop linked it, else the
/// `recording-<UTC stamp>.wav` in `calls_dir` started closest to the session's own start
/// (within a minute; session folders are named in local time, recordings in UTC).
pub fn find_recording(session: &Path, calls_dir: &Path) -> Option<PathBuf> {
    let linked = session.join(crate::live_log::RECORDING);
    if linked.exists() {
        return Some(linked);
    }
    let name = session.file_name()?.to_str()?;
    let start = chrono::NaiveDateTime::parse_from_str(name.get(..15)?, "%Y%m%d-%H%M%S").ok()?;
    let start = start
        .and_local_timezone(chrono::Local)
        .single()?
        .with_timezone(&chrono::Utc);
    std::fs::read_dir(calls_dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| {
            let stem = p
                .file_stem()?
                .to_str()?
                .strip_prefix("recording-")?
                .to_string();
            let t = chrono::NaiveDateTime::parse_from_str(&stem, "%Y%m%d-%H%M%S").ok()?;
            let gap = (t.and_utc() - start).num_seconds().abs();
            (p.extension()? == "wav" && gap <= 60).then_some((gap, p))
        })
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, p)| p)
}

/// Run the sidecar on `wav`, return its turns.
pub fn diarize_with_sidecar(bin: &Path, wav: &Path, scratch: &Path) -> Result<Vec<Turn>> {
    if !bin.exists() {
        bail!("diarizer sidecar not found at {}", bin.display());
    }
    let out = scratch.join(".diarizer-out.json");
    let _ = std::fs::remove_file(&out);
    // Low priority and the Neural Engine only: the Mac is busy with the recording's own models.
    let status = Command::new("nice")
        .args(["-n", "10"])
        .arg(bin)
        .arg("process")
        .arg(wav)
        .args(["--mode", "offline", "--compute-units", "ane", "--output"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("start the diarizer sidecar")?;
    if !status.success() {
        bail!("diarizer sidecar exited with {status}");
    }
    let json = std::fs::read_to_string(&out).context("sidecar wrote no output")?;
    let _ = std::fs::remove_file(&out);
    Ok(rediarize::parse_turns(&json)?)
}

/// Re-label `session` from `wav`. Writes `stream.jsonl.orig` once, rewrites `stream.jsonl`
/// in place (a hard link kept next to the call sees it), writes `diarization.json`.
pub fn run(session: &Path, wav: &Path, bin: &Path, agent_name: &str) -> Result<(Stats, usize)> {
    let turns = diarize_with_sidecar(bin, wav, session)?;
    if turns.is_empty() {
        bail!("the diarizer found no speech");
    }
    let mut wav_in = voxkit::types::WavRanges::open(wav).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut embedder = voxkit::speaker::SpeakerEmbedder::new(&crate::audio::speaker_model_path())
        .map_err(|e| anyhow::anyhow!("speaker model: {e}"))?;
    let prints = voxkit::speaker::load_voiceprints(&voxkit::paths::voices_dir(), &mut embedder);
    let names = rediarize::name_speakers(
        &turns,
        &mut |a, b| wav_in.read(a, b, 16_000),
        &mut embedder,
        &prints,
        rediarize::NAME_THRESHOLD,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let labels = rediarize::display_labels(&turns, &names);

    let stream = session.join(crate::live_log::SESSION_STREAM);
    let text = std::fs::read_to_string(&stream).context("read stream.jsonl")?;
    let orig = stream.with_extension(format!("jsonl.{ORIG_SUFFIX}"));
    if !orig.exists() {
        std::fs::write(&orig, &text)?;
    }
    let (new, stats) = relabel_stream(&text, &turns, &labels, agent_name);
    // In place, not by rename: `<call>.live.jsonl` is a hard link to this file.
    std::fs::write(&stream, new)?;
    let report = serde_json::json!({
        "diarizer": "fluidaudio-offline",
        "names": names,
        "labels": labels,
        "segments": turns.iter().map(|t| serde_json::json!({
            "speaker": labels.get(&t.speaker).unwrap_or(&t.speaker),
            "start": t.start,
            "end": t.end,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(session.join(REPORT_FILE), serde_json::to_string(&report)?)?;
    Ok((stats, labels.len()))
}

/// `supervox rediarize [session] [--wav FILE]`: `session` is a folder, a folder name under
/// `~/.supervox/live/sessions`, or omitted for the newest one.
pub fn cmd(session: Option<&str>, wav: Option<&str>, agent_name: &str) -> Result<()> {
    let sessions = crate::live_log::live_dir().join(crate::live_log::SESSIONS_DIR);
    let dir = match session {
        Some(s) if Path::new(s).is_dir() => PathBuf::from(s),
        Some(s) => sessions.join(s),
        None => std::fs::read_dir(&sessions)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join(crate::live_log::SESSION_STREAM).exists())
            .max()
            .context("no live sessions")?,
    };
    let calls = supervox_agent::storage::default_calls_dir();
    let wav = match wav {
        Some(w) => PathBuf::from(w),
        None => find_recording(&dir, &calls)
            .with_context(|| format!("no recording found for {}", dir.display()))?,
    };
    let t0 = std::time::Instant::now();
    let (st, speakers) = run(&dir, &wav, &sidecar_path(), agent_name)?;
    println!(
        "{}: {speakers} speakers, {} lines relabelled, {} unchanged, {} without a span (kept), {:.1}s",
        dir.display(),
        st.relabelled,
        st.unchanged,
        st.no_span,
        t0.elapsed().as_secs_f32()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(s: &str, a: f32, b: f32) -> Turn {
        Turn {
            speaker: s.into(),
            start: a,
            end: b,
        }
    }

    fn labels() -> HashMap<String, String> {
        HashMap::from([
            ("x1".to_string(), "S1".to_string()),
            ("x2".to_string(), "Рустам".to_string()),
        ])
    }

    #[test]
    fn lines_with_a_span_take_the_covering_speaker_and_others_pass_through() {
        let stream = [
            r#"{"type":"start","ts":"t"}"#,
            r#"{"type":"transcript","t":99,"speaker":"S1","text":"a","a":0.5,"b":4.0}"#,
            r#"{"type":"transcript","t":120,"speaker":"S3","text":"b","a":10.0,"b":13.0}"#,
            r#"{"type":"transcript","t":130,"speaker":"S2","text":"no span"}"#,
            r#"{"type":"transcript","t":140,"speaker":"Алина","text":"agent","a":20.0,"b":22.0}"#,
            r#"{"type":"heartbeat","ts":"t"}"#,
        ]
        .join("\n");
        let turns = [turn("x1", 0.0, 5.0), turn("x2", 9.0, 14.0)];
        let (out, st) = relabel_stream(&stream, &turns, &labels(), "Алина");
        let rows: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[1]["speaker"], "S1", "already right: untouched");
        assert_eq!(rows[2]["speaker"], "Рустам");
        assert_eq!(rows[3]["speaker"], "S2", "no span: live label kept");
        assert_eq!(rows[4]["speaker"], "Алина", "the agent is not diarized");
        assert_eq!(rows[5]["type"], "heartbeat");
        assert_eq!(
            st,
            Stats {
                relabelled: 1,
                unchanged: 1,
                no_span: 1,
                agent: 1
            }
        );
        // The arrival time, which runs behind the audio, is never used to place a line.
        assert_eq!(rows[2]["t"], 120);
    }

    #[test]
    fn a_line_no_turn_touches_keeps_its_label() {
        let stream = r#"{"type":"transcript","t":1,"speaker":"S4","text":"x","a":50.0,"b":52.0}"#;
        let (out, st) = relabel_stream(stream, &[turn("x1", 0.0, 5.0)], &labels(), "Алина");
        assert!(out.contains(r#""speaker":"S4""#));
        assert_eq!(st.unchanged, 1);
    }

    #[test]
    fn recording_is_found_by_start_time_across_the_utc_offset() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = tmp.path().join("calls");
        std::fs::create_dir(&calls).unwrap();
        // Session named in local time; recording named in UTC, 6 s later.
        let local = chrono::NaiveDateTime::parse_from_str("20261119-205946", "%Y%m%d-%H%M%S")
            .unwrap()
            .and_local_timezone(chrono::Local)
            .single()
            .unwrap();
        let utc = (local + chrono::Duration::seconds(6)).with_timezone(&chrono::Utc);
        let rec = calls.join(format!("recording-{}.wav", utc.format("%Y%m%d-%H%M%S")));
        std::fs::write(&rec, b"x").unwrap();
        std::fs::write(calls.join("recording-20200101-000000.wav"), b"x").unwrap();
        let session = tmp.path().join("20261119-205946");
        std::fs::create_dir(&session).unwrap();
        assert_eq!(find_recording(&session, &calls), Some(rec));
        assert_eq!(
            find_recording(&tmp.path().join("20250101-000000"), &calls),
            None
        );
    }
}
