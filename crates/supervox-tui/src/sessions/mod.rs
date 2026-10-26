//! Every recording supervox has made, in one place: discovery across the three layouts,
//! fuzzy and phrase search, the browser screen and its reader, and `supervox sessions`.

pub mod browser;
pub mod discover;
pub mod reader;
pub mod search;
pub mod transcript;

use discover::SessionEntry;

fn kind_name(e: &SessionEntry) -> &'static str {
    match e.kind {
        discover::Kind::Session => "session",
        discover::Kind::Flat => "flat",
        discover::Kind::Call => "call",
        discover::Kind::Current => "current",
    }
}

fn entry_json(e: &SessionEntry) -> serde_json::Value {
    serde_json::json!({
        "name": e.name,
        "kind": kind_name(e),
        "started": e.started.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        "duration_secs": e.duration_secs,
        "title": e.title,
        "streams": e.streams,
        "summary": e.summary,
        "call": e.call,
        "bytes": e.bytes,
        "live": e.live,
    })
}

/// `supervox sessions --list` / `--json`: every recording, newest first.
pub fn cmd_list(entries: &[SessionEntry], json: bool) -> anyhow::Result<()> {
    if json {
        let all: Vec<_> = entries.iter().map(entry_json).collect();
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    for e in entries {
        let dur = e.duration_secs.map(browser::duration).unwrap_or_default();
        let live = if e.live { " ●" } else { "" };
        println!(
            "{}  {dur:>5}  {:<7} {}{live}  {}",
            e.date_label(),
            kind_name(e),
            e.name,
            e.title.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

/// `supervox sessions --grep <phrase>`: every line with the phrase, newest recording first.
pub fn cmd_grep(entries: &[SessionEntry], phrase: &str, json: bool) -> anyhow::Result<()> {
    let mut index = search::PhraseIndex::new(entries.len());
    for (i, e) in entries.iter().enumerate() {
        index.insert(i, search::load(&search::Source::from(e)));
    }
    let hits = index.search(phrase, usize::MAX);
    let mut rows = Vec::new();
    for h in &hits {
        let (e, doc) = (&entries[h.entry], index.doc(h.entry).expect("indexed"));
        let (t, speaker, text) = if h.in_summary {
            (None, "summary", doc.summary[h.line].as_str())
        } else {
            let u = &doc.lines[h.line];
            (u.t, u.speaker.as_str(), u.text.as_str())
        };
        if json {
            rows.push(serde_json::json!({
                "session": e.name, "t": t, "speaker": speaker, "text": text,
                "in_summary": h.in_summary,
            }));
        } else {
            let at = t
                .map(|t| format!("t={}", transcript::clock(t)))
                .unwrap_or_default();
            println!(
                "{}  {:<28} {at:<9} {speaker}: {text}",
                e.date_label(),
                e.name
            );
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        eprintln!("{} lines in {} recordings", hits.len(), entries.len());
    }
    Ok(())
}

/// Everything under `~/.supervox`.
pub fn discover_all() -> Vec<SessionEntry> {
    discover::discover(
        &crate::live_log::live_dir(),
        &supervox_agent::storage::default_calls_dir(),
    )
}
