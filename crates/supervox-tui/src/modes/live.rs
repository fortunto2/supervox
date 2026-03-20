use crate::audio::AudioSource;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::time::Instant;

/// A single line in the transcript view — either speech or its translation.
#[derive(Debug, Clone)]
pub struct TranscriptLine {
    pub source: AudioSource,
    pub text: String,
    pub is_translation: bool,
}

/// State for live mode.
pub struct LiveState {
    pub lines: Vec<TranscriptLine>,
    pub summary_lines: Vec<String>,
    /// Current delta (partial) transcript, shown dimmed until final.
    pub current_delta: Option<(AudioSource, String)>,
    pub is_recording: bool,
    pub recording_start: Option<Instant>,
    pub audio_level: f32,
    pub stt_backend: String,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
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
        self.lines.clear();
        self.summary_lines.clear();
        self.current_delta = None;
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
    pub fn push_final_transcript(&mut self, source: AudioSource, text: &str) {
        if !text.is_empty() {
            self.lines.push(TranscriptLine {
                source,
                text: text.to_string(),
                is_translation: false,
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
        });
    }

    /// Set the rolling summary text (replaces previous).
    pub fn set_summary(&mut self, text: &str) {
        self.summary_lines = text.lines().map(|l| l.to_string()).collect();
    }

    /// Count of non-translation transcript lines (for pipeline IDs).
    pub fn transcript_count(&self) -> usize {
        self.lines.iter().filter(|l| !l.is_translation).count()
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

    let has_content = !state.lines.is_empty() || state.current_delta.is_some();
    if !has_content {
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
        for line in &state.lines {
            let (prefix_color, text_style) = if line.is_translation {
                let color = source_color(&line.source);
                (
                    color,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                )
            } else {
                let color = source_color(&line.source);
                (color, Style::default().fg(Color::White))
            };
            let prefix = if line.is_translation {
                format!("  → {}: ", line.source.label())
            } else {
                format!("{}: ", line.source.label())
            };
            text_lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(prefix_color)),
                Span::styled(line.text.clone(), text_style),
            ]));
        }
        // Show current delta (partial) dimmed
        if let Some((source, text)) = &state.current_delta {
            text_lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", source.label()),
                    Style::default()
                        .fg(source_color(source))
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    text.clone(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ),
            ]));
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

/// Color for speaker source labels.
fn source_color(source: &AudioSource) -> Color {
    match source {
        AudioSource::Mic => Color::Cyan,
        AudioSource::System => Color::Yellow,
    }
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

        state.push_final_transcript(AudioSource::Mic, "Hello world");
        assert!(state.current_delta.is_none());
        assert_eq!(state.lines.len(), 1);
        assert_eq!(state.lines[0].text, "Hello world");
        assert_eq!(state.lines[0].source, AudioSource::Mic);
        assert!(!state.lines[0].is_translation);
    }

    #[test]
    fn push_translation() {
        let mut state = LiveState::default();
        state.push_final_transcript(AudioSource::System, "Bonjour");
        state.push_translation(AudioSource::System, "Привет");
        assert_eq!(state.lines.len(), 2);
        assert!(!state.lines[0].is_translation);
        assert!(state.lines[1].is_translation);
        assert_eq!(state.lines[1].source, AudioSource::System);
    }

    #[test]
    fn transcript_count() {
        let mut state = LiveState::default();
        state.push_final_transcript(AudioSource::Mic, "Hello");
        state.push_translation(AudioSource::Mic, "Привет");
        state.push_final_transcript(AudioSource::System, "Bonjour");
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
