use crate::app::App;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

/// State for agent mode.
pub struct AgentState {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    #[allow(dead_code)] // Used when ChatState is wired
    pub scroll_offset: usize,
}

pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            messages: vec![ChatMessage {
                role: MessageRole::System,
                content: "Welcome to SuperVox Agent. Ask questions about your calls.".into(),
            }],
            input: String::new(),
            scroll_offset: 0,
        }
    }
}

/// Render agent mode — chat panel + input box.
pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let state = &app.agent_state;

    let [chat_area, input_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);

    // Chat messages
    let items: Vec<ListItem> = state
        .messages
        .iter()
        .map(|msg| {
            let (prefix, style) = match msg.role {
                MessageRole::System => ("SYS", Style::default().fg(Color::DarkGray)),
                MessageRole::User => (">", Style::default().fg(Color::Cyan)),
                MessageRole::Assistant => ("AI", Style::default().fg(Color::Green)),
            };
            ListItem::new(Line::styled(format!("{prefix}: {}", msg.content), style))
        })
        .collect();

    let chat = List::new(items).block(
        Block::default()
            .title(" Agent Chat ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(chat, chat_area);

    // Input box
    let input = Paragraph::new(state.input.as_str())
        .block(
            Block::default()
                .title(" Input (Enter to send, Esc to quit) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));
    f.render_widget(input, input_area);
}

/// Handle key events in agent mode.
pub fn handle_key(app: &mut App, key: KeyEvent) {
    match key.code {
        crossterm::event::KeyCode::Char(c) => {
            app.agent_state.input.push(c);
        }
        crossterm::event::KeyCode::Backspace => {
            app.agent_state.input.pop();
        }
        crossterm::event::KeyCode::Enter if !app.agent_state.input.is_empty() => {
            let question = std::mem::take(&mut app.agent_state.input);
            app.agent_state.messages.push(ChatMessage {
                role: MessageRole::User,
                content: question,
            });
            // AI-NOTE: LLM call integration will replace this placeholder
            app.agent_state.messages.push(ChatMessage {
                role: MessageRole::Assistant,
                content: "Agent processing is not yet connected to LLM.".into(),
            });
        }
        _ => {}
    }
}
