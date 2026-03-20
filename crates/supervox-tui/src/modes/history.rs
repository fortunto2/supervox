use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use supervox_agent::types::Call;

/// State for call history browser.
pub struct CallHistoryState {
    pub calls: Vec<Call>,
    pub cursor: usize,
    pub scroll_offset: usize,
    /// When true, waiting for y/n confirmation to delete selected call.
    pub confirm_delete: bool,
}

impl CallHistoryState {
    pub fn new(calls: Vec<Call>) -> Self {
        Self {
            calls,
            cursor: 0,
            scroll_offset: 0,
            confirm_delete: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            if self.cursor < self.scroll_offset {
                self.scroll_offset = self.cursor;
            }
        }
    }

    pub fn move_down(&mut self) {
        if !self.calls.is_empty() && self.cursor < self.calls.len() - 1 {
            self.cursor += 1;
        }
    }

    pub fn selected(&self) -> Option<&Call> {
        self.calls.get(self.cursor)
    }
}

/// Render call history browser.
pub fn render(f: &mut Frame, area: Rect, state: &CallHistoryState) {
    let [list_area, detail_area] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).areas(area);

    let visible_height = list_area.height.saturating_sub(2) as usize; // subtract borders
    let scroll = if state.cursor >= state.scroll_offset + visible_height {
        state.cursor.saturating_sub(visible_height - 1)
    } else {
        state.scroll_offset
    };

    let items: Vec<ListItem> = state
        .calls
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, call)| {
            let date = call.created_at.format("%Y-%m-%d %H:%M").to_string();
            let duration = format_duration(call.duration_secs);
            let preview = call
                .transcript
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect::<String>();

            let style = if i == state.cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {date} "), style),
                Span::styled(format!(" {duration} "), style),
                Span::styled(format!(" {preview}"), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(format!(" Call History ({}) ", state.calls.len()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(list, list_area);

    // Detail bar
    let detail_text = if state.confirm_delete {
        "Delete this call? (y/n)".to_string()
    } else if state.calls.is_empty() {
        "No calls recorded yet".to_string()
    } else {
        "↑/↓/j/k = navigate  Enter = open  d = delete  Esc = back".to_string()
    };
    let detail = Paragraph::new(Line::from(detail_text))
        .style(Style::default().fg(Color::DarkGray))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(detail, detail_area);
}

fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    let m = total / 60;
    let s = total % 60;
    format!("{m}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_call(id: &str, transcript: &str) -> Call {
        Call {
            id: id.to_string(),
            created_at: Utc::now(),
            duration_secs: 120.0,
            participants: vec![],
            language: Some("en".into()),
            transcript: transcript.to_string(),
            translation: None,
            tags: vec![],
        }
    }

    #[test]
    fn cursor_bounds_empty() {
        let mut state = CallHistoryState::new(vec![]);
        state.move_up();
        assert_eq!(state.cursor, 0);
        state.move_down();
        assert_eq!(state.cursor, 0);
        assert!(state.selected().is_none());
    }

    #[test]
    fn cursor_navigation() {
        let calls = vec![
            make_call("1", "Hello"),
            make_call("2", "World"),
            make_call("3", "Foo"),
        ];
        let mut state = CallHistoryState::new(calls);
        assert_eq!(state.cursor, 0);

        state.move_down();
        assert_eq!(state.cursor, 1);
        state.move_down();
        assert_eq!(state.cursor, 2);
        // Can't go past end
        state.move_down();
        assert_eq!(state.cursor, 2);

        state.move_up();
        assert_eq!(state.cursor, 1);
        state.move_up();
        assert_eq!(state.cursor, 0);
        // Can't go below 0
        state.move_up();
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn selected_returns_correct_call() {
        let calls = vec![make_call("a", "First"), make_call("b", "Second")];
        let mut state = CallHistoryState::new(calls);
        assert_eq!(state.selected().unwrap().id, "a");
        state.move_down();
        assert_eq!(state.selected().unwrap().id, "b");
    }

    #[test]
    fn format_duration_works() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(65.0), "1:05");
        assert_eq!(format_duration(3600.0), "60:00");
    }

    #[test]
    fn confirm_delete_initial_state() {
        let state = CallHistoryState::new(vec![make_call("1", "Hello")]);
        assert!(!state.confirm_delete);
    }

    #[test]
    fn confirm_delete_toggle() {
        let mut state = CallHistoryState::new(vec![make_call("1", "Hello")]);
        state.confirm_delete = true;
        assert!(state.confirm_delete);
        state.confirm_delete = false;
        assert!(!state.confirm_delete);
    }

    #[test]
    fn confirm_delete_preserves_cursor() {
        let calls = vec![
            make_call("1", "A"),
            make_call("2", "B"),
            make_call("3", "C"),
        ];
        let mut state = CallHistoryState::new(calls);
        state.move_down(); // cursor at 1
        state.confirm_delete = true;
        assert_eq!(state.cursor, 1);
        assert_eq!(state.selected().unwrap().id, "2");
    }
}
