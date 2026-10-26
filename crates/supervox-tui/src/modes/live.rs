use crate::audio::AudioSource;
use crate::scroll::{Pane, pane_title, position_label, render_pane};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use sgr_agent_tui::FocusRing;
use std::time::Instant;
use supervox_agent::types::Bookmark;

const TRANSCRIPT: &str = "transcript";
const SUMMARY: &str = "summary";

/// A single line in the transcript view — either speech or its translation.
#[derive(Debug, Clone)]
pub struct TranscriptLine {
    pub source: AudioSource,
    pub text: String,
    pub is_translation: bool,
    /// Diarized speaker index (mic only).
    pub speaker: Option<usize>,
}

/// State for live mode.
pub struct LiveState {
    pub lines: Vec<TranscriptLine>,
    pub summary_lines: Vec<String>,
    /// Current delta (partial) transcript, shown dimmed until final.
    pub current_delta: Option<(AudioSource, String)>,
    pub is_recording: bool,
    pub recording_start: Option<Instant>,
    pub mic_level: f32,
    pub system_level: f32,
    pub is_ducked: bool,
    pub stt_backend: String,
    pub bookmarks: Vec<Bookmark>,
    /// Current input device name (`None` = system default).
    pub mic_device: Option<String>,
    /// Summary panel title, set from `config.summary`.
    pub summary_title: String,
    /// External summaries are Markdown already; internal ones get bullets.
    pub summary_raw: bool,
    /// Follows the newest line until scrolled up; End resumes.
    pub transcript_pane: Pane,
    /// A document: starts at the top, keeps its place when the agent rewrites it.
    pub summary_pane: Pane,
    /// Which pane the scroll keys move; Tab cycles, a click or the wheel picks.
    pub focus: FocusRing,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            summary_lines: Vec::new(),
            current_delta: None,
            is_recording: false,
            recording_start: None,
            mic_level: 0.0,
            system_level: 0.0,
            is_ducked: false,
            stt_backend: "realtime".into(),
            mic_device: None,
            summary_title: " Rolling Summary ".into(),
            summary_raw: false,
            bookmarks: Vec::new(),
            transcript_pane: Pane::following(),
            summary_pane: Pane::at_top(),
            focus: FocusRing::new(&[TRANSCRIPT, SUMMARY]),
        }
    }
}

impl LiveState {
    pub fn elapsed_secs(&self) -> u64 {
        self.recording_start
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0)
    }

    pub fn start_recording(&mut self) {
        self.is_recording = true;
        self.recording_start = Some(Instant::now());
        self.lines.clear();
        self.summary_lines.clear();
        self.current_delta = None;
        self.bookmarks.clear();
        self.transcript_pane = Pane::following();
        self.summary_pane = Pane::at_top();
    }

    fn focused_pane(&mut self) -> &mut Pane {
        if self.focus.is_focused(SUMMARY) {
            &mut self.summary_pane
        } else {
            &mut self.transcript_pane
        }
    }

    /// Tab/Shift-Tab switch panes; arrows, j/k, PgUp/PgDn, Home/End, g/G scroll the
    /// focused one. True if the key was one of these.
    pub fn handle_scroll_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Tab => self.focus.next(),
            KeyCode::BackTab => self.focus.prev(),
            _ => return self.focused_pane().view.handle_key(key),
        }
        true
    }

    /// The wheel scrolls the pane under the pointer (and focuses it); a click focuses.
    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.focus.click(mouse.column, mouse.row);
                self.focused_pane().view.handle_wheel(mouse);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus.click(mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    /// Add a bookmark at the current elapsed time.
    pub fn add_bookmark(&mut self) -> Bookmark {
        let timestamp_secs = self.elapsed_secs() as f64;
        let bookmark = Bookmark {
            timestamp_secs,
            note: None,
        };
        self.bookmarks.push(bookmark.clone());
        bookmark
    }

    pub fn stop_recording(&mut self) {
        self.is_recording = false;
        self.current_delta = None;
    }

    /// Update current delta (partial) transcript, shown dimmed.
    pub fn update_delta(&mut self, source: AudioSource, text: &str) {
        self.current_delta = Some((source, text.to_string()));
    }

    /// Push a finalized transcript line.
    pub fn push_final_transcript(
        &mut self,
        source: AudioSource,
        text: &str,
        speaker: Option<usize>,
    ) {
        if !text.is_empty() {
            self.lines.push(TranscriptLine {
                source,
                text: text.to_string(),
                is_translation: false,
                speaker,
            });
        }
        self.current_delta = None;
    }

    /// Push a translation line, inheriting source from the last transcript.
    pub fn push_translation(&mut self, source: AudioSource, text: &str) {
        self.lines.push(TranscriptLine {
            source,
            text: text.to_string(),
            is_translation: true,
            speaker: None,
        });
    }

    /// Set the rolling summary text (replaces previous).
    pub fn set_summary(&mut self, text: &str) {
        self.summary_lines = text.lines().map(|l| l.to_string()).collect();
        self.summary_pane.replaced();
    }

    /// Count of non-translation transcript lines (for pipeline IDs).
    pub fn transcript_count(&self) -> usize {
        self.lines.iter().filter(|l| !l.is_translation).count()
    }
}

/// One transcript line: `S1: text`, translations indented and italic.
fn transcript_line(line: &TranscriptLine) -> Line<'_> {
    let color = speaker_color(&line.source, line.speaker);
    let text_style = if line.is_translation {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::ITALIC)
    } else {
        Style::default().fg(Color::White)
    };
    let label = crate::audio::speaker_label(&line.source, line.speaker);
    let prefix = if line.is_translation {
        format!("  → {label}: ")
    } else {
        format!("{label}: ")
    };
    Line::from(vec![
        Span::styled(prefix, Style::default().fg(color)),
        Span::styled(line.text.as_str(), text_style),
    ])
}

/// The partial (not yet final) transcript, dimmed.
fn delta_line(source: &AudioSource, text: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}: ", source.label()),
            Style::default()
                .fg(speaker_color(source, None))
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            text.to_string(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
    ])
}

/// A pane border: thick when focused, in the pane's own color either way.
fn pane_block(title: String, color: Color, focused: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(if focused {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(Style::default().fg(color))
}

fn render_transcript(f: &mut Frame, area: Rect, state: &mut LiveState) {
    let color = if state.is_recording {
        Color::Green
    } else {
        Color::Cyan
    };
    let title = pane_title(&[
        "Transcript",
        &position_label(&state.transcript_pane.view, true),
    ]);
    let block = pane_block(title, color, state.focus.is_focused(TRANSCRIPT));
    if state.lines.is_empty() && state.current_delta.is_none() {
        let msg = if state.is_recording {
            "Listening..."
        } else {
            "Press 'r' to start recording"
        };
        let hint = Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray)));
        f.render_widget(hint.block(block), area);
        return;
    }
    let tail = state
        .current_delta
        .as_ref()
        .map(|(source, text)| delta_line(source, text));
    render_pane(
        f,
        area,
        block,
        &mut state.transcript_pane,
        &state.lines,
        |_, l| transcript_line(l),
        tail,
    );
}

fn render_summary(f: &mut Frame, area: Rect, state: &mut LiveState) {
    let title = pane_title(&[
        &state.summary_title,
        &position_label(&state.summary_pane.view, false),
    ]);
    let block = pane_block(title, Color::Yellow, state.focus.is_focused(SUMMARY));
    let gray = Style::default().fg(Color::Gray);
    if state.summary_lines.is_empty() {
        let hint = Paragraph::new(Span::styled("Summary will appear here", gray));
        f.render_widget(hint.block(block), area);
        return;
    }
    let raw = state.summary_raw;
    render_pane(
        f,
        area,
        block,
        &mut state.summary_pane,
        &state.summary_lines,
        |_, l| match raw {
            true => Line::styled(l.as_str(), gray),
            false => Line::styled(format!("• {l}"), gray),
        },
        None,
    );
}

/// Render live mode — left panel (transcript + translation), right panel (summary), bottom status.
pub fn render(f: &mut Frame, area: Rect, state: &mut LiveState) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(main_area);
    state.focus.set_rect(TRANSCRIPT, left);
    state.focus.set_rect(SUMMARY, right);
    render_transcript(f, left, state);
    render_summary(f, right, state);

    // Status bar
    let mic_indicator = if state.is_recording {
        Span::styled(
            " ● REC ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ○ IDLE ", Style::default().fg(Color::DarkGray))
    };

    let mic_bar = audio_level_bar(state.mic_level, 6);
    let sys_bar = audio_level_bar(state.system_level, 6);

    let timer = if state.is_recording {
        let secs = state.elapsed_secs();
        format!(" {}:{:02} ", secs / 60, secs % 60)
    } else {
        " 0:00 ".into()
    };

    let mic_name = state
        .mic_device
        .as_deref()
        .map(|n| n.chars().take(18).collect::<String>())
        .unwrap_or_else(|| "default".into());
    let mic_label = if state.is_ducked {
        Span::styled(
            format!("mic {mic_bar} \u{1F507}"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )
    } else {
        Span::styled(
            format!("mic {mic_bar} {mic_name}"),
            Style::default().fg(Color::Cyan),
        )
    };

    let mut status_spans = vec![
        mic_indicator,
        mic_label,
        Span::raw(" "),
        Span::styled(format!("sys {sys_bar}"), Style::default().fg(Color::Yellow)),
        Span::raw(" │ "),
        Span::styled(
            format!("STT: {}", state.stt_backend),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" │ "),
        Span::styled(timer, Style::default().fg(Color::White)),
    ];

    if !state.bookmarks.is_empty() {
        let count = state.bookmarks.len();
        status_spans.push(Span::raw(" │ "));
        status_spans.push(Span::styled(
            format!("{count} bookmark{}", if count == 1 { "" } else { "s" }),
            Style::default().fg(Color::Yellow),
        ));
    }

    status_spans.push(Span::raw(" │ "));
    status_spans.push(Span::styled(
        " Space=rec/stop Enter=mark Tab/PgUp/PgDn=scroll /=sessions ?=help q=quit ",
        Style::default().fg(Color::DarkGray),
    ));

    let status_line = Line::from(status_spans);

    f.render_widget(
        Paragraph::new(status_line).style(Style::default().bg(Color::Black)),
        status_area,
    );
}

/// Color for speaker labels: one per diarized speaker, else per source.
fn speaker_color(source: &AudioSource, speaker: Option<usize>) -> Color {
    match (source, speaker) {
        (AudioSource::System, _) => Color::Yellow,
        (AudioSource::Mic, Some(1)) => Color::Cyan,
        (AudioSource::Mic, Some(2)) => Color::Green,
        (AudioSource::Mic, Some(3)) => Color::Magenta,
        (AudioSource::Mic, Some(_)) => Color::LightRed,
        (AudioSource::Mic, None) => Color::Cyan,
    }
}

fn audio_level_bar(level: f32, width: usize) -> String {
    // RMS 0.0-0.15 is typical speech range (same scale as souffleur)
    let normalized = (level.min(0.15) / 0.15).min(1.0);
    let bars = (normalized * width as f32) as usize;
    format!("[{}{}]", "█".repeat(bars), "░".repeat(width - bars))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn screen(state: &mut LiveState) -> String {
        let mut term = Terminal::new(TestBackend::new(80, 14)).unwrap();
        term.draw(|f| render(f, f.area(), state)).unwrap();
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

    fn press(state: &mut LiveState, code: KeyCode) -> bool {
        state.handle_scroll_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn recording(lines: usize) -> LiveState {
        let mut s = LiveState::default();
        s.start_recording();
        for i in 0..lines {
            s.push_final_transcript(AudioSource::Mic, &format!("L{i:03}"), Some(1));
        }
        s
    }

    #[test]
    fn a_long_transcript_shows_its_tail_and_stays_put_once_scrolled_up() {
        let mut s = recording(100);
        let out = screen(&mut s);
        assert!(out.contains("L099") && !out.contains("L080"), "{out}");
        assert!(press(&mut s, KeyCode::PageUp));
        let out = screen(&mut s);
        assert!(
            !out.contains("L099") && out.contains("End follows"),
            "{out}"
        );
        s.push_final_transcript(AudioSource::Mic, "L100", Some(1));
        assert!(
            !screen(&mut s).contains("L100"),
            "scrolled up: new lines wait below"
        );
        assert!(press(&mut s, KeyCode::End));
        let out = screen(&mut s);
        assert!(
            out.contains("L100") && !out.contains("End follows"),
            "{out}"
        );
    }

    #[test]
    fn the_partial_line_follows_the_tail() {
        let mut s = recording(40);
        s.update_delta(AudioSource::Mic, "still talk");
        let out = screen(&mut s);
        assert!(out.contains("L039") && out.contains("still talk"), "{out}");
    }

    #[test]
    fn a_long_wrapped_line_is_shown_to_its_last_row() {
        let mut s = recording(30);
        let long = (0..40)
            .map(|i| format!("w{i:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        s.push_final_transcript(AudioSource::Mic, &long, Some(1));
        let out = screen(&mut s);
        assert!(
            out.contains("w39"),
            "the last wrapped row is on screen: {out}"
        );
    }

    #[test]
    fn the_summary_scrolls_on_its_own_and_keeps_its_place_on_rewrite() {
        let mut s = recording(100);
        s.summary_raw = true;
        let doc = |v: &str| {
            (0..60)
                .map(|i| format!("R{i:02}{v}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        s.set_summary(&doc(""));
        let out = screen(&mut s);
        assert!(
            out.contains("R00") && !out.contains("R59"),
            "a document starts at the top"
        );
        assert!(press(&mut s, KeyCode::Tab));
        assert!(press(&mut s, KeyCode::Char('j')));
        assert!(press(&mut s, KeyCode::Char('j')));
        let out = screen(&mut s);
        assert!(out.contains("R02") && !out.contains("R01"), "{out}");
        assert!(out.contains("L099"), "the transcript did not move");
        s.set_summary(&doc("v2"));
        let out = screen(&mut s);
        assert!(out.contains("R02v2") && !out.contains("R01v2"), "{out}");
        press(&mut s, KeyCode::End);
        assert!(screen(&mut s).contains("R59v2"));
    }

    #[test]
    fn the_wheel_scrolls_the_pane_under_the_pointer() {
        let mut s = recording(100);
        screen(&mut s); // lays out the pane rects
        let wheel = |column| MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        s.handle_mouse(wheel(10));
        assert!(!s.transcript_pane.view.is_following());
        assert!(s.focus.is_focused(TRANSCRIPT));
        s.handle_mouse(wheel(70));
        assert!(s.focus.is_focused(SUMMARY));
    }

    #[test]
    fn a_new_recording_follows_again() {
        let mut s = recording(100);
        screen(&mut s);
        press(&mut s, KeyCode::Home);
        s.start_recording();
        assert!(s.transcript_pane.view.is_following());
        assert!(!press(&mut s, KeyCode::Char('x')));
    }

    #[test]
    fn live_state_default() {
        let state = LiveState::default();
        assert!(!state.is_recording);
        assert!(state.lines.is_empty());
        assert!(state.current_delta.is_none());
        assert_eq!(state.stt_backend, "realtime");
    }

    #[test]
    fn start_stop_recording() {
        let mut state = LiveState::default();
        state.start_recording();
        assert!(state.is_recording);
        assert!(state.recording_start.is_some());

        state.stop_recording();
        assert!(!state.is_recording);
        assert!(state.current_delta.is_none());
    }

    #[test]
    fn delta_and_final_transcript() {
        let mut state = LiveState::default();
        state.update_delta(AudioSource::Mic, "Hel");
        assert!(state.current_delta.is_some());
        let (src, text) = state.current_delta.as_ref().unwrap();
        assert_eq!(*src, AudioSource::Mic);
        assert_eq!(text, "Hel");
        assert!(state.lines.is_empty());

        state.push_final_transcript(AudioSource::Mic, "Hello world", None);
        assert!(state.current_delta.is_none());
        assert_eq!(state.lines.len(), 1);
        assert_eq!(state.lines[0].text, "Hello world");
        assert_eq!(state.lines[0].source, AudioSource::Mic);
        assert!(!state.lines[0].is_translation);
    }

    #[test]
    fn push_translation() {
        let mut state = LiveState::default();
        state.push_final_transcript(AudioSource::System, "Bonjour", None);
        state.push_translation(AudioSource::System, "Привет");
        assert_eq!(state.lines.len(), 2);
        assert!(!state.lines[0].is_translation);
        assert!(state.lines[1].is_translation);
        assert_eq!(state.lines[1].source, AudioSource::System);
    }

    #[test]
    fn transcript_count() {
        let mut state = LiveState::default();
        state.push_final_transcript(AudioSource::Mic, "Hello", None);
        state.push_translation(AudioSource::Mic, "Привет");
        state.push_final_transcript(AudioSource::System, "Bonjour", None);
        assert_eq!(state.transcript_count(), 2);
    }

    #[test]
    fn set_summary_multiline() {
        let mut state = LiveState::default();
        state.set_summary("Line 1\nLine 2\nLine 3");
        assert_eq!(state.summary_lines.len(), 3);
        assert_eq!(state.summary_lines[0], "Line 1");
    }

    #[test]
    fn add_bookmark_during_recording() {
        let mut state = LiveState::default();
        state.start_recording();
        assert!(state.bookmarks.is_empty());

        let bm = state.add_bookmark();
        assert_eq!(state.bookmarks.len(), 1);
        assert!(bm.note.is_none());
        // Timestamp should be >= 0 (recording just started)
        assert!(bm.timestamp_secs >= 0.0);
    }

    #[test]
    fn bookmarks_cleared_on_start() {
        let mut state = LiveState::default();
        state.start_recording();
        state.add_bookmark();
        assert_eq!(state.bookmarks.len(), 1);

        // Starting new recording clears bookmarks
        state.start_recording();
        assert!(state.bookmarks.is_empty());
    }

    #[test]
    fn multiple_bookmarks() {
        let mut state = LiveState::default();
        state.start_recording();
        state.add_bookmark();
        state.add_bookmark();
        state.add_bookmark();
        assert_eq!(state.bookmarks.len(), 3);
    }

    #[test]
    fn audio_level_bar_empty() {
        assert_eq!(audio_level_bar(0.0, 6), "[░░░░░░]");
    }

    #[test]
    fn audio_level_bar_full() {
        assert_eq!(audio_level_bar(0.15, 6), "[██████]"); // 0.15 = max speech → full
    }

    #[test]
    fn audio_level_bar_half() {
        assert_eq!(audio_level_bar(0.075, 6), "[███░░░]"); // half of 0.15 range
    }

    #[test]
    fn dual_level_fields() {
        let state = LiveState::default();
        assert_eq!(state.mic_level, 0.0);
        assert_eq!(state.system_level, 0.0);
    }

    #[test]
    fn ducking_default_false() {
        let state = LiveState::default();
        assert!(!state.is_ducked);
    }

    #[test]
    fn ducking_state_independent_of_recording() {
        let mut state = LiveState::default();
        state.start_recording();
        assert!(!state.is_ducked);
        state.is_ducked = true;
        assert!(state.is_ducked);
        // Stop recording doesn't reset ducking (pipeline handles that)
        state.stop_recording();
        assert!(state.is_ducked);
    }
}
