use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render agent mode — chat panel for Q&A.
pub fn render(f: &mut Frame, area: Rect, _app: &App) {
    let content = Paragraph::new("Type a question about your calls...")
        .block(
            Block::default()
                .title(" Agent Chat ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(content, area);
}

/// Handle key events in agent mode.
pub fn handle_key(_app: &mut App, _key: KeyEvent) {
    // Phase 5: input handling, send to agent
}
