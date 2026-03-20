use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use std::collections::HashSet;
use supervox_agent::types::{Call, ThemeCount};

/// State for call history browser.
pub struct CallHistoryState {
    /// Filtered view of calls (displayed in the list).
    pub calls: Vec<Call>,
    /// All loaded calls (unfiltered).
    pub all_calls: Vec<Call>,
    pub cursor: usize,
    pub scroll_offset: usize,
    /// When true, waiting for y/n confirmation to delete selected call.
    pub confirm_delete: bool,
    /// IDs of calls that have cached analysis.
    pub analyzed_ids: HashSet<String>,
    /// Currently active tag filters.
    pub active_tags: HashSet<String>,
    /// All available tags with counts.
    pub available_tags: Vec<ThemeCount>,
    /// Whether the tag filter popup is visible.
    pub show_tag_filter: bool,
    /// Cursor position in the tag filter popup.
    pub tag_cursor: usize,
}

impl CallHistoryState {
    pub fn new(calls: Vec<Call>) -> Self {
        let available_tags = supervox_agent::storage::collect_tags(&calls);
        Self {
            all_calls: calls.clone(),
            calls,
            cursor: 0,
            scroll_offset: 0,
            confirm_delete: false,
            analyzed_ids: HashSet::new(),
            active_tags: HashSet::new(),
            available_tags,
            show_tag_filter: false,
            tag_cursor: 0,
        }
    }

    pub fn with_analyzed_ids(mut self, ids: HashSet<String>) -> Self {
        self.analyzed_ids = ids;
        self
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

    /// Rebuild `calls` from `all_calls` based on `active_tags`.
    pub fn apply_filter(&mut self) {
        if self.active_tags.is_empty() {
            self.calls = self.all_calls.clone();
        } else {
            self.calls = self
                .all_calls
                .iter()
                .filter(|call| {
                    call.tags
                        .iter()
                        .any(|t| self.active_tags.contains(&t.to_lowercase()))
                })
                .cloned()
                .collect();
        }
        // Reset cursor if it's out of bounds
        if self.cursor >= self.calls.len() {
            self.cursor = self.calls.len().saturating_sub(1);
        }
        self.scroll_offset = 0;
    }

    /// Toggle a tag in the active filter set and re-apply.
    pub fn toggle_tag(&mut self, tag: &str) {
        let key = tag.to_lowercase();
        if self.active_tags.contains(&key) {
            self.active_tags.remove(&key);
        } else {
            self.active_tags.insert(key);
        }
        self.apply_filter();
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

            let indicator = if state.analyzed_ids.contains(&call.id) {
                "\u{2713}"
            } else {
                "\u{2717}"
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {date} "), style),
                Span::styled(format!(" {duration} "), style),
                Span::styled(format!(" {indicator} "), style),
                Span::styled(format!(" {preview}"), style),
            ]))
        })
        .collect();

    let total_secs: u64 = state.calls.iter().map(|c| c.duration_secs as u64).sum();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let filtered = !state.active_tags.is_empty();
    let title = if filtered {
        format!(
            " Call History ({}/{} calls, filtered) ",
            state.calls.len(),
            state.all_calls.len()
        )
    } else if total_secs > 0 {
        format!(
            " Call History ({} calls, {hours}h {mins}m total) ",
            state.calls.len()
        )
    } else {
        format!(" Call History ({}) ", state.calls.len())
    };

    let list = List::new(items).block(
        Block::default()
            .title(title)
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
        "↑/↓/j/k = navigate  Enter = open  d = delete  t = filter tags  Esc = back".to_string()
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

/// Render tag filter popup overlay.
pub fn render_tag_filter(f: &mut Frame, area: Rect, state: &CallHistoryState) {
    if !state.show_tag_filter || state.available_tags.is_empty() {
        return;
    }

    let popup_height = (state.available_tags.len() as u16 + 4).min(area.height.saturating_sub(4));
    let popup_width = 40u16.min(area.width.saturating_sub(4));
    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = state
        .available_tags
        .iter()
        .enumerate()
        .map(|(i, tc)| {
            let is_active = state.active_tags.contains(&tc.theme);
            let check = if is_active { "[x]" } else { "[ ]" };
            let style = if i == state.tag_cursor {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!(" {check} {} ({})", tc.theme, tc.count),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Filter by Tag (space=toggle, t/Esc=close) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(list, popup_area);
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
            audio_path: None,
            bookmarks: vec![],
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
    fn apply_filter_no_tags_passthrough() {
        let mut c1 = make_call("1", "A");
        c1.tags = vec!["meeting".into()];
        let c2 = make_call("2", "B");
        let mut state = CallHistoryState::new(vec![c1, c2]);
        state.apply_filter();
        assert_eq!(state.calls.len(), 2);
    }

    #[test]
    fn apply_filter_with_tags() {
        let mut c1 = make_call("1", "A");
        c1.tags = vec!["meeting".into()];
        let mut c2 = make_call("2", "B");
        c2.tags = vec!["budget".into()];
        let c3 = make_call("3", "C");
        let mut state = CallHistoryState::new(vec![c1, c2, c3]);
        state.active_tags.insert("meeting".into());
        state.apply_filter();
        assert_eq!(state.calls.len(), 1);
        assert_eq!(state.calls[0].id, "1");
    }

    #[test]
    fn toggle_tag_on_off() {
        let mut c1 = make_call("1", "A");
        c1.tags = vec!["meeting".into()];
        let c2 = make_call("2", "B");
        let mut state = CallHistoryState::new(vec![c1, c2]);

        // Toggle on
        state.toggle_tag("meeting");
        assert!(state.active_tags.contains("meeting"));
        assert_eq!(state.calls.len(), 1);

        // Toggle off
        state.toggle_tag("meeting");
        assert!(!state.active_tags.contains("meeting"));
        assert_eq!(state.calls.len(), 2);
    }

    #[test]
    fn apply_filter_resets_cursor() {
        let mut c1 = make_call("1", "A");
        c1.tags = vec!["a".into()];
        let mut c2 = make_call("2", "B");
        c2.tags = vec!["b".into()];
        let mut state = CallHistoryState::new(vec![c1, c2]);
        state.cursor = 1; // at second call
        state.active_tags.insert("a".into());
        state.apply_filter();
        // Only 1 result, cursor should be 0
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn available_tags_populated() {
        let mut c1 = make_call("1", "A");
        c1.tags = vec!["meeting".into(), "budget".into()];
        let mut c2 = make_call("2", "B");
        c2.tags = vec!["meeting".into()];
        let state = CallHistoryState::new(vec![c1, c2]);
        assert!(!state.available_tags.is_empty());
        assert_eq!(state.available_tags[0].theme, "meeting");
        assert_eq!(state.available_tags[0].count, 2);
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
