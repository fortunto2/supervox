//! A dataset of recurring voices built from live sessions, local only.
//!
//! A session folder keeps `embeddings.jsonl`: every final segment's raw speaker embedding with
//! the label the pipeline showed (`S<n>`, or a voiceprint's name), named or not. With
//! `speakers.json` (cluster -> name) the segments of a named cluster are exported to
//! `~/.supervox/voices/dataset/<name>/<session-id>/embeddings.jsonl`. `rebuild` turns a
//! person's dataset into `~/.supervox/voices/<name>.emb2`, which the next call loads as a voiceprint.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const EMBEDDINGS_FILE: &str = "embeddings.jsonl";
/// Fewest segments a voiceprint is built from.
pub const MIN_SEGMENTS: usize = 20;
/// Cosine to the median below which a segment is an outlier.
pub const OUTLIER_COS: f32 = 0.5;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Segment {
    /// The label shown when the segment was spoken: `S<n>` or a voiceprint's name.
    pub label: String,
    pub a: Option<f32>,
    pub b: Option<f32>,
    pub emb: Vec<f32>,
}

pub fn dataset_dir() -> PathBuf {
    voxkit::paths::voices_dir().join("dataset")
}

/// Append one segment's embedding to the session folder. Silent on IO errors, like the stream.
pub fn record(session: &Path, label: &str, span: Option<(f32, f32)>, emb: &[f32]) {
    let seg = Segment {
        label: label.to_string(),
        a: span.map(|s| s.0),
        b: span.map(|s| s.1),
        emb: emb.to_vec(),
    };
    let Ok(line) = serde_json::to_string(&seg) else {
        return;
    };
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(session.join(EMBEDDINGS_FILE))
    {
        let _ = writeln!(f, "{line}");
    }
}

fn read_segments(path: &Path) -> Vec<Segment> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn is_cluster(label: &str) -> bool {
    label
        .strip_prefix('S')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `speakers.json` of a session folder (empty when absent or invalid).
pub fn read_speakers(session: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(session.join(crate::live_log::SPEAKERS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Who a segment belongs to: the agent's name for its cluster, or the voiceprint's own name.
pub fn name_of<'a>(label: &'a str, speakers: &'a BTreeMap<String, String>) -> Option<&'a str> {
    match speakers
        .get(label)
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
    {
        Some(n) => Some(n),
        None if is_cluster(label) => None,
        None => Some(label),
    }
}

/// Export a session's named segments into the dataset (replacing what an earlier sync put
/// there) and keep the session's `speakers.json`: a `live/speakers.json` the agent wrote is
/// merged in and removed, so it cannot name the next session's clusters. Returns segments per name.
pub fn sync_session(session: &Path, live: &Path, dataset: &Path) -> BTreeMap<String, usize> {
    let mut speakers = BTreeMap::new();
    for dir in [live, session] {
        let text = fs::read_to_string(dir.join(crate::live_log::SPEAKERS_FILE)).unwrap_or_default();
        if let Ok(m) = serde_json::from_str::<BTreeMap<String, String>>(&text) {
            speakers.extend(m);
        }
    }
    if !speakers.is_empty()
        && let Ok(text) = serde_json::to_string_pretty(&speakers)
    {
        let _ = fs::write(session.join(crate::live_log::SPEAKERS_FILE), text);
    }
    let _ = fs::remove_file(live.join(crate::live_log::SPEAKERS_FILE));
    let Some(id) = session.file_name() else {
        return BTreeMap::new();
    };
    if let Ok(rd) = fs::read_dir(dataset) {
        for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
            let _ = fs::remove_dir_all(p.join(id));
        }
    }
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for seg in read_segments(&session.join(EMBEDDINGS_FILE)) {
        if let Some(name) = name_of(&seg.label, &speakers)
            && let Ok(line) = serde_json::to_string(&seg)
        {
            by_name.entry(name.to_string()).or_default().push(line);
        }
    }
    let mut counts = BTreeMap::new();
    for (name, lines) in by_name {
        let dir = dataset.join(&name).join(id);
        if fs::create_dir_all(&dir).is_ok()
            && fs::write(dir.join(EMBEDDINGS_FILE), lines.join("\n") + "\n").is_ok()
        {
            counts.insert(name, lines.len());
        }
    }
    counts
}

/// `supervox voices name <session> S3 <name>`: merge into the session's `speakers.json`, then sync.
pub fn name_cluster(session: &Path, label: &str, name: &str, dataset: &Path) -> Result<usize> {
    if !is_cluster(label) {
        bail!("{label} is not a cluster label (S1, S2, ...)");
    }
    let mut speakers = read_speakers(session);
    speakers.insert(label.to_string(), name.trim().to_string());
    fs::write(
        session.join(crate::live_log::SPEAKERS_FILE),
        serde_json::to_string_pretty(&speakers)?,
    )?;
    // The live dir is not involved: a retro-label is about a finished session
    let none = session.join(".no-live");
    let counts = sync_session(session, &none, dataset);
    Ok(counts.get(name.trim()).copied().unwrap_or(0))
}

fn unit(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n == 0.0 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / n).collect()
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Unit mean of the embeddings after dropping those whose cosine to the component-wise
/// median is under `cutoff`. Returns `(mean, kept, dropped)`.
pub fn robust_mean(embs: &[Vec<f32>], cutoff: f32) -> Option<(Vec<f32>, usize, usize)> {
    let units: Vec<Vec<f32>> = embs.iter().map(|e| unit(e)).collect();
    let dim = units.first()?.len();
    if units.iter().any(|u| u.len() != dim) {
        return None;
    }
    let med = unit(
        &(0..dim)
            .map(|i| median(units.iter().map(|u| u[i]).collect()))
            .collect::<Vec<_>>(),
    );
    let kept: Vec<&Vec<f32>> = units
        .iter()
        .filter(|u| u.iter().zip(&med).map(|(a, b)| a * b).sum::<f32>() >= cutoff)
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut mean = vec![0.0f32; dim];
    for u in &kept {
        for (m, x) in mean.iter_mut().zip(u.iter()) {
            *m += x;
        }
    }
    Some((unit(&mean), kept.len(), units.len() - kept.len()))
}

fn name_segments(name: &str, dataset: &Path) -> (Vec<Vec<f32>>, usize) {
    let mut embs = Vec::new();
    let mut sessions = 0;
    let Ok(rd) = fs::read_dir(dataset.join(name)) else {
        return (embs, 0);
    };
    for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
        let segs = read_segments(&p.join(EMBEDDINGS_FILE));
        if !segs.is_empty() {
            sessions += 1;
            embs.extend(segs.into_iter().map(|s| s.emb));
        }
    }
    (embs, sessions)
}

/// Recompute `<voices>/<name>.emb2` from the person's whole dataset. Returns `(kept, dropped)`.
pub fn rebuild(name: &str, dataset: &Path, voices: &Path, min: usize) -> Result<(usize, usize)> {
    let (embs, _) = name_segments(name, dataset);
    if embs.len() < min {
        bail!(
            "{name}: {} segments in the dataset, {min} needed (name more clusters, or lower --min)",
            embs.len()
        );
    }
    let (mean, kept, dropped) = robust_mean(&embs, OUTLIER_COS).context("no usable embeddings")?;
    if kept < min {
        bail!(
            "{name}: only {kept} of {} segments agree, {min} needed",
            embs.len()
        );
    }
    let bytes: Vec<u8> = mean.iter().flat_map(|x| x.to_le_bytes()).collect();
    fs::create_dir_all(voices)?;
    fs::write(voices.join(format!("{name}.emb2")), bytes)?;
    Ok((kept, dropped))
}

/// `(name, segments, sessions, has voiceprint)` for every person in the dataset.
pub fn list(dataset: &Path, voices: &Path) -> Vec<(String, usize, usize, bool)> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dataset) else {
        return out;
    };
    for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
        let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        let (embs, sessions) = name_segments(&name, dataset);
        let has = voices.join(format!("{name}.emb2")).exists();
        out.push((name, embs.len(), sessions, has));
    }
    out.sort();
    out
}

/// A session folder from a path, a folder name under `live/sessions`, or a unique prefix of one.
pub fn find_session(arg: &str) -> Result<PathBuf> {
    if Path::new(arg).is_dir() {
        return Ok(PathBuf::from(arg));
    }
    let sessions = crate::live_log::live_dir().join(crate::live_log::SESSIONS_DIR);
    let exact = sessions.join(arg);
    if exact.is_dir() {
        return Ok(exact);
    }
    let hits: Vec<PathBuf> = fs::read_dir(&sessions)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(arg))
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("no session {arg}"),
        _ => bail!("{arg} matches {} sessions", hits.len()),
    }
}

pub enum Cmd {
    Name {
        session: String,
        label: String,
        name: String,
    },
    Rebuild {
        name: String,
        min: usize,
    },
    List,
}

pub fn cmd(c: Cmd) -> Result<()> {
    let (dataset, voices) = (dataset_dir(), voxkit::paths::voices_dir());
    match c {
        Cmd::Name {
            session,
            label,
            name,
        } => {
            let dir = find_session(&session)?;
            let n = name_cluster(&dir, &label, &name, &dataset)?;
            println!(
                "{label} -> {name}: {n} segments of {} in the dataset",
                dir.display()
            );
        }
        Cmd::Rebuild { name, min } => {
            let (kept, dropped) = rebuild(&name, &dataset, &voices, min)?;
            println!("{name}: voiceprint from {kept} segments ({dropped} outliers dropped)");
        }
        Cmd::List => {
            let mut people: HashMap<String, (usize, usize, bool)> = HashMap::new();
            for (n, segs, sessions, has) in list(&dataset, &voices) {
                people.insert(n, (segs, sessions, has));
            }
            let mut names: Vec<_> = people.keys().cloned().collect();
            names.sort();
            for n in names {
                let (segs, sessions, has) = people[&n];
                println!(
                    "{n}: {segs} segments, {sessions} sessions, {}",
                    if has {
                        "voiceprint"
                    } else {
                        "no voiceprint yet"
                    }
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(base: &[f32], bump: f32, i: usize) -> Vec<f32> {
        let mut v = base.to_vec();
        v[i % base.len()] += bump;
        v
    }

    fn voice() -> Vec<f32> {
        vec![1.0, 0.8, 0.6, 0.4, 0.2, 0.1, 0.0, 0.3]
    }

    #[test]
    fn rebuild_drops_outliers_and_points_at_the_voice() {
        let tmp = tempfile::tempdir().unwrap();
        let (dataset, voices) = (tmp.path().join("ds"), tmp.path().join("voices"));
        let dir = dataset.join("Anna").join("s1");
        fs::create_dir_all(&dir).unwrap();
        let mut lines = String::new();
        for i in 0..25 {
            let seg = Segment {
                label: "S2".into(),
                a: None,
                b: None,
                emb: near(&voice(), 0.05, i),
            };
            lines += &(serde_json::to_string(&seg).unwrap() + "\n");
        }
        for _ in 0..3 {
            let other = vec![0.0, 0.1, 0.0, -0.5, 1.0, -0.9, 0.8, 0.0];
            let seg = Segment {
                label: "S2".into(),
                a: None,
                b: None,
                emb: other,
            };
            lines += &(serde_json::to_string(&seg).unwrap() + "\n");
        }
        fs::write(dir.join(EMBEDDINGS_FILE), lines).unwrap();

        let (kept, dropped) = rebuild("Anna", &dataset, &voices, 20).unwrap();
        assert_eq!((kept, dropped), (25, 3));
        let bytes = fs::read(voices.join("Anna.emb2")).unwrap();
        let emb: Vec<f32> = bytes
            .chunks(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let cos: f32 = unit(&emb)
            .iter()
            .zip(unit(&voice()))
            .map(|(a, b)| a * b)
            .sum();
        assert!(cos > 0.99, "{cos}");
        assert!(rebuild("Anna", &dataset, &voices, 40).is_err());
    }

    #[test]
    fn a_named_cluster_lands_in_the_dataset_and_a_retro_label_adds_another() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp.path().join("sessions/20261120-100000");
        let (live, dataset) = (tmp.path().join("live"), tmp.path().join("ds"));
        fs::create_dir_all(&session).unwrap();
        fs::create_dir_all(&live).unwrap();
        record(&session, "S2", Some((1.0, 2.0)), &voice());
        record(&session, "S3", None, &voice());
        record(&session, "S1", None, &voice());
        record(&session, "Bob", None, &voice()); // matched a voiceprint
        fs::write(
            live.join(crate::live_log::SPEAKERS_FILE),
            r#"{"S2":"Anna"}"#,
        )
        .unwrap();

        let counts = sync_session(&session, &live, &dataset);
        assert_eq!(counts.get("Anna"), Some(&1));
        assert_eq!(counts.get("Bob"), Some(&1));
        assert!(!counts.contains_key("S1"));
        // the agent's loose file moved into the session, so the next one starts clean
        assert!(!live.join(crate::live_log::SPEAKERS_FILE).exists());
        assert_eq!(
            read_speakers(&session).get("S2").map(String::as_str),
            Some("Anna")
        );

        assert_eq!(name_cluster(&session, "S3", "Anna", &dataset).unwrap(), 2);
        assert!(name_cluster(&session, "Bob", "X", &dataset).is_err());
        assert_eq!(list(&dataset, &tmp.path().join("v"))[0].1, 2);
    }
}
