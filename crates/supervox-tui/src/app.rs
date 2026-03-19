use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use sgr_agent_tui::{init_terminal, restore_terminal, setup_panic_hook};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::audio::{AudioEvent, AudioPipeline};
use crate::modes;

/// Application mode.
#[derive(Debug, Clone)]
pub enum Mode {
    Live,
    Analysis { file: String },
    Agent,
}

/// Application state.
pub struct App {
    pub mode: Mode,
    pub running: bool,
    pub status: String,
    pub live_state: modes::live::LiveState,
    pub audio: AudioPipeline,
    pub audio_event_rx: mpsc::UnboundedReceiver<AudioEvent>,
    pub audio_event_tx: mpsc::UnboundedSender<AudioEvent>,
}

impl App {
    pub fn new(mode: Mode) -> Self {
        let status = match &mode {
            Mode::Live => "Live mode — press 'r' to start recording".into(),
            Mode::Analysis { file } => format!("Analysis mode — {file}"),
            Mode::Agent => "Agent mode — type a question".into(),
        };
        let (audio_event_tx, audio_event_rx) = mpsc::unbounded_channel();
        Self {
            mode,
            running: true,
            status,
            live_state: modes::live::LiveState::default(),
            audio: AudioPipeline::default(),
            audio_event_rx,
            audio_event_tx,
        }
    }

    fn mode_label(&self) -> &str {
        match &self.mode {
            Mode::Live => "LIVE",
            Mode::Analysis { .. } => "ANALYSIS",
            Mode::Agent => "AGENT",
        }
    }

    fn process_audio_event(&mut self, event: AudioEvent) {
        match event {
            AudioEvent::Level(level) => {
                self.live_state.audio_level = level;
            }
            AudioEvent::Transcript(text) => {
                self.live_state.push_transcript(&text);
            }
            AudioEvent::Error(e) => {
                self.status = format!("Error: {e}");
            }
            AudioEvent::Stopped {
                transcript,
                duration_secs,
            } => {
                self.live_state.stop_recording();
                let calls_dir = supervox_agent::storage::default_calls_dir();
                match crate::audio::save_recorded_call(&transcript, duration_secs, &calls_dir) {
                    Ok(()) => {
                        if transcript.is_empty() {
                            self.status = "Recording stopped (no speech detected)".into();
                        } else {
                            self.status = format!("Call saved ({:.0}s)", duration_secs);
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
    let mut app = App::new(mode);

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
                        Mode::Live => unreachable!(),
                    }

                    let status = Paragraph::new(Line::from(format!(
                        " [{}] {} | q=quit",
                        app.mode_label(),
                        app.status
                    )))
                    .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                    f.render_widget(status, status_area);
                }
            }
        })?;

        // Process audio events (non-blocking)
        while let Ok(event) = app.audio_event_rx.try_recv() {
            app.process_audio_event(event);
        }

        // Poll terminal events
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('q')) if !app.live_state.is_recording => {
                    app.running = false;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                    app.audio.stop();
                    app.running = false;
                }
                _ => match &app.mode {
                    Mode::Live => {
                        handle_live_key(&mut app, key);
                    }
                    Mode::Analysis { .. } => modes::analysis::handle_key(&mut app, key),
                    Mode::Agent => modes::agent::handle_key(&mut app, key),
                },
            }
        }
    }

    restore_terminal()?;
    Ok(())
}

fn handle_live_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('r') if !app.live_state.is_recording => {
            let tx = app.audio_event_tx.clone();
            match app.audio.start(tx) {
                Ok(()) => {
                    app.live_state.start_recording();
                    app.status = "Recording...".into();
                }
                Err(e) => {
                    app.status = format!("Mic error: {e}");
                }
            }
        }
        KeyCode::Char('s') if app.live_state.is_recording => {
            app.audio.stop();
            app.status = "Stopping...".into();
        }
        _ => {}
    }
}
