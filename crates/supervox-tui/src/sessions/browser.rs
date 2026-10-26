//! The sessions browser: every recording in one list, filtered fuzzy by name, date and
//! title, or searched by phrase across transcripts and summaries, with a preview of the
//! selection. Enter opens the reader; Tab switches between the two searches (rust-code's
//! session search toggles "by session" / "by message" the same way).

use super::discover::{Kind, SessionEntry, discover};
use super::reader::{
    Reader, ReaderAction, highlighted, speaker_color, summary_line, utterance_line,
};
use super::search::{Doc, Hit, PhraseIndex, Source, fuzzy_rank, load, spawn_loader};
use super::transcript::clock;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use std::sync::mpsc::{Receiver, TryRecvError};

/// More hits than anyone reads; keeps a one-letter query cheap.
const HIT_LIMIT: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Fuzzy over names, dates and titles.
    Fuzzy,
    /// A phrase in transcripts and summaries.
    Phrase,
}

/// What the app should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAction {
    None,
    /// Leave the browser.
    Back,
    Help,
}

pub struct Browser {
    entries: Vec<SessionEntry>,
    index: PhraseIndex,
    loader: Option<Receiver<(usize, Doc)>>,
    mode: SearchMode,
    query: String,
    results: Vec<usize>,
    hits: Vec<Hit>,
    list: ListState,
    list_rect: Rect,
    reader: Option<Reader>,
    /// Summary lines of an entry not loaded yet, for the preview.
    preview: Option<(usize, Vec<String>)>,
}

/// `42m`, `1h02`, `<1m`.
pub fn duration(secs: u64) -> String {
    match secs {
        0..60 => "<1m".into(),
        60..3600 => format!("{}m", secs / 60),
        _ => format!("{}h{:02}", secs / 3600, (secs / 60) % 60),
    }
}

/// `text` cut to `width` chars around the first match of `needle`, with `…` where cut.
pub fn snippet(text: &str, needle: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width || width < 8 {
        return text.to_string();
    }
    let at = super::search::find_ci(text, needle)
        .first()
        .map_or(0, |r| text[..r.start].chars().count());
    let start = at
        .saturating_sub(width / 3)
        .min(chars.len().saturating_sub(width));
    let mut cut: Vec<char> = chars[start..start + width].to_vec();
    if start > 0 {
        cut[0] = '…';
    }
    if start + width < chars.len() {
        cut[width - 1] = '…';
    }
    cut.into_iter().collect()
}

fn owned(spans: Vec<Span<'_>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|s| Span::styled(s.content.into_owned(), s.style))
        .collect()
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

impl Browser {
    /// Everything under `~/.supervox`.
    pub fn open() -> Self {
        Self::new(discover(
            &crate::live_log::live_dir(),
            &supervox_agent::storage::default_calls_dir(),
        ))
    }

    pub fn new(entries: Vec<SessionEntry>) -> Self {
        let sources = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, Source::from(e)))
            .collect();
        let mut b = Self {
            index: PhraseIndex::new(entries.len()),
            loader: Some(spawn_loader(sources)),
            entries,
            mode: SearchMode::Fuzzy,
            query: String::new(),
            results: Vec::new(),
            hits: Vec::new(),
            list: ListState::default(),
            list_rect: Rect::default(),
            reader: None,
            preview: None,
        };
        b.refresh();
        b
    }

    pub fn is_loading(&self) -> bool {
        self.loader.is_some()
    }

    #[cfg(test)]
    pub fn is_reading(&self) -> bool {
        self.reader.is_some()
    }

    /// Take what the loader finished. Phrase hits are re-run when new text arrived.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.loader else {
            return false;
        };
        let mut got = false;
        loop {
            match rx.try_recv() {
                Ok((i, doc)) => {
                    self.index.insert(i, doc);
                    got = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.loader = None;
                    break;
                }
            }
        }
        if got && self.mode == SearchMode::Phrase && !self.query.trim().is_empty() {
            let keep = self.list.selected();
            self.hits = self.index.search(&self.query, HIT_LIMIT);
            self.select(keep.unwrap_or(0));
        }
        got
    }

    fn count(&self) -> usize {
        match self.mode {
            SearchMode::Fuzzy => self.results.len(),
            SearchMode::Phrase => self.hits.len(),
        }
    }

    fn select(&mut self, i: usize) {
        let n = self.count();
        self.list.select((n > 0).then(|| i.min(n - 1)));
    }

    fn move_by(&mut self, delta: isize) {
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.select((cur + delta).max(0) as usize);
    }

    fn refresh(&mut self) {
        match self.mode {
            SearchMode::Fuzzy => self.results = fuzzy_rank(&self.query, &self.entries),
            SearchMode::Phrase => self.hits = self.index.search(&self.query, HIT_LIMIT),
        }
        self.select(0);
    }

    fn page(&self) -> isize {
        (self.list_rect.height.saturating_sub(2).max(1)) as isize
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> BrowserAction {
        if let Some(reader) = &mut self.reader {
            match reader.handle_key(key) {
                ReaderAction::Close => self.reader = None,
                ReaderAction::Help => return BrowserAction::Help,
                ReaderAction::None => {}
            }
            return BrowserAction::None;
        }
        self.handle_list_key(key)
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> BrowserAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return BrowserAction::Back,
            KeyCode::Enter => self.open_selected(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.mode = match self.mode {
                    SearchMode::Fuzzy => SearchMode::Phrase,
                    SearchMode::Phrase => SearchMode::Fuzzy,
                };
                self.refresh();
            }
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Char('p') if ctrl => self.move_by(-1),
            KeyCode::Char('n') if ctrl => self.move_by(1),
            KeyCode::PageUp => self.move_by(-self.page()),
            KeyCode::PageDown => self.move_by(self.page()),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(usize::MAX),
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refresh();
            }
            KeyCode::Char('w') if ctrl => {
                let kept = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(kept);
                self.refresh();
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refresh();
            }
            KeyCode::Char('?') if self.query.is_empty() => return BrowserAction::Help,
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                self.refresh();
            }
            _ => {}
        }
        BrowserAction::None
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if let Some(reader) = &mut self.reader {
            reader.handle_mouse(mouse);
            return;
        }
        let inside = sgr_agent_tui::point_in_rect(mouse.column, mouse.row, self.list_rect);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_by(-(crate::scroll::WHEEL_ROWS as isize)),
            MouseEventKind::ScrollDown => self.move_by(crate::scroll::WHEEL_ROWS as isize),
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let row = mouse.row.saturating_sub(self.list_rect.y + 1) as usize;
                self.select(self.list.offset() + row);
            }
            _ => {}
        }
    }

    /// The selected entry and, in phrase mode, the hit.
    fn selected(&self) -> Option<(usize, Option<Hit>)> {
        let i = self.list.selected()?;
        match self.mode {
            SearchMode::Fuzzy => self.results.get(i).map(|e| (*e, None)),
            SearchMode::Phrase => self.hits.get(i).map(|h| (h.entry, Some(*h))),
        }
    }

    fn open_selected(&mut self) {
        let Some((i, hit)) = self.selected() else {
            return;
        };
        let entry = self.entries[i].clone();
        // A recording in progress is read fresh; the rest come from the index if loaded.
        let doc = match self.index.doc(i).filter(|_| !entry.live) {
            Some(doc) => doc.clone(),
            None => load(&Source::from(&entry)),
        };
        let phrase = match self.mode {
            SearchMode::Phrase => self.query.as_str(),
            SearchMode::Fuzzy => "",
        };
        let at = hit.map(|h| (h.line, h.in_summary));
        self.reader = Some(Reader::new(entry, doc, phrase, at));
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        if let Some(reader) = &mut self.reader {
            reader.render(f, area);
            return;
        }
        let [query, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(area);
        let [list, preview] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(body);
        self.list_rect = list;
        self.render_query(f, query);
        self.render_list(f, list);
        self.render_preview(f, preview);
        self.render_footer(f, footer);
    }

    fn render_query(&self, f: &mut Frame, area: Rect) {
        let tab = |m: SearchMode, name: &str| {
            if self.mode == m {
                Span::styled(
                    format!("[{name}]"),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(name.to_string(), dim())
            }
        };
        let title = Line::from(vec![
            Span::raw(" Sessions  "),
            tab(SearchMode::Fuzzy, "name/date/title"),
            Span::raw(" "),
            tab(SearchMode::Phrase, "phrase in transcripts"),
            Span::styled("  Tab switches ", dim()),
        ]);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title);
        let text = Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(self.query.as_str()),
            Span::styled("█", Style::default().fg(Color::Cyan)),
        ]);
        f.render_widget(Paragraph::new(text).block(block), area);
    }

    fn entry_row(&self, e: &SessionEntry) -> ListItem<'static> {
        let (mark, color) = match (e.live, e.kind) {
            (true, _) | (_, Kind::Current) => ("●", Color::Red),
            (_, Kind::Call) => ("c", Color::DarkGray),
            (_, Kind::Flat) => ("·", Color::DarkGray),
            (_, Kind::Session) => (" ", Color::DarkGray),
        };
        let dur = e.duration_secs.map(duration).unwrap_or_default();
        let label = Span::styled(
            e.label().to_string(),
            if e.title.is_some() {
                Style::default().fg(Color::White)
            } else {
                dim()
            },
        );
        ListItem::new(Line::from(vec![
            Span::styled(format!("{mark} "), Style::default().fg(color)),
            Span::styled(e.date_label(), Style::default().fg(Color::Cyan)),
            Span::styled(format!(" {dur:>5}  "), dim()),
            label,
        ]))
    }

    fn hit_row(&self, h: &Hit, width: usize) -> ListItem<'static> {
        let e = &self.entries[h.entry];
        let mut spans = vec![Span::styled(
            format!("{} ", e.date_label()),
            Style::default().fg(Color::Cyan),
        )];
        let Some(doc) = self.index.doc(h.entry) else {
            return ListItem::new(Line::from(spans));
        };
        let needle = self.query.trim().to_lowercase();
        let text = if h.in_summary {
            spans.push(Span::styled("summary ", Style::default().fg(Color::Yellow)));
            doc.summary[h.line].trim().to_string()
        } else {
            let u = &doc.lines[h.line];
            let at = u.t.map(|t| format!("t={}", clock(t))).unwrap_or_default();
            spans.push(Span::styled(format!("{at:<8}"), dim()));
            if !u.speaker.is_empty() {
                spans.push(Span::styled(
                    format!("{}: ", u.speaker),
                    Style::default().fg(speaker_color(&u.speaker)),
                ));
            }
            u.text.clone()
        };
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let cut = snippet(&text, &needle, width.saturating_sub(used + 3));
        spans.extend(owned(highlighted(&cut, &needle, Style::default())));
        ListItem::new(Line::from(spans))
    }

    /// First row on screen, moved just enough to keep the selection visible.
    fn scroll_list(&mut self, height: usize) -> usize {
        let mut offset = self.list.offset();
        if let Some(sel) = self.list.selected() {
            if sel < offset {
                offset = sel;
            } else if sel >= offset + height {
                offset = sel + 1 - height;
            }
        }
        *self.list.offset_mut() = offset;
        offset
    }

    /// Only the rows on screen are built: 2000 phrase hits would otherwise be cut and
    /// highlighted 20 times a second.
    fn render_list(&mut self, f: &mut Frame, area: Rect) {
        let width = area.width.saturating_sub(2) as usize;
        let height = area.height.saturating_sub(2).max(1) as usize;
        let from = self.scroll_list(height);
        let to = (from + height).min(self.count());
        let items: Vec<ListItem> = (from..to)
            .map(|row| match self.mode {
                SearchMode::Fuzzy => self.entry_row(&self.entries[self.results[row]]),
                SearchMode::Phrase => self.hit_row(&self.hits[row], width),
            })
            .collect();
        let mut window = ListState::default().with_selected(self.list.selected().map(|s| s - from));
        let title = match self.mode {
            SearchMode::Fuzzy => {
                format!(" {}/{} recordings ", self.results.len(), self.entries.len())
            }
            SearchMode::Phrase if self.query.trim().is_empty() => " type a phrase ".into(),
            SearchMode::Phrase => {
                let more = if self.hits.len() >= HIT_LIMIT {
                    "+"
                } else {
                    ""
                };
                format!(" {}{more} lines ", self.hits.len())
            }
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        f.render_stateful_widget(list, area, &mut window);
    }

    /// Summary of an entry the loader has not reached: read once, kept for redraws.
    fn summary_preview(&mut self, i: usize) -> Vec<String> {
        if let Some(doc) = self.index.doc(i) {
            return doc.summary.clone();
        }
        if let Some((cached, lines)) = &self.preview
            && *cached == i
        {
            return lines.clone();
        }
        let lines: Vec<String> = self.entries[i]
            .summary
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default();
        self.preview = Some((i, lines.clone()));
        lines
    }

    fn render_preview(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(dim())
            .title(" Preview · Enter opens ");
        let Some((i, hit)) = self.selected() else {
            f.render_widget(Paragraph::new("").block(block), area);
            return;
        };
        let needle = self.query.trim().to_lowercase();
        let lines: Vec<Line> = match (hit, self.index.doc(i)) {
            (Some(h), Some(doc)) => context(doc, h, &needle, area.height as usize),
            _ => {
                let mut lines = self.summary_preview(i);
                if lines.is_empty()
                    && let Some(doc) = self.index.doc(i)
                {
                    lines = doc.lines.iter().take(40).map(|u| u.text.clone()).collect();
                }
                lines
                    .into_iter()
                    .map(|l| Line::from(owned(summary_line(&l, "", false).spans)))
                    .collect()
            }
        };
        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block);
        f.render_widget(para, area);
    }

    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let loading = if self.is_loading() {
            format!(" indexing {}/{} ·", self.index.loaded(), self.index.size())
        } else {
            String::new()
        };
        let text = format!(
            "{loading} Enter open · Tab name↔phrase · ↑↓ PgUp/PgDn select · Ctrl+U clear · Esc back "
        );
        f.render_widget(Paragraph::new(Line::from(Span::styled(text, dim()))), area);
    }
}

/// The lines around a hit, the hit itself marked.
fn context<'a>(doc: &'a Doc, h: Hit, needle: &str, height: usize) -> Vec<Line<'a>> {
    let around = height.saturating_sub(2) / 2;
    let len = if h.in_summary {
        doc.summary.len()
    } else {
        doc.lines.len()
    };
    let (from, to) = (
        h.line.saturating_sub(around),
        (h.line + around + 1).min(len),
    );
    (from..to)
        .map(|i| match h.in_summary {
            true => summary_line(&doc.summary[i], needle, i == h.line),
            false => utterance_line(&doc.lines[i], needle, i == h.line),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::Path;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(b: &mut Browser, text: &str) {
        for c in text.chars() {
            b.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn write_session(live: &Path, name: &str, summary: &str, said: &[(u64, &str)]) {
        let dir = live.join("sessions").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let body: String = said
            .iter()
            .map(|(t, text)| {
                format!(
                    "{}\n",
                    serde_json::json!({"type": "transcript", "t": t, "speaker": "S1", "text": text})
                )
            })
            .collect();
        std::fs::write(dir.join("stream.jsonl"), body).unwrap();
        std::fs::write(dir.join("summary.md"), summary).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, Browser) {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("live");
        write_session(
            &live,
            "20261025-182225-dell-nvidia",
            "# Dell × NVIDIA hackathon\n- GB10 has 121 GB free\n",
            &[(83, "the GB10 box runs everything"), (95, "fifteen agents")],
        );
        write_session(
            &live,
            "20261025-163736",
            "# Sergey Levine talk\n",
            &[
                (7, "robots learn from humans"),
                (3723, "espresso for thirteen hours"),
            ],
        );
        let mut b = Browser::new(discover(&live, &tmp.path().join("calls")));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while b.is_loading() && std::time::Instant::now() < deadline {
            b.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        (tmp, b)
    }

    fn screen(b: &mut Browser) -> String {
        let mut term = Terminal::new(TestBackend::new(100, 16)).unwrap();
        term.draw(|f| b.render(f, f.area())).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_list_shows_every_recording_and_fuzzy_filters_it() {
        let (_tmp, mut b) = fixture();
        let s = screen(&mut b);
        assert!(s.contains("2/2 recordings"), "{s}");
        assert!(s.contains("Dell × NVIDIA hackathon") && s.contains("Sergey Levine talk"));
        assert!(s.contains("1h02"), "duration from the stream tail: {s}");
        type_text(&mut b, "levine");
        let s = screen(&mut b);
        assert!(
            s.contains("1/2 recordings") && !s.contains("NVIDIA hackathon  "),
            "{s}"
        );
        b.handle_key(key(KeyCode::Enter));
        assert!(b.is_reading());
        let s = screen(&mut b);
        assert!(s.contains("1:02:03") && s.contains("espresso"), "{s}");
        b.handle_key(key(KeyCode::Esc));
        assert!(!b.is_reading());
        assert_eq!(b.handle_key(key(KeyCode::Esc)), BrowserAction::Back);
    }

    #[test]
    fn phrase_mode_lists_matching_lines_with_their_time_and_opens_on_the_hit() {
        let (_tmp, mut b) = fixture();
        b.handle_key(key(KeyCode::Tab));
        type_text(&mut b, "gb10");
        let s = screen(&mut b);
        assert!(s.contains("2 lines"), "{s}");
        assert!(
            s.contains("t=1:23") && s.contains("S1: the GB10 box"),
            "{s}"
        );
        assert!(s.contains("summary"), "the summary line is a hit too: {s}");
        b.handle_key(key(KeyCode::Enter));
        let s = screen(&mut b);
        assert!(
            s.contains("match 1/1") && s.contains("1:23 S1: the GB10 box"),
            "{s}"
        );
    }

    #[test]
    fn question_mark_is_help_only_on_an_empty_query() {
        let (_tmp, mut b) = fixture();
        assert_eq!(b.handle_key(key(KeyCode::Char('?'))), BrowserAction::Help);
        type_text(&mut b, "why");
        assert_eq!(b.handle_key(key(KeyCode::Char('?'))), BrowserAction::None);
        assert_eq!(b.query, "why?");
        b.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(b.query.is_empty());
    }

    #[test]
    fn arrows_and_the_wheel_move_the_selection_within_bounds() {
        let (_tmp, mut b) = fixture();
        screen(&mut b);
        assert_eq!(b.list.selected(), Some(0));
        b.handle_key(key(KeyCode::Down));
        b.handle_key(key(KeyCode::Down));
        assert_eq!(b.list.selected(), Some(1));
        b.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(b.list.selected(), Some(0));
    }

    #[test]
    fn a_long_hit_list_scrolls_to_keep_the_selection_on_screen() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("live");
        let said: Vec<(u64, String)> = (0..300)
            .map(|i| (i, format!("agent number {i:03}")))
            .collect();
        let said: Vec<(u64, &str)> = said.iter().map(|(t, s)| (*t, s.as_str())).collect();
        write_session(&live, "20261025-100000", "# Many\n", &said);
        let mut b = Browser::new(discover(&live, &tmp.path().join("calls")));
        while b.is_loading() {
            b.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        b.handle_key(key(KeyCode::Tab));
        type_text(&mut b, "agent");
        assert_eq!(b.hits.len(), 300);
        b.handle_key(key(KeyCode::End));
        let s = screen(&mut b);
        assert!(
            s.contains("agent number 299") && !s.contains("agent number 000"),
            "{s}"
        );
        b.handle_key(key(KeyCode::Home));
        let s = screen(&mut b);
        assert!(s.contains("agent number 000"), "{s}");
    }

    #[test]
    fn snippet_keeps_the_match_in_view() {
        let text = format!("{} needle {}", "a".repeat(100), "b".repeat(100));
        let s = snippet(&text, "needle", 30);
        assert_eq!(s.chars().count(), 30);
        assert!(s.contains("needle") && s.starts_with('…') && s.ends_with('…'));
        assert_eq!(snippet("short", "x", 30), "short");
    }

    #[test]
    fn durations_read_like_a_clock() {
        assert_eq!(duration(30), "<1m");
        assert_eq!(duration(2520), "42m");
        assert_eq!(duration(3723), "1h02");
    }
}
