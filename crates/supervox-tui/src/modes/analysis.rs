use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render analysis mode — scrollable panel with summary + action items.
pub fn render(f: &mut Frame, area: Rect, _app: &App) {
    let content = Paragraph::new("Loading analysis...")
        .block(
            Block::default()
                .title(" Call Analysis ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(content, area);
}

/// Handle key events in analysis mode.
pub fn handle_key(_app: &mut App, _key: KeyEvent) {
    // Phase 5: scroll, copy follow-up
}
