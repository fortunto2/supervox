use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use sgr_agent_tui::{init_terminal, restore_terminal, setup_panic_hook};
use std::time::Duration;

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
}

impl App {
    pub fn new(mode: Mode) -> Self {
        let status = match &mode {
            Mode::Live => "Live mode — press 'r' to start recording".into(),
            Mode::Analysis { file } => format!("Analysis mode — {file}"),
            Mode::Agent => "Agent mode — type a question".into(),
        };
        Self {
            mode,
            running: true,
            status,
            live_state: modes::live::LiveState::default(),
        }
    }

    fn mode_label(&self) -> &str {
        match &self.mode {
            Mode::Live => "LIVE",
            Mode::Analysis { .. } => "ANALYSIS",
            Mode::Agent => "AGENT",
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
                    // Live mode handles its own status bar
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

                    // Status bar for non-live modes
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

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match (key.modifiers, key.code) {
                (_, KeyCode::Char('q')) if !app.live_state.is_recording => {
                    app.running = false;
                }
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                    app.running = false;
                }
                _ => match &app.mode {
                    Mode::Live => modes::live::handle_key(&mut app.live_state, key),
                    Mode::Analysis { .. } => modes::analysis::handle_key(&mut app, key),
                    Mode::Agent => modes::agent::handle_key(&mut app, key),
                },
            }
        }
    }

    restore_terminal()?;
    Ok(())
}
