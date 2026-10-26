//! Reading one recording: transcript and summary side by side, each scrollable, with the
//! search phrase highlighted and `n`/`N` stepping through its matches.

use super::discover::SessionEntry;
use super::search::{Doc, find_ci};
use super::transcript::{Utterance, clock};
use crate::scroll::{Pane, pane_title, position_label, render_pane};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use sgr_agent_tui::FocusRing;

const TRANSCRIPT: &str = "transcript";
const SUMMARY: &str = "summary";

/// What the browser should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderAction {
    None,
    /// Back to the list.
    Close,
    Help,
}

/// One pane's matches and which one is current.
#[derive(Debug, Default)]
struct Matches {
    lines: Vec<usize>,
    current: Option<usize>,
}

impl Matches {
    fn step(&mut self, forward: bool) -> Option<usize> {
        let n = self.lines.len();
        if n == 0 {
            return None;
        }
        let next = match (self.current, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        self.current = Some(next);
        self.lines.get(next).copied()
    }

    fn current_line(&self) -> Option<usize> {
        self.current.and_then(|i| self.lines.get(i).copied())
    }

    fn label(&self) -> String {
        match (self.current, self.lines.len()) {
            (_, 0) => String::new(),
            (Some(i), n) => format!("match {}/{n} · n/N", i + 1),
            (None, 1) => "1 match · n".into(),
            (None, n) => format!("{n} matches · n/N"),
        }
    }
}

/// The read view of one recording.
pub struct Reader {
    pub entry: SessionEntry,
    doc: Doc,
    needle: String,
    spoken: Matches,
    noted: Matches,
    transcript: Pane,
    summary: Pane,
    /// A line to bring into view on the next render, which knows the wrap width.
    jump: Option<(&'static str, usize)>,
    focus: FocusRing,
}

impl Reader {
    /// Open `doc`. With a phrase, its matches are highlighted; `at` is the line that was
    /// picked (`in_summary` selects the pane).
    pub fn new(entry: SessionEntry, doc: Doc, phrase: &str, at: Option<(usize, bool)>) -> Self {
        let needle = phrase.trim().to_lowercase();
        let mut r = Self {
            spoken: Matches {
                lines: doc.matching_lines(&needle),
                current: None,
            },
            noted: Matches {
                lines: doc.matching_summary(&needle),
                current: None,
            },
            entry,
            doc,
            needle,
            transcript: Pane::at_top(),
            summary: Pane::at_top(),
            jump: None,
            focus: FocusRing::new(&[TRANSCRIPT, SUMMARY]),
        };
        if let Some((line, in_summary)) = at {
            let (pane, m) = match in_summary {
                true => (SUMMARY, &mut r.noted),
                false => (TRANSCRIPT, &mut r.spoken),
            };
            m.current = m.lines.iter().position(|l| *l == line);
            r.focus.focus(pane);
            r.jump = Some((pane, line));
        } else if r.doc.lines.is_empty() {
            r.focus.focus(SUMMARY);
        }
        r
    }

    fn focused(&mut self) -> (&'static str, &mut Pane, &mut Matches) {
        if self.focus.is_focused(SUMMARY) {
            (SUMMARY, &mut self.summary, &mut self.noted)
        } else {
            (TRANSCRIPT, &mut self.transcript, &mut self.spoken)
        }
    }

    /// Next (`forward`) or previous match in the focused pane.
    pub fn step_match(&mut self, forward: bool) {
        let (pane, _, matches) = self.focused();
        if let Some(line) = matches.step(forward) {
            self.jump = Some((pane, line));
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ReaderAction {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) | (KeyCode::Char('/'), _) => {
                return ReaderAction::Close;
            }
            (KeyCode::Char('?'), _) => return ReaderAction::Help,
            (KeyCode::Tab, _) => self.focus.next(),
            (KeyCode::BackTab, _) => self.focus.prev(),
            (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                self.focus.focus(TRANSCRIPT);
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                self.focus.focus(SUMMARY);
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) => self.step_match(true),
            (KeyCode::Char('N'), _) | (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.step_match(false)
            }
            _ => {
                self.focused().1.view.handle_key(key);
            }
        }
        ReaderAction::None
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.focus.click(mouse.column, mouse.row);
                self.focused().1.view.handle_wheel(mouse);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus.click(mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let [head, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).areas(area);
        self.render_header(f, head);
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(body);
        self.focus.set_rect(TRANSCRIPT, left);
        self.focus.set_rect(SUMMARY, right);
        self.apply_jump(left, right);
        self.render_transcript(f, left);
        self.render_summary(f, right);
    }

    /// Bring the pending line into view, now that the pane widths are known.
    fn apply_jump(&mut self, left: Rect, right: Rect) {
        let Some((pane, line)) = self.jump.take() else {
            return;
        };
        let needle = self.needle.as_str();
        let (p, area) = if pane == SUMMARY {
            let current = self.noted.current_line();
            let width = Block::bordered().inner(right).width;
            let lines = &self.doc.summary;
            self.summary.rows.sync(width, lines, |i, l| {
                summary_line(l, needle, current == Some(i))
            });
            (&mut self.summary, right)
        } else {
            let current = self.spoken.current_line();
            let width = Block::bordered().inner(left).width;
            let lines = &self.doc.lines;
            self.transcript.rows.sync(width, lines, |i, u| {
                utterance_line(u, needle, current == Some(i))
            });
            (&mut self.transcript, left)
        };
        let height = Block::bordered().inner(area).height as usize;
        p.view.layout(p.rows.total(), height);
        p.view.reveal(p.rows.row_of(line));
    }

    fn block(&self, id: &str, title: String, color: Color) -> Block<'static> {
        let focused = self.focus.is_focused(id);
        Block::default()
            .borders(Borders::ALL)
            .border_type(if focused {
                BorderType::Thick
            } else {
                BorderType::Plain
            })
            .border_style(Style::default().fg(color))
            .title(title)
    }

    /// `10-25 18:22 · 42m · Dell × NVIDIA … · 20261025-182225-dell-nvidia`
    fn render_header(&self, f: &mut Frame, area: Rect) {
        let e = &self.entry;
        let mut spans = vec![Span::styled(
            format!(" {} ", e.date_label()),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        )];
        if let Some(d) = e.duration_secs {
            spans.push(Span::raw(format!(" {} ·", super::browser::duration(d))));
        }
        spans.push(Span::styled(
            format!(" {} ", e.label()),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        if e.title.is_some() {
            spans.push(Span::styled(
                format!("· {}", e.name),
                Style::default().fg(Color::DarkGray),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_transcript(&mut self, f: &mut Frame, area: Rect) {
        let title = pane_title(&[
            "Transcript",
            &position_label(&self.transcript.view, false),
            &self.spoken.label(),
        ]);
        let block = self.block(TRANSCRIPT, title, Color::Cyan);
        if self.doc.lines.is_empty() {
            let msg = Paragraph::new(Span::styled(
                "No transcript in this recording",
                Style::default().fg(Color::DarkGray),
            ));
            f.render_widget(msg.block(block), area);
            return;
        }
        let (needle, current) = (self.needle.as_str(), self.spoken.current_line());
        render_pane(
            f,
            area,
            block,
            &mut self.transcript,
            &self.doc.lines,
            |i, u| utterance_line(u, needle, current == Some(i)),
            None,
        );
    }

    fn render_summary(&mut self, f: &mut Frame, area: Rect) {
        let title = pane_title(&[
            "Summary",
            &position_label(&self.summary.view, false),
            &self.noted.label(),
        ]);
        let block = self.block(SUMMARY, title, Color::Yellow);
        if self.doc.summary.is_empty() {
            let msg = Paragraph::new(Span::styled(
                "No summary",
                Style::default().fg(Color::DarkGray),
            ));
            f.render_widget(msg.block(block), area);
            return;
        }
        let (needle, current) = (self.needle.as_str(), self.noted.current_line());
        render_pane(
            f,
            area,
            block,
            &mut self.summary,
            &self.doc.summary,
            |i, l| summary_line(l, needle, current == Some(i)),
            None,
        );
    }
}

/// Spans of `text` in `base`, with each occurrence of `needle` (lowercase) marked.
pub fn highlighted<'a>(text: &'a str, needle: &str, base: Style) -> Vec<Span<'a>> {
    let mark = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut at = 0;
    for r in find_ci(text, needle) {
        if r.start > at {
            spans.push(Span::styled(&text[at..r.start], base));
        }
        spans.push(Span::styled(&text[r.clone()], mark));
        at = r.end;
    }
    if at < text.len() || spans.is_empty() {
        spans.push(Span::styled(&text[at..], base));
    }
    spans
}

/// Speaker color shared with the live pane: one per diarized speaker.
pub fn speaker_color(speaker: &str) -> Color {
    match speaker {
        "S1" | "You" => Color::Cyan,
        "S2" => Color::Green,
        "S3" => Color::Magenta,
        "Them" => Color::Yellow,
        _ => Color::LightRed,
    }
}

/// `12:34 S1: text`, the current match on a dark background.
pub fn utterance_line<'a>(u: &'a Utterance, needle: &str, current: bool) -> Line<'a> {
    let mut spans = Vec::new();
    if let Some(t) = u.t {
        spans.push(Span::styled(
            format!("{:>5} ", clock(t)),
            Style::default().fg(Color::DarkGray),
        ));
    }
    if !u.speaker.is_empty() {
        spans.push(Span::styled(
            format!("{}: ", u.speaker),
            Style::default().fg(speaker_color(&u.speaker)),
        ));
    }
    spans.extend(highlighted(
        &u.text,
        needle,
        Style::default().fg(Color::White),
    ));
    let line = Line::from(spans);
    if current {
        line.style(Style::default().bg(Color::DarkGray))
    } else {
        line
    }
}

/// A summary line with headings set off, as Markdown reads.
pub fn summary_line<'a>(text: &'a str, needle: &str, current: bool) -> Line<'a> {
    let base = if text.starts_with('#') {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let line = Line::from(highlighted(text, needle, base));
    if current {
        line.style(Style::default().bg(Color::DarkGray))
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::super::discover::Kind;
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn entry() -> SessionEntry {
        SessionEntry {
            name: "20261025-182225-dell-nvidia".into(),
            kind: Kind::Session,
            started: None,
            duration_secs: None,
            title: None,
            streams: vec![],
            summary: None,
            call: None,
            bytes: 0,
            live: false,
        }
    }

    fn doc() -> Doc {
        let lines = (0..200)
            .map(|i| Utterance {
                t: Some(i * 10),
                speaker: "S1".into(),
                text: if i % 50 == 7 {
                    format!("line {i} about GB10 memory")
                } else {
                    format!("line {i}")
                },
            })
            .collect();
        Doc::new(lines, "# Dell\n- GB10 has 121 GB free\n- other\n")
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn screen(r: &mut Reader) -> String {
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| r.render(f, f.area())).unwrap();
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
    fn opening_on_a_hit_scrolls_it_into_view_and_n_steps_through_matches() {
        let mut r = Reader::new(entry(), doc(), "gb10", Some((57, false)));
        assert_eq!(r.spoken.lines, [7, 57, 107, 157]);
        assert_eq!(r.spoken.current, Some(1));
        let s = screen(&mut r);
        assert!(s.contains("line 57 about"), "{s}");
        assert!(s.contains("match 2/4"), "{s}");
        r.handle_key(key(KeyCode::Char('n')));
        let s = screen(&mut r);
        assert!(s.contains("line 107 about"), "{s}");
        r.handle_key(key(KeyCode::Char('N')));
        r.handle_key(key(KeyCode::Char('N')));
        let s = screen(&mut r);
        assert!(s.contains("line 7 about") && s.contains("match 1/4"), "{s}");
        r.handle_key(key(KeyCode::Char('N'))); // wraps to the last
        assert_eq!(r.spoken.current, Some(3));
    }

    #[test]
    fn panes_scroll_independently_and_tab_moves_focus() {
        let mut r = Reader::new(entry(), doc(), "", None);
        screen(&mut r);
        r.handle_key(key(KeyCode::End));
        let s = screen(&mut r);
        assert!(s.contains("line 199"), "{s}");
        assert!(s.contains("100%"));
        r.handle_key(key(KeyCode::Tab));
        r.handle_key(key(KeyCode::Char('j'))); // summary fits: nothing moves
        let s = screen(&mut r);
        assert!(s.contains("line 199") && s.contains("# Dell"), "{s}");
        assert_eq!(r.handle_key(key(KeyCode::Esc)), ReaderAction::Close);
    }

    #[test]
    fn a_summary_hit_focuses_the_summary_pane() {
        let r = Reader::new(entry(), doc(), "GB10", Some((1, true)));
        assert!(r.focus.is_focused(SUMMARY));
        assert_eq!(r.noted.current, Some(0));
    }

    #[test]
    fn highlighted_marks_every_occurrence_without_losing_text() {
        let spans = highlighted("GB10 и gb10!", "gb10", Style::default());
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "GB10 и gb10!");
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].style.bg, Some(Color::Yellow));
        assert_eq!(highlighted("", "x", Style::default()).len(), 1);
    }
}
