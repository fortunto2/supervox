use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Per-mode keybinding definitions: (key, description).
const LIVE_KEYS: &[(&str, &str)] = &[
    ("Space", "Start/stop recording"),
    ("r/s", "Start/stop recording"),
    ("Enter", "Add bookmark"),
    ("b", "Add bookmark"),
    ("h", "Call history"),
    ("m", "Next microphone (idle)"),
    ("Tab", "Focus transcript / summary"),
    ("↑↓ j/k", "Scroll focused pane"),
    ("PgUp/PgDn", "Scroll a page"),
    ("Home/g", "Top (stops following)"),
    ("End/G", "Bottom, follow new lines"),
    ("Wheel", "Scroll pane under pointer"),
    ("/ or o", "Sessions browser (recording continues)"),
    ("q", "Quit"),
    ("?", "Toggle help"),
];

const SESSIONS_KEYS: &[(&str, &str)] = &[
    ("type", "Filter (fuzzy) or phrase"),
    ("Tab", "Switch name/date/title ↔ phrase"),
    ("↑↓ PgUp/PgDn", "Select"),
    ("Enter", "Read: transcript + summary"),
    ("Ctrl+U/W", "Clear query / word"),
    ("Esc", "Back"),
    ("— reader —", ""),
    ("Tab ←→", "Focus transcript / summary"),
    ("↑↓ j/k PgUp/Dn", "Scroll focused pane"),
    ("Home/End", "Top / bottom"),
    ("n / N", "Next / previous match"),
    ("Esc q /", "Back to the list"),
];

const ANALYSIS_KEYS: &[(&str, &str)] = &[
    ("f", "Generate follow-up email"),
    ("c", "Copy analysis to clipboard"),
    ("C", "Copy follow-up to clipboard"),
    ("e", "Export as markdown to clipboard"),
    ("h", "Call history"),
    ("↑/↓", "Scroll"),
    ("q", "Quit"),
    ("?", "Toggle help"),
];

const AGENT_KEYS: &[(&str, &str)] = &[
    ("Enter", "Send message"),
    ("Esc", "Quit"),
    ("?", "Toggle help"),
];

const HISTORY_KEYS: &[(&str, &str)] = &[
    ("↑/↓/j/k", "Navigate"),
    ("Enter", "Open in Analysis"),
    ("d", "Delete call"),
    ("t", "Filter by tag"),
    ("Esc", "Back"),
    ("q", "Quit"),
    ("?", "Toggle help"),
];

/// Returns keybinding definitions for the given mode name.
pub fn keys_for_mode(mode: &str) -> &'static [(&'static str, &'static str)] {
    match mode {
        "LIVE" => LIVE_KEYS,
        "ANALYSIS" => ANALYSIS_KEYS,
        "AGENT" => AGENT_KEYS,
        "HISTORY" => HISTORY_KEYS,
        "SESSIONS" => SESSIONS_KEYS,
        _ => &[],
    }
}

/// Render a centered help overlay popup.
pub fn render_help(f: &mut Frame, area: Rect, mode: &str) {
    let keys = keys_for_mode(mode);

    let max_key_width = keys
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let max_desc_width = keys
        .iter()
        .map(|(_, d)| d.chars().count())
        .max()
        .unwrap_or(0);
    let content_width = (max_key_width + max_desc_width + 5) as u16; // " key  — desc "
    let popup_width = content_width.max(20).min(area.width.saturating_sub(4)) + 4; // borders + padding
    let popup_height = (keys.len() as u16 + 4).min(area.height.saturating_sub(2)); // title + padding + keys + footer

    let popup_area = centered_rect(popup_width, popup_height, area);

    f.render_widget(Clear, popup_area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (key, desc) in keys {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:>width$}", key, width = max_key_width),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  —  ", Style::default().fg(Color::DarkGray)),
            Span::styled(*desc, Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        Style::default().fg(Color::DarkGray),
    )));

    let title = format!(" Help — {mode} ");
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, popup_area);
}

/// Create a centered rectangle within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    let horizontal = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0]);
    horizontal[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_for_known_modes() {
        assert!(!keys_for_mode("LIVE").is_empty());
        assert!(!keys_for_mode("ANALYSIS").is_empty());
        assert!(!keys_for_mode("AGENT").is_empty());
    }

    #[test]
    fn keys_for_unknown_mode() {
        assert!(keys_for_mode("UNKNOWN").is_empty());
    }

    #[test]
    fn live_keys_contain_space_toggle() {
        let keys = keys_for_mode("LIVE");
        assert!(keys.iter().any(|(k, _)| *k == "Space"));
    }

    #[test]
    fn analysis_keys_contain_follow_up() {
        let keys = keys_for_mode("ANALYSIS");
        assert!(keys.iter().any(|(k, _)| *k == "f"));
    }

    #[test]
    fn analysis_keys_contain_export() {
        let keys = keys_for_mode("ANALYSIS");
        assert!(keys.iter().any(|(k, _)| *k == "e"));
    }

    #[test]
    fn history_keys_contain_delete() {
        let keys = keys_for_mode("HISTORY");
        assert!(keys.iter().any(|(k, _)| *k == "d"));
    }

    #[test]
    fn live_and_sessions_keys_cover_scrolling_and_search() {
        let live = keys_for_mode("LIVE");
        for k in ["Tab", "PgUp/PgDn", "End/G", "/ or o"] {
            assert!(live.iter().any(|(key, _)| *key == k), "{k}");
        }
        let sessions = keys_for_mode("SESSIONS");
        assert!(sessions.iter().any(|(key, _)| *key == "n / N"));
    }

    #[test]
    fn history_keys_not_empty() {
        assert!(!keys_for_mode("HISTORY").is_empty());
    }
}
