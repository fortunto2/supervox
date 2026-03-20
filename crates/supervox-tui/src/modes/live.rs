use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::time::Instant;

/// State for live mode.
pub struct LiveState {
    pub transcript_lines: Vec<String>,
    pub translation_lines: Vec<String>,
    pub summary_lines: Vec<String>,
    /// Current delta (partial) transcript, shown dimmed until final.
    pub current_delta: Option<String>,
    pub is_recording: bool,
    pub recording_start: Option<Instant>,
    pub audio_level: f32,
    pub stt_backend: String,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            transcript_lines: Vec::new(),
            translation_lines: Vec::new(),
            summary_lines: Vec::new(),
            current_delta: None,
            is_recording: false,
            recording_start: None,
            audio_level: 0.0,
            stt_backend: "realtime".into(),
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
        self.transcript_lines.clear();
        self.translation_lines.clear();
        self.summary_lines.clear();
        self.current_delta = None;
    }

    pub fn stop_recording(&mut self) {
        self.is_recording = false;
        self.current_delta = None;
    }

    /// Update current delta (partial) transcript, shown dimmed.
    pub fn update_delta(&mut self, source_label: &str, text: &str) {
        self.current_delta = Some(format!("{source_label}: {text}"));
    }

    /// Push a finalized transcript line with source label.
    pub fn push_final_transcript(&mut self, source_label: &str, text: &str) {
        if !text.is_empty() {
            self.transcript_lines
                .push(format!("{source_label}: {text}"));
        }
        self.current_delta = None;
    }

    pub fn push_translation(&mut self, text: &str) {
        self.translation_lines.push(text.to_string());
    }

    /// Set the rolling summary text (replaces previous).
    pub fn set_summary(&mut self, text: &str) {
        self.summary_lines = text.lines().map(|l| l.to_string()).collect();
    }
}

/// Render live mode — left panel (transcript + translation), right panel (summary), bottom status.
pub fn render(f: &mut Frame, area: Rect, state: &LiveState) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(area);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(main_area);

    // Left panel: transcript + translation + delta
    let mut text_lines: Vec<Line> = Vec::new();

    if state.transcript_lines.is_empty() && state.current_delta.is_none() {
        let msg = if state.is_recording {
            "Listening..."
        } else {
            "Press 'r' to start recording"
        };
        text_lines.push(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, t) in state.transcript_lines.iter().enumerate() {
            text_lines.push(Line::from(Span::styled(
                t.clone(),
                Style::default().fg(Color::White),
            )));
            if let Some(tr) = state.translation_lines.get(i) {
                text_lines.push(Line::from(Span::styled(
                    format!("  → {tr}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
        // Show current delta (partial) dimmed
        if let Some(delta) = &state.current_delta {
            text_lines.push(Line::from(Span::styled(
                delta.clone(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            )));
        }
    }

    let transcript = Paragraph::new(text_lines)
        .block(
            Block::default()
                .title(" Transcript ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if state.is_recording {
                    Color::Green
                } else {
                    Color::Cyan
                })),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(transcript, left);

    // Right panel: rolling summary
    let summary_text = if state.summary_lines.is_empty() {
        "Summary will appear here".to_string()
    } else {
        state
            .summary_lines
            .iter()
            .map(|s| format!("• {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let summary = Paragraph::new(summary_text)
        .block(
            Block::default()
                .title(" Rolling Summary ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::Gray));
    f.render_widget(summary, right);

    // Status bar
    let mic_indicator = if state.is_recording {
        Span::styled(
            " ● REC ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ○ IDLE ", Style::default().fg(Color::DarkGray))
    };

    let level_bar = audio_level_bar(state.audio_level);

    let timer = if state.is_recording {
        let secs = state.elapsed_secs();
        format!(" {}:{:02} ", secs / 60, secs % 60)
    } else {
        " 0:00 ".into()
    };

    let status_line = Line::from(vec![
        mic_indicator,
        Span::styled(level_bar, Style::default().fg(Color::Green)),
        Span::raw(" │ "),
        Span::styled(
            format!("STT: {}", state.stt_backend),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" │ "),
        Span::styled(timer, Style::default().fg(Color::White)),
        Span::raw(" │ "),
        Span::styled(
            " r=record s=stop q=quit ",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    f.render_widget(
        Paragraph::new(status_line).style(Style::default().bg(Color::Black)),
        status_area,
    );
}

fn audio_level_bar(level: f32) -> String {
    let bars = (level * 10.0).min(10.0) as usize;
    format!("[{}{}]", "█".repeat(bars), "░".repeat(10 - bars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_state_default() {
        let state = LiveState::default();
        assert!(!state.is_recording);
        assert!(state.transcript_lines.is_empty());
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
        state.update_delta("You", "Hel");
        assert_eq!(state.current_delta.as_deref(), Some("You: Hel"));
        assert!(state.transcript_lines.is_empty());

        state.push_final_transcript("You", "Hello world");
        assert!(state.current_delta.is_none());
        assert_eq!(state.transcript_lines.len(), 1);
        assert_eq!(state.transcript_lines[0], "You: Hello world");
    }

    #[test]
    fn push_translation() {
        let mut state = LiveState::default();
        state.push_final_transcript("Them", "Bonjour");
        state.push_translation("Привет");
        assert_eq!(state.transcript_lines.len(), 1);
        assert_eq!(state.translation_lines.len(), 1);
    }

    #[test]
    fn set_summary_multiline() {
        let mut state = LiveState::default();
        state.set_summary("Line 1\nLine 2\nLine 3");
        assert_eq!(state.summary_lines.len(), 3);
        assert_eq!(state.summary_lines[0], "Line 1");
    }

    #[test]
    fn audio_level_bar_empty() {
        assert_eq!(audio_level_bar(0.0), "[░░░░░░░░░░]");
    }

    #[test]
    fn audio_level_bar_full() {
        assert_eq!(audio_level_bar(1.0), "[██████████]");
    }

    #[test]
    fn audio_level_bar_half() {
        assert_eq!(audio_level_bar(0.5), "[█████░░░░░]");
    }
}
