use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use sgr_agent_tui::{init_terminal, restore_terminal, setup_panic_hook};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::audio::{AudioEvent, AudioPipeline, AudioSource};
use crate::modes;
use supervox_agent::types::{CallAnalysis, Config};

/// Async events from background tasks (analysis, follow-up, agent).
pub enum AppEvent {
    AnalysisReady(CallAnalysis),
    AnalysisError(String),
    FollowUpReady(String),
    FollowUpError(String),
    AgentChunk(String),
    AgentDone,
    AgentError(String),
}

/// Application mode.
#[derive(Debug, Clone)]
pub enum Mode {
    Live,
    Analysis { file: String },
    Agent,
    History { return_to: Box<Mode> },
}

/// Application state.
pub struct App {
    pub mode: Mode,
    pub running: bool,
    pub status: String,
    pub config: Config,
    pub live_state: modes::live::LiveState,
    pub analysis_state: modes::analysis::AnalysisState,
    pub agent_state: modes::agent::AgentState,
    pub history_state: Option<modes::history::CallHistoryState>,
    pub show_help: bool,
    /// Timed status message (text, when set). Clears after 5 seconds.
    pub status_error: Option<(String, std::time::Instant)>,
    pub audio: AudioPipeline,
    pub audio_event_rx: mpsc::UnboundedReceiver<AudioEvent>,
    pub audio_event_tx: mpsc::UnboundedSender<AudioEvent>,
    /// Sender for translation pipeline (source_id, text).
    pub translate_tx: Option<mpsc::UnboundedSender<(String, String)>>,
    /// Sender for summary pipeline (final transcript text).
    pub summary_tx: Option<mpsc::UnboundedSender<String>>,
    /// Channel for async app events (analysis, follow-up, agent).
    pub app_event_rx: mpsc::UnboundedReceiver<AppEvent>,
    pub app_event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl App {
    pub fn new(mode: Mode, config: Config) -> Self {
        let status = match &mode {
            Mode::Live => "Live mode — press 'r' to start recording".into(),
            Mode::Analysis { file } => format!("Analysis mode — {file}"),
            Mode::Agent => "Agent mode — type a question".into(),
            Mode::History { .. } => "Call history".into(),
        };

        let analysis_file = match &mode {
            Mode::Analysis { file } => file.clone(),
            _ => String::new(),
        };

        let (audio_event_tx, audio_event_rx) = mpsc::unbounded_channel();
        let (app_event_tx, app_event_rx) = mpsc::unbounded_channel();
        let mut app = Self {
            mode,
            running: true,
            status,
            config,
            live_state: modes::live::LiveState::default(),
            analysis_state: modes::analysis::AnalysisState::new(&analysis_file),
            agent_state: modes::agent::AgentState::default(),
            history_state: None,
            show_help: false,
            status_error: None,
            audio: AudioPipeline::default(),
            audio_event_rx,
            audio_event_tx,
            translate_tx: None,
            summary_tx: None,
            app_event_rx,
            app_event_tx,
        };

        // Load call file for analysis mode — use cached analysis or trigger LLM
        if !analysis_file.is_empty() {
            app.analysis_state.load_from_call(&analysis_file);
            let calls_dir = supervox_agent::storage::default_calls_dir();
            if !app.analysis_state.try_load_cached(&calls_dir)
                && let Some(transcript) = app.analysis_state.get_transcript()
            {
                app.analysis_state.loading = true;
                app.spawn_analysis(transcript);
            }
        }

        app
    }

    fn mode_label(&self) -> &str {
        match &self.mode {
            Mode::Live => "LIVE",
            Mode::Analysis { .. } => "ANALYSIS",
            Mode::Agent => "AGENT",
            Mode::History { .. } => "HISTORY",
        }
    }

    /// Spawn async LLM analysis for the given transcript.
    pub fn spawn_analysis(&self, transcript: String) {
        let model = self.config.effective_model().to_string();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match crate::analysis_pipeline::analyze_transcript(&transcript, &model).await {
                Ok(analysis) => {
                    let _ = tx.send(AppEvent::AnalysisReady(analysis));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::AnalysisError(e));
                }
            }
        });
    }

    /// Spawn async follow-up draft generation.
    pub fn spawn_follow_up(&self) {
        let analysis = CallAnalysis {
            summary: self.analysis_state.summary.clone().unwrap_or_default(),
            action_items: self
                .analysis_state
                .action_items
                .iter()
                .map(|d| supervox_agent::types::ActionItem {
                    description: d.clone(),
                    assignee: None,
                    deadline: None,
                })
                .collect(),
            follow_up_draft: None,
            decisions: self.analysis_state.decisions.clone(),
            open_questions: self.analysis_state.open_questions.clone(),
            mood: supervox_agent::types::Mood::Neutral,
            themes: self.analysis_state.themes.clone(),
        };
        let analysis_json = serde_json::to_string(&analysis).unwrap_or_default();
        let language = self.config.my_language.clone();
        let model = self.config.effective_model().to_string();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match crate::analysis_pipeline::draft_follow_up(&analysis_json, &language, &model).await
            {
                Ok(text) => {
                    let _ = tx.send(AppEvent::FollowUpReady(text));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::FollowUpError(e));
                }
            }
        });
    }

    fn process_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::AnalysisReady(analysis) => {
                // Persist analysis and update call tags
                if let Some(call_id) = &self.analysis_state.call_id {
                    let calls_dir = supervox_agent::storage::default_calls_dir();
                    if let Err(e) =
                        supervox_agent::storage::save_analysis(&calls_dir, call_id, &analysis)
                    {
                        tracing::warn!("Failed to save analysis: {e}");
                    }
                    if let Err(e) = supervox_agent::storage::update_call_tags(
                        &calls_dir,
                        call_id,
                        &analysis.themes,
                    ) {
                        tracing::warn!("Failed to update call tags: {e}");
                    }
                }
                self.analysis_state.populate_from_analysis(&analysis);
                self.status = "Analysis complete".into();
            }
            AppEvent::AnalysisError(e) => {
                self.analysis_state.error = Some(e.clone());
                self.analysis_state.loading = false;
                self.status_error =
                    Some((format!("Analysis error: {e}"), std::time::Instant::now()));
            }
            AppEvent::FollowUpReady(text) => {
                self.analysis_state.follow_up = Some(text);
                self.status = "Follow-up draft ready".into();
            }
            AppEvent::FollowUpError(e) => {
                self.status_error =
                    Some((format!("Follow-up error: {e}"), std::time::Instant::now()));
            }
            AppEvent::AgentChunk(text) => {
                self.agent_state.push_assistant_chunk(&text);
            }
            AppEvent::AgentDone => {
                self.agent_state.finish_response();
            }
            AppEvent::AgentError(e) => {
                self.agent_state.push_error(&e);
                self.status_error = Some((format!("Agent error: {e}"), std::time::Instant::now()));
            }
        }
    }

    fn process_audio_event(&mut self, event: AudioEvent) {
        match event {
            AudioEvent::Level(level) => {
                self.live_state.audio_level = level;
            }
            AudioEvent::Transcript {
                source,
                text,
                is_final,
            } => {
                if is_final {
                    self.live_state.push_final_transcript(source.clone(), &text);
                    // Feed into translation + summary pipelines
                    if !text.is_empty() {
                        if let Some(tx) = &self.translate_tx {
                            let _ = tx.send((
                                format!(
                                    "{}-{}",
                                    source.label(),
                                    self.live_state.transcript_count()
                                ),
                                text.clone(),
                            ));
                        }
                        if let Some(tx) = &self.summary_tx {
                            let _ = tx.send(format!("{}: {}", source.label(), text));
                        }
                    }
                } else {
                    self.live_state.update_delta(source, &text);
                }
            }
            AudioEvent::Translation { source_id, text } => {
                // Parse source from source_id (format: "You-N" or "Them-N")
                let source = if source_id.starts_with("You") {
                    AudioSource::Mic
                } else {
                    AudioSource::System
                };
                self.live_state.push_translation(source, &text);
            }
            AudioEvent::Summary(text) => {
                self.live_state.set_summary(&text);
            }
            AudioEvent::Error(e) => {
                self.status = format!("Error: {e}");
            }
            AudioEvent::Stopped {
                transcript,
                duration_secs,
                audio_path,
            } => {
                self.live_state.stop_recording();
                let calls_dir = supervox_agent::storage::default_calls_dir();
                match crate::audio::save_recorded_call(
                    &transcript,
                    duration_secs,
                    &calls_dir,
                    audio_path.as_deref(),
                ) {
                    Ok(_call_id) => {
                        if transcript.is_empty() {
                            self.status = "Recording stopped (no speech detected)".into();
                        } else {
                            self.status = format!(
                                "Call saved ({:.0}s) — switching to Analysis",
                                duration_secs
                            );
                            // Auto-flow: switch to Analysis mode + trigger LLM analysis
                            let call_file = crate::audio::last_saved_call_path(&calls_dir);
                            if let Some(path) = call_file {
                                let file = path.to_string_lossy().to_string();
                                self.analysis_state = modes::analysis::AnalysisState::new(&file);
                                self.analysis_state.load_from_call(&file);
                                self.analysis_state.loading = true;
                                self.mode = Mode::Analysis { file };
                                // Spawn LLM analysis with the transcript
                                self.spawn_analysis(transcript.clone());
                            }
                        }
                    }
                    Err(e) => {
                        self.status = format!("Save error: {e}");
                    }
                }
            }
        }
    }
}

/// Run the TUI application.
pub async fn run(mode: Mode) -> Result<()> {
    setup_panic_hook();
    let mut terminal = init_terminal()?;

    let config_path = supervox_agent::storage::default_config_path();
    let config = supervox_agent::storage::load_config(&config_path)
        .map_err(|e| anyhow::anyhow!("Config error: {e}"))?;

    let mut app = App::new(mode, config);

    while app.running {
        terminal.draw(|f| {
            let area = f.area();

            match &app.mode {
                Mode::Live => {
                    modes::live::render(f, area, &app.live_state);
                }
                _ => {
                    let [main_area, status_area] =
                        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

                    match &app.mode {
                        Mode::Analysis { .. } => modes::analysis::render(f, main_area, &app),
                        Mode::Agent => modes::agent::render(f, main_area, &app),
                        Mode::History { .. } => {
                            if let Some(hs) = &app.history_state {
                                modes::history::render(f, main_area, hs);
                                modes::history::render_tag_filter(f, main_area, hs);
                            }
                        }
                        Mode::Live => unreachable!(),
                    }

                    // Show error in red if active, otherwise normal status
                    let (status_text, status_style) = if let Some((err, when)) = &app.status_error {
                        if when.elapsed().as_secs() < 5 {
                            (
                                format!(" [{}] {} ", app.mode_label(), err),
                                Style::default().fg(Color::White).bg(Color::Red),
                            )
                        } else {
                            (
                                format!(" [{}] {} | ?=help q=quit", app.mode_label(), app.status),
                                Style::default().fg(Color::White).bg(Color::DarkGray),
                            )
                        }
                    } else {
                        (
                            format!(" [{}] {} | ?=help q=quit", app.mode_label(), app.status),
                            Style::default().fg(Color::White).bg(Color::DarkGray),
                        )
                    };
                    let status = Paragraph::new(Line::from(status_text)).style(status_style);
                    f.render_widget(status, status_area);
                }
            }

            // Help overlay on top of everything
            if app.show_help {
                crate::help::render_help(f, area, app.mode_label());
            }
        })?;

        // Auto-clear expired status errors
        if let Some((_, when)) = &app.status_error
            && when.elapsed().as_secs() >= 5
        {
            app.status_error = None;
        }

        // Process audio events (non-blocking)
        while let Ok(event) = app.audio_event_rx.try_recv() {
            app.process_audio_event(event);
        }

        // Process app events (analysis, follow-up, agent)
        while let Ok(event) = app.app_event_rx.try_recv() {
            app.process_app_event(event);
        }

        // Poll terminal events
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            // Ctrl+C always quits
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                app.audio.stop();
                app.running = false;
                continue;
            }

            // Any key dismisses help overlay
            if app.show_help {
                app.show_help = false;
                continue;
            }

            // `?` toggles help overlay
            if key.code == KeyCode::Char('?') {
                app.show_help = true;
                continue;
            }

            // Esc in History returns to previous mode; elsewhere quits
            if key.code == KeyCode::Esc {
                if let Mode::History { return_to } = &app.mode {
                    let return_mode = return_to.clone();
                    app.mode = *return_mode;
                    app.history_state = None;
                    app.status = "Back".into();
                } else if !app.live_state.is_recording {
                    app.running = false;
                }
                continue;
            }

            // Mode-specific handling
            match &app.mode {
                Mode::Live => {
                    // q quits only when not recording
                    if key.code == KeyCode::Char('q') && !app.live_state.is_recording {
                        app.running = false;
                    } else {
                        handle_live_key(&mut app, key);
                    }
                }
                Mode::Analysis { .. } => {
                    if key.code == KeyCode::Char('q') {
                        app.running = false;
                    } else {
                        modes::analysis::handle_key(&mut app, key);
                    }
                }
                Mode::Agent => {
                    // In agent mode, all keys go to input handler (no 'q' quit)
                    modes::agent::handle_key(&mut app, key);
                }
                Mode::History { .. } => {
                    handle_history_key(&mut app, key);
                }
            }
        }
    }

    restore_terminal()?;
    Ok(())
}

pub fn open_history(app: &mut App) {
    let calls_dir = supervox_agent::storage::default_calls_dir();
    let calls = supervox_agent::storage::list_calls(&calls_dir).unwrap_or_default();
    let analyzed_ids: std::collections::HashSet<String> = calls
        .iter()
        .filter(|c| {
            supervox_agent::storage::load_analysis(&calls_dir, &c.id)
                .ok()
                .flatten()
                .is_some()
        })
        .map(|c| c.id.clone())
        .collect();
    app.history_state =
        Some(modes::history::CallHistoryState::new(calls).with_analyzed_ids(analyzed_ids));
    let return_mode = app.mode.clone();
    app.mode = Mode::History {
        return_to: Box::new(return_mode),
    };
    app.status = "Call history".into();
}

fn handle_history_key(app: &mut App, key: crossterm::event::KeyEvent) {
    // Handle tag filter popup keys
    if let Some(hs) = &mut app.history_state
        && hs.show_tag_filter
    {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if hs.tag_cursor > 0 {
                    hs.tag_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if hs.tag_cursor + 1 < hs.available_tags.len() {
                    hs.tag_cursor += 1;
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(tc) = hs.available_tags.get(hs.tag_cursor).cloned() {
                    hs.toggle_tag(&tc.theme);
                    let count = hs.active_tags.len();
                    app.status = if count > 0 {
                        format!("{count} tag filter(s) active")
                    } else {
                        "Filters cleared".into()
                    };
                }
            }
            KeyCode::Char('t') | KeyCode::Esc => {
                hs.show_tag_filter = false;
            }
            _ => {}
        }
        return;
    }

    // Handle delete confirmation state first
    if let Some(hs) = &mut app.history_state
        && hs.confirm_delete
    {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let call_id = hs.selected().map(|c| c.id.clone());
                if let Some(id) = call_id {
                    let calls_dir = supervox_agent::storage::default_calls_dir();
                    match supervox_agent::storage::delete_call(&calls_dir, &id) {
                        Ok(()) => {
                            hs.calls.remove(hs.cursor);
                            if hs.cursor > 0 && hs.cursor >= hs.calls.len() {
                                hs.cursor = hs.calls.len().saturating_sub(1);
                            }
                            app.status = format!("Deleted call {id}");
                        }
                        Err(e) => {
                            app.status = format!("Delete failed: {e}");
                        }
                    }
                }
                hs.confirm_delete = false;
            }
            _ => {
                // Any other key cancels
                hs.confirm_delete = false;
                app.status = "Delete cancelled".into();
            }
        }
        return;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(hs) = &mut app.history_state {
                hs.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(hs) = &mut app.history_state {
                hs.move_down();
            }
        }
        KeyCode::Char('t') => {
            if let Some(hs) = &mut app.history_state
                && !hs.available_tags.is_empty()
            {
                hs.show_tag_filter = true;
                hs.tag_cursor = 0;
            }
        }
        KeyCode::Char('d') => {
            if let Some(hs) = &mut app.history_state
                && !hs.calls.is_empty()
            {
                hs.confirm_delete = true;
                app.status = "Delete this call? Press 'y' to confirm".into();
            }
        }
        KeyCode::Enter => {
            // Open selected call in analysis mode
            if let Some(hs) = &app.history_state
                && let Some(call) = hs.selected()
            {
                let calls_dir = supervox_agent::storage::default_calls_dir();
                let file = calls_dir
                    .join(format!("{}.json", call.id))
                    .to_string_lossy()
                    .to_string();
                app.analysis_state = modes::analysis::AnalysisState::new(&file);
                app.analysis_state.load_from_call(&file);
                app.mode = Mode::Analysis { file: file.clone() };
                app.history_state = None;
                // Use cached analysis or spawn LLM
                if !app.analysis_state.try_load_cached(&calls_dir)
                    && let Some(transcript) = app.analysis_state.get_transcript()
                {
                    app.analysis_state.loading = true;
                    app.spawn_analysis(transcript);
                }
            }
        }
        KeyCode::Char('q') => {
            app.running = false;
        }
        _ => {}
    }
}

fn handle_live_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('h') if !app.live_state.is_recording => {
            open_history(app);
        }
        KeyCode::Char('r') if !app.live_state.is_recording => {
            let tx = app.audio_event_tx.clone();
            let calls_dir = supervox_agent::storage::default_calls_dir();
            match app.audio.start(tx, &app.config, calls_dir) {
                Ok(()) => {
                    app.live_state.start_recording();
                    app.status = "Recording...".into();

                    // Start translation pipeline
                    let (tr_tx, tr_rx) = mpsc::unbounded_channel();
                    crate::intelligence::start_translation_pipeline(
                        &app.config,
                        tr_rx,
                        app.audio_event_tx.clone(),
                    );
                    app.translate_tx = Some(tr_tx);

                    // Start summary pipeline
                    let (sum_tx, sum_rx) = mpsc::unbounded_channel();
                    crate::intelligence::start_summary_pipeline(
                        &app.config,
                        sum_rx,
                        app.audio_event_tx.clone(),
                    );
                    app.summary_tx = Some(sum_tx);
                }
                Err(e) => {
                    app.status = format!("Mic error: {e}");
                }
            }
        }
        KeyCode::Char('s') if app.live_state.is_recording => {
            app.audio.stop();
            // Drop intelligence pipeline senders to stop background tasks
            app.translate_tx = None;
            app.summary_tx = None;
            app.status = "Stopping...".into();
        }
        _ => {}
    }
}
