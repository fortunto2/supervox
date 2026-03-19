use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render live mode — left panel (transcript), right panel (summary), status.
pub fn render(f: &mut Frame, area: Rect, _app: &App) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    let transcript = Paragraph::new("Waiting for audio...")
        .block(
            Block::default()
                .title(" Transcript ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(transcript, left);

    let summary = Paragraph::new("Summary will appear here")
        .block(
            Block::default()
                .title(" Rolling Summary ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().fg(Color::Gray));
    f.render_widget(summary, right);
}

/// Handle key events in live mode.
pub fn handle_key(app: &mut App, key: KeyEvent) {
    match key.code {
        crossterm::event::KeyCode::Char('r') => {
            app.status = "Recording started...".into();
        }
        crossterm::event::KeyCode::Char('s') => {
            app.status = "Recording stopped.".into();
        }
        _ => {}
    }
}
