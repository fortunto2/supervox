//! Finding a recording: fuzzy over names, dates and titles; a phrase across every
//! transcript and summary.
//!
//! The fuzzy matcher is the one rust-code uses everywhere (`neo_frizbee`, SIMD
//! Smith-Waterman, through the same thin wrapper as rc-cli's `fuzzy.rs`). Phrase search runs
//! over an in-memory index that a background thread fills newest session first, so the
//! browser opens at once and a search covers whatever is loaded so far.

use super::discover::SessionEntry;
use super::transcript::{self, Utterance};
use neo_frizbee::{Config, match_list};
use std::path::PathBuf;
use std::sync::mpsc;

/// rust-code's matcher settings (it allows two typos), scaled to the needle: a typo on a
/// four-letter query already matches "dell" in "Intelligence". None below 5 letters, one
/// up to 9, then two.
fn fuzzy_config(needle: &str) -> Config {
    Config {
        max_typos: Some((needle.chars().count() / 5).min(2) as u16),
        sort: false,
        ..Default::default()
    }
}

/// Long needles can overflow the matcher's u16 score; nobody types a name this long.
const MAX_NEEDLE: usize = 64;

/// Entry indices that match `query`, best first; equal scores keep the newest first.
/// An empty query lists everything in discovery order (newest first).
pub fn fuzzy_rank(query: &str, entries: &[SessionEntry]) -> Vec<usize> {
    let needle: String = query.trim().chars().take(MAX_NEEDLE).collect();
    if needle.is_empty() {
        return (0..entries.len()).collect();
    }
    let hay: Vec<String> = entries.iter().map(SessionEntry::haystack).collect();
    let refs: Vec<&str> = hay.iter().map(String::as_str).collect();
    let mut matches = match_list(&needle, &refs, &fuzzy_config(&needle));
    matches.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    matches.into_iter().map(|m| m.index as usize).collect()
}

/// A recording's searchable text.
#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub lines: Vec<Utterance>,
    pub summary: Vec<String>,
    lower: Vec<String>,
    summary_lower: Vec<String>,
}

impl Doc {
    pub fn new(lines: Vec<Utterance>, summary: &str) -> Self {
        let summary: Vec<String> = summary.lines().map(String::from).collect();
        Self {
            lower: lines.iter().map(|l| l.text.to_lowercase()).collect(),
            summary_lower: summary.iter().map(|l| l.to_lowercase()).collect(),
            lines,
            summary,
        }
    }

    /// Transcript line indices containing `needle` (already lowercase).
    pub fn matching_lines(&self, needle: &str) -> Vec<usize> {
        matching(&self.lower, needle)
    }

    pub fn matching_summary(&self, needle: &str) -> Vec<usize> {
        matching(&self.summary_lower, needle)
    }
}

fn matching(lower: &[String], needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    lower
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i)
        .collect()
}

/// Everything a recording's text is read from.
#[derive(Debug, Clone)]
pub struct Source {
    pub streams: Vec<PathBuf>,
    pub call: Option<PathBuf>,
    pub summary: Option<PathBuf>,
}

impl From<&SessionEntry> for Source {
    fn from(e: &SessionEntry) -> Self {
        Self {
            streams: e.streams.clone(),
            call: e.call.clone(),
            summary: e.summary.clone(),
        }
    }
}

/// Read a recording's transcript and summary from disk.
pub fn load(src: &Source) -> Doc {
    let lines = match &src.call {
        Some(call) => transcript::load_call(call),
        None => transcript::load_streams(&src.streams),
    };
    let summary = src
        .summary
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    Doc::new(lines, &summary)
}

/// Load `sources` (entry index, source) on a thread, in order, one message per recording.
pub fn spawn_loader(sources: Vec<(usize, Source)>) -> mpsc::Receiver<(usize, Doc)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (i, src) in sources {
            if tx.send((i, load(&src))).is_err() {
                break; // the browser closed
            }
        }
    });
    rx
}

/// Where a phrase was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub entry: usize,
    pub line: usize,
    /// In the summary rather than the transcript.
    pub in_summary: bool,
}

/// The loaded text of every recording, by entry index.
#[derive(Debug, Default)]
pub struct PhraseIndex {
    docs: Vec<Option<Doc>>,
    loaded: usize,
}

impl PhraseIndex {
    pub fn new(entries: usize) -> Self {
        Self {
            docs: (0..entries).map(|_| None).collect(),
            loaded: 0,
        }
    }

    pub fn insert(&mut self, entry: usize, doc: Doc) {
        if let Some(slot) = self.docs.get_mut(entry) {
            if slot.is_none() {
                self.loaded += 1;
            }
            *slot = Some(doc);
        }
    }

    pub fn doc(&self, entry: usize) -> Option<&Doc> {
        self.docs.get(entry).and_then(Option::as_ref)
    }

    pub fn loaded(&self) -> usize {
        self.loaded
    }

    /// Recordings the index is for, loaded or not.
    pub fn size(&self) -> usize {
        self.docs.len()
    }

    /// Lines containing `query`, ignoring case: newest recording first, transcript lines in
    /// time order, then its summary lines. At most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let needle = query.trim().to_lowercase();
        let mut hits = Vec::new();
        if needle.is_empty() {
            return hits;
        }
        for (entry, doc) in self.docs.iter().enumerate() {
            let Some(doc) = doc else { continue };
            let spoken = doc.matching_lines(&needle).into_iter().map(|line| Hit {
                entry,
                line,
                in_summary: false,
            });
            let noted = doc.matching_summary(&needle).into_iter().map(|line| Hit {
                entry,
                line,
                in_summary: true,
            });
            for hit in spoken.chain(noted) {
                if hits.len() >= limit {
                    return hits;
                }
                hits.push(hit);
            }
        }
        hits
    }
}

/// Byte ranges of `text` where `needle` (lowercase) occurs, ignoring case. Works on the
/// original text, so highlights land right even where lowercasing changes byte lengths.
pub fn find_ci(text: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    let pat: Vec<char> = needle.chars().collect();
    if pat.is_empty() {
        return Vec::new();
    }
    // (byte offset of the original char, one lowercase char)
    let low: Vec<(usize, char)> = text
        .char_indices()
        .flat_map(|(i, c)| c.to_lowercase().map(move |l| (i, l)))
        .collect();
    let char_end = |i: usize| {
        text[i..]
            .chars()
            .next()
            .map_or(text.len(), |c| i + c.len_utf8())
    };
    let mut out = Vec::new();
    let mut k = 0;
    while k + pat.len() <= low.len() {
        if low[k..k + pat.len()]
            .iter()
            .map(|(_, c)| *c)
            .eq(pat.iter().copied())
        {
            let start = low[k].0;
            let end = char_end(low[k + pat.len() - 1].0);
            out.push(start..end);
            k += pat.len();
        } else {
            k += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::discover::Kind;
    use super::*;

    fn entry(name: &str, title: Option<&str>, started: &str) -> SessionEntry {
        SessionEntry {
            name: name.into(),
            kind: Kind::Session,
            started: chrono::NaiveDateTime::parse_from_str(started, "%Y-%m-%d %H:%M").ok(),
            duration_secs: None,
            title: title.map(String::from),
            streams: vec![],
            summary: None,
            call: None,
            bytes: 0,
            live: false,
        }
    }

    fn sample() -> Vec<SessionEntry> {
        vec![
            entry(
                "20261025-182225-dell-nvidia",
                Some("Dell × NVIDIA AI Hackathon — Top 8 demos"),
                "2026-10-25 18:22",
            ),
            entry(
                "20261025-163736",
                Some("Sergey Levine · Physical Intelligence"),
                "2026-10-25 16:37",
            ),
            entry("20261024-01a16c3c", None, "2026-10-24 21:48"),
            entry(
                "20261020-120000",
                Some("Weekly sync with Dell sales"),
                "2026-10-20 12:00",
            ),
        ]
    }

    #[test]
    fn empty_query_lists_everything_newest_first() {
        assert_eq!(fuzzy_rank("", &sample()), [0, 1, 2, 3]);
        assert_eq!(fuzzy_rank("   ", &sample()), [0, 1, 2, 3]);
    }

    #[test]
    fn fuzzy_finds_by_title_name_and_date() {
        let e = sample();
        assert_eq!(fuzzy_rank("levine", &e).first(), Some(&1));
        assert_eq!(fuzzy_rank("nvidia", &e).first(), Some(&0));
        assert_eq!(fuzzy_rank("10-24", &e).first(), Some(&2));
        // subsequence, the fuzzy part
        assert_eq!(fuzzy_rank("phys intel", &e).first(), Some(&1));
        assert!(fuzzy_rank("zzzzqqq", &e).is_empty());
    }

    #[test]
    fn equal_matches_keep_the_newest_first() {
        let e = sample();
        let dell = fuzzy_rank("dell", &e);
        assert_eq!(&dell[..2], [0, 3]);
    }

    #[test]
    fn a_typo_is_forgiven_only_in_a_longer_query() {
        let e = sample();
        assert_eq!(
            fuzzy_rank("levnie", &e).first(),
            Some(&1),
            "6 letters: one typo"
        );
        assert!(!fuzzy_rank("lxv", &e).contains(&1), "3 letters: exact");
    }

    #[test]
    fn a_huge_query_does_not_panic_the_matcher() {
        let long = "a".repeat(5000);
        let _ = fuzzy_rank(&long, &sample());
    }

    fn said(t: u64, text: &str) -> Utterance {
        Utterance {
            t: Some(t),
            speaker: "S1".into(),
            text: text.into(),
        }
    }

    fn index() -> PhraseIndex {
        let mut idx = PhraseIndex::new(3);
        idx.insert(
            0,
            Doc::new(
                vec![
                    said(5, "GB10 has 128 GB"),
                    said(9, "nothing"),
                    said(12, "gb10 again"),
                ],
                "# Dell\n- GB10: 121 GB free\n",
            ),
        );
        // entry 1 not loaded yet
        idx.insert(2, Doc::new(vec![said(1, "Ещё про GB10 и Привет")], ""));
        idx
    }

    #[test]
    fn phrase_hits_come_newest_session_first_then_time_then_summary() {
        let hits = index().search("gb10", 100);
        let at: Vec<(usize, usize, bool)> = hits
            .iter()
            .map(|h| (h.entry, h.line, h.in_summary))
            .collect();
        assert_eq!(
            at,
            [(0, 0, false), (0, 2, false), (0, 1, true), (2, 0, false)]
        );
    }

    #[test]
    fn phrase_search_ignores_case_in_cyrillic_and_respects_the_limit() {
        let idx = index();
        assert_eq!(idx.search("ПРИВЕТ", 10).len(), 1);
        assert_eq!(idx.search("gb10", 2).len(), 2);
        assert!(idx.search("", 10).is_empty());
        assert!(
            idx.search("128 gb free", 10).is_empty(),
            "a phrase, not a bag of words"
        );
        assert_eq!(idx.loaded(), 2);
        assert_eq!(idx.size(), 3);
    }

    #[test]
    fn the_loader_fills_the_index_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let stream = dir.path().join("stream.jsonl");
        std::fs::write(
            &stream,
            format!(
                "{}\n",
                serde_json::json!({"type": "transcript", "t": 7, "speaker": "S1", "text": "loaded"})
            ),
        )
        .unwrap();
        let summary = dir.path().join("summary.md");
        std::fs::write(&summary, "# Title\nline\n").unwrap();
        let src = Source {
            streams: vec![stream],
            call: None,
            summary: Some(summary),
        };
        let rx = spawn_loader(vec![(4, src.clone()), (1, src)]);
        let got: Vec<(usize, Doc)> = rx.iter().collect();
        assert_eq!(got.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [4, 1]);
        assert_eq!(got[0].1.lines[0].text, "loaded");
        assert_eq!(got[0].1.summary, ["# Title", "line"]);
    }

    #[test]
    fn find_ci_returns_ranges_in_the_original_text() {
        let text = "Ещё про GB10 и gb10";
        let ranges = find_ci(text, "gb10");
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].clone()], "GB10");
        assert_eq!(&text[ranges[1].clone()], "gb10");
        assert_eq!(&text[find_ci(text, "ещё")[0].clone()], "Ещё");
        // 'İ' lowercases to two chars; ranges still cut on char boundaries
        let tricky = "İstanbul and istanbul";
        for r in find_ci(tricky, "stanbul") {
            assert_eq!(&tricky[r], "stanbul");
        }
        assert!(find_ci("abc", "").is_empty());
    }
}
