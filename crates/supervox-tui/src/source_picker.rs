//! "What are we listening to?" — the list shown when Live opens.
//!
//! One list instead of config edits and `m`-cycling: every input device (a Bluetooth
//! headset such as Ray-Ban Meta glasses shows up here as a microphone), the glasses stream
//! when a local RTMP receiver is up (SpecBridge, `tools/receive.sh`), and any extra stream
//! addresses from `SUPERVOX_INPUTS` (comma-separated). Enter picks, Esc keeps the config.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

/// The glasses stream SpecBridge publishes into the local mediamtx receiver.
pub const GLASSES_STREAM: &str = "rtmp://127.0.0.1:1935/live/glasses";

#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    /// Use this input device as the mic (`None` = system default).
    Mic(Option<String>),
    /// Pull this network stream as the second source.
    Stream(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub choice: Choice,
}

pub struct SourcePicker {
    pub items: Vec<Item>,
    state: ListState,
}

/// Names that are glasses (Bluetooth mic of Meta / Ray-Ban glasses).
pub fn is_glasses(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("rb meta")
        || n.contains("ray-ban")
        || n.contains("meta glasses")
        || n.contains("oakley meta")
}

fn port_open(addr: &str) -> bool {
    addr.parse::<SocketAddr>()
        .is_ok_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(200)).is_ok())
}

impl SourcePicker {
    /// Build the list from what is actually available right now.
    pub fn discover(current_mic: Option<&str>) -> Self {
        let mut items = Vec::new();
        let mics = voxkit::mic::list_input_devices().unwrap_or_default();

        // Glasses first: Bluetooth mic, then the phone stream.
        for m in mics.iter().filter(|m| is_glasses(m)) {
            items.push(Item {
                label: format!("Glasses (Bluetooth): {m}"),
                choice: Choice::Mic(Some(m.clone())),
            });
        }
        if port_open("127.0.0.1:1935") {
            items.push(Item {
                label: format!("Glasses (via phone): {GLASSES_STREAM}"),
                choice: Choice::Stream(GLASSES_STREAM.into()),
            });
        }
        if let Ok(extra) = std::env::var("SUPERVOX_INPUTS") {
            for url in extra.split(',').map(str::trim).filter(|u| !u.is_empty()) {
                items.push(Item {
                    label: format!("Stream: {url}"),
                    choice: Choice::Stream(url.into()),
                });
            }
        }
        items.push(Item {
            label: "System default mic".into(),
            choice: Choice::Mic(None),
        });
        for m in mics.iter().filter(|m| !is_glasses(m)) {
            items.push(Item {
                label: format!("Mic: {m}"),
                choice: Choice::Mic(Some(m.clone())),
            });
        }

        // Start on the current mic if it is in the list, else on the first (glasses) entry.
        let start = current_mic
            .and_then(|cur| {
                items
                    .iter()
                    .position(|i| i.choice == Choice::Mic(Some(cur.to_string())))
            })
            .filter(|_| !items.iter().any(|i| i.label.starts_with("Glasses")))
            .unwrap_or(0);
        let mut state = ListState::default();
        state.select(Some(start));
        Self { items, state }
    }

    /// `Some(Some(choice))` = picked, `Some(None)` = dismissed, `None` = keep showing.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Option<Choice>> {
        let n = self.items.len();
        let i = self.state.selected().unwrap_or(0);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.state.select(Some((i + n - 1) % n.max(1))),
            KeyCode::Down | KeyCode::Char('j') => self.state.select(Some((i + 1) % n.max(1))),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return Some(self.items.get(i).map(|it| it.choice.clone()));
            }
            KeyCode::Esc | KeyCode::Char('q') => return Some(None),
            _ => {}
        }
        None
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let h = (self.items.len() as u16 + 4).min(area.height);
        let [row] = Layout::vertical([Constraint::Length(h)])
            .flex(Flex::Center)
            .areas(area);
        let [rect] = Layout::horizontal([Constraint::Percentage(80)])
            .flex(Flex::Center)
            .areas(row);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" What are we listening to? ")
            .title_bottom(Line::from(" ↑↓ choose · Enter start · Esc keep config "));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        if self.items.is_empty() {
            f.render_widget(Paragraph::new("No inputs found"), inner);
            return;
        }
        let list = List::new(self.items.iter().map(|i| ListItem::new(i.label.clone())))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        f.render_stateful_widget(list, inner, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_glasses_names() {
        assert!(is_glasses("RB Meta 12AB"));
        assert!(is_glasses("Ray-Ban Meta"));
        assert!(!is_glasses("MacBook Pro Microphone"));
    }
}
