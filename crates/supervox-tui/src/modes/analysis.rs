use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// State for analysis mode.
pub struct AnalysisState {
    pub file_path: String,
    pub call_id: Option<String>,
    pub raw_transcript: Option<String>,
    pub summary: Option<String>,
    pub action_items: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub mood: Option<String>,
    pub themes: Vec<String>,
    pub follow_up: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub scroll_offset: u16,
}

impl AnalysisState {
    pub fn new(file_path: &str) -> Self {
        Self {
            file_path: file_path.to_string(),
            call_id: None,
            raw_transcript: None,
            summary: None,
            action_items: Vec::new(),
            decisions: Vec::new(),
            open_questions: Vec::new(),
            mood: None,
            themes: Vec::new(),
            follow_up: None,
            loading: false,
            error: None,
            scroll_offset: 0,
        }
    }

    /// Get the raw transcript (if loaded from a call file).
    pub fn get_transcript(&self) -> Option<String> {
        self.raw_transcript.clone()
    }

    pub fn load_from_call(&mut self, file_path: &str) {
        match std::fs::read_to_string(file_path) {
            Ok(json) => match serde_json::from_str::<supervox_agent::types::Call>(&json) {
                Ok(call) => {
                    self.call_id = Some(call.id.clone());
                    self.summary = Some(format!(
                        "Call: {} ({:.0}s, {})",
                        call.created_at.format("%Y-%m-%d %H:%M"),
                        call.duration_secs,
                        call.language.as_deref().unwrap_or("unknown")
                    ));
                    // Store raw transcript for LLM analysis
                    if !call.transcript.is_empty() {
                        self.raw_transcript = Some(call.transcript.clone());
                        self.action_items = vec![format!("Transcript: {}", call.transcript)];
                    }
                    self.loading = false;
                }
                Err(e) => {
                    self.error = Some(format!("Invalid call JSON: {e}"));
                    self.loading = false;
                }
            },
            Err(e) => {
                self.error = Some(format!("Cannot read file: {e}"));
                self.loading = false;
            }
        }
    }

    /// Try to load cached analysis from disk. Returns true if cached analysis was found.
    pub fn try_load_cached(&mut self, calls_dir: &std::path::Path) -> bool {
        let call_id = match &self.call_id {
            Some(id) => id.clone(),
            None => return false,
        };
        match supervox_agent::storage::load_analysis(calls_dir, &call_id) {
            Ok(Some(analysis)) => {
                self.populate_from_analysis(&analysis);
                true
            }
            _ => false,
        }
    }

    /// Populate state fields from a CallAnalysis.
    pub fn populate_from_analysis(&mut self, analysis: &supervox_agent::types::CallAnalysis) {
        self.summary = Some(analysis.summary.clone());
        self.action_items = analysis
            .action_items
            .iter()
            .map(|a| {
                let mut s = a.description.clone();
                if let Some(who) = &a.assignee {
                    s.push_str(&format!(" (@{who})"));
                }
                if let Some(when) = &a.deadline {
                    s.push_str(&format!(" — due {when}"));
                }
                s
            })
            .collect();
        self.decisions = analysis.decisions.clone();
        self.open_questions = analysis.open_questions.clone();
        self.mood = Some(format!("{:?}", analysis.mood));
        self.themes = analysis.themes.clone();
        self.follow_up = analysis.follow_up_draft.clone();
        self.loading = false;
    }
}

/// Render analysis mode — scrollable panel with summary + action items + follow-up.
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.analysis_state;

    if let Some(error) = &state.error {
        let content = Paragraph::new(format!("Error: {error}"))
            .block(
                Block::default()
                    .title(" Call Analysis ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            )
            .style(Style::default().fg(Color::Red));
        f.render_widget(content, area);
        return;
    }

    if state.loading {
        let content = Paragraph::new("Analyzing call...")
            .block(
                Block::default()
                    .title(" Call Analysis ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(content, area);
        return;
    }

    let [main_area, follow_up_area] =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    // Main analysis panel
    let mut lines: Vec<Line> = Vec::new();

    if let Some(summary) = &state.summary {
        lines.push(Line::from(Span::styled(
            summary.as_str(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    if let Some(mood) = &state.mood {
        lines.push(Line::from(format!("Mood: {mood}")));
        lines.push(Line::from(""));
    }

    if !state.themes.is_empty() {
        lines.push(Line::from(Span::styled(
            "Themes:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for theme in &state.themes {
            lines.push(Line::from(format!("  • {theme}")));
        }
        lines.push(Line::from(""));
    }

    if !state.action_items.is_empty() {
        lines.push(Line::from(Span::styled(
            "Action Items:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for item in &state.action_items {
            lines.push(Line::from(format!("  ☐ {item}")));
        }
        lines.push(Line::from(""));
    }

    if !state.decisions.is_empty() {
        lines.push(Line::from(Span::styled(
            "Decisions:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for d in &state.decisions {
            lines.push(Line::from(format!("  ✓ {d}")));
        }
        lines.push(Line::from(""));
    }

    if !state.open_questions.is_empty() {
        lines.push(Line::from(Span::styled(
            "Open Questions:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for q in &state.open_questions {
            lines.push(Line::from(format!("  ? {q}")));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from("No analysis data available."));
    }

    let analysis = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Call Analysis ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: false })
        .scroll((state.scroll_offset, 0));
    f.render_widget(analysis, main_area);

    // Follow-up draft panel
    let follow_up_text = state
        .follow_up
        .as_deref()
        .unwrap_or("No follow-up draft generated. Press 'f' to generate.");

    let follow_up = Paragraph::new(follow_up_text)
        .block(
            Block::default()
                .title(" Follow-up Draft ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));
    f.render_widget(follow_up, follow_up_area);
}

/// Format analysis text for clipboard copy.
fn format_analysis_for_clipboard(state: &AnalysisState) -> String {
    let mut parts = Vec::new();
    if let Some(summary) = &state.summary {
        parts.push(format!("Summary: {summary}"));
    }
    if let Some(mood) = &state.mood {
        parts.push(format!("Mood: {mood}"));
    }
    if !state.themes.is_empty() {
        parts.push(format!("Themes: {}", state.themes.join(", ")));
    }
    if !state.action_items.is_empty() {
        parts.push("Action Items:".into());
        for item in &state.action_items {
            parts.push(format!("  - {item}"));
        }
    }
    if !state.decisions.is_empty() {
        parts.push("Decisions:".into());
        for d in &state.decisions {
            parts.push(format!("  - {d}"));
        }
    }
    if !state.open_questions.is_empty() {
        parts.push("Open Questions:".into());
        for q in &state.open_questions {
            parts.push(format!("  - {q}"));
        }
    }
    parts.join("\n")
}

/// Handle key events in analysis mode.
pub fn handle_key(app: &mut App, key: KeyEvent) {
    match key.code {
        crossterm::event::KeyCode::Up => {
            app.analysis_state.scroll_offset = app.analysis_state.scroll_offset.saturating_sub(1);
        }
        crossterm::event::KeyCode::Down => {
            app.analysis_state.scroll_offset += 1;
        }
        crossterm::event::KeyCode::PageUp => {
            app.analysis_state.scroll_offset = app.analysis_state.scroll_offset.saturating_sub(10);
        }
        crossterm::event::KeyCode::PageDown => {
            app.analysis_state.scroll_offset += 10;
        }
        crossterm::event::KeyCode::Char('f') => {
            // Generate follow-up email via LLM
            if app.analysis_state.summary.is_some() && !app.analysis_state.loading {
                app.status = "Generating follow-up...".into();
                app.spawn_follow_up();
            }
        }
        crossterm::event::KeyCode::Char('c') => {
            // Copy formatted analysis text to clipboard
            let text = format_analysis_for_clipboard(&app.analysis_state);
            if !text.is_empty() {
                match crate::clipboard::copy_to_clipboard(&text) {
                    Ok(()) => app.status = "Copied to clipboard".into(),
                    Err(e) => app.status = format!("Copy failed: {e}"),
                }
            }
        }
        crossterm::event::KeyCode::Char('C') => {
            // Copy follow-up text to clipboard
            if let Some(follow_up) = &app.analysis_state.follow_up {
                match crate::clipboard::copy_to_clipboard(follow_up) {
                    Ok(()) => app.status = "Follow-up copied".into(),
                    Err(e) => app.status = format!("Copy failed: {e}"),
                }
            } else {
                app.status = "No follow-up to copy".into();
            }
        }
        crossterm::event::KeyCode::Char('e') => {
            // Export call + analysis as markdown to clipboard
            let md = build_export_markdown(&app.analysis_state);
            if !md.is_empty() {
                match crate::clipboard::copy_to_clipboard(&md) {
                    Ok(()) => app.status = "Analysis exported to clipboard (markdown)".into(),
                    Err(e) => app.status = format!("Export failed: {e}"),
                }
            }
        }
        crossterm::event::KeyCode::Char('h') => {
            crate::app::open_history(app);
        }
        _ => {}
    }
}

/// Build markdown export from current analysis state.
fn build_export_markdown(state: &AnalysisState) -> String {
    // Try to load the original call for full context
    let call = if !state.file_path.is_empty() {
        std::fs::read_to_string(&state.file_path)
            .ok()
            .and_then(|json| serde_json::from_str::<supervox_agent::types::Call>(&json).ok())
    } else {
        None
    };

    // Build a CallAnalysis from current state
    let analysis = if state.summary.is_some() || !state.action_items.is_empty() {
        Some(supervox_agent::types::CallAnalysis {
            summary: state.summary.clone().unwrap_or_default(),
            action_items: state
                .action_items
                .iter()
                .map(|d| supervox_agent::types::ActionItem {
                    description: d.clone(),
                    assignee: None,
                    deadline: None,
                })
                .collect(),
            follow_up_draft: state.follow_up.clone(),
            decisions: state.decisions.clone(),
            open_questions: state.open_questions.clone(),
            mood: state
                .mood
                .as_deref()
                .and_then(|m| serde_json::from_str(&format!("\"{m}\"")).ok())
                .unwrap_or(supervox_agent::types::Mood::Neutral),
            themes: state.themes.clone(),
        })
    } else {
        None
    };

    match call {
        Some(c) => supervox_agent::storage::export_call_markdown(&c, analysis.as_ref()),
        None => format_analysis_for_clipboard(state),
    }
}
