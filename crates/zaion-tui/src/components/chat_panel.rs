//! Chat panel component - Ink-style conversation interface
//!
//! Displays a conversation view with messages, extended thinking blocks,
//! and tool calls. Supports Markdown rendering and virtual scrolling.

use super::{
    ChatMessage, Component, ComponentAction, ComponentId, DataEvent, MessageRole, SystemEvent,
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

pub struct ChatPanel {
    id: ComponentId,
    messages: Vec<ChatMessage>,
    active: bool,
    visible: bool,
    scroll_offset: usize,
    input_buffer: String,
    /// Viewport height for virtual scrolling
    viewport_height: usize,
    /// Track which thinking blocks are expanded
    expanded_thinking: Vec<bool>,
}

impl ChatPanel {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            messages: Vec::new(),
            active: false,
            visible: true,
            scroll_offset: 0,
            input_buffer: String::new(),
            viewport_height: 20,
            expanded_thinking: Vec::new(),
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.messages.len().saturating_sub(1);
    }

    /// Toggle thinking block expansion for a message
    fn toggle_thinking(&mut self, message_index: usize) {
        if message_index < self.expanded_thinking.len() {
            self.expanded_thinking[message_index] = !self.expanded_thinking[message_index];
        }
    }

    /// Get visible message range for virtual scrolling
    fn visible_range(&self) -> (usize, usize) {
        let start = self.scroll_offset;
        let end = (start + self.viewport_height).min(self.messages.len());
        (start, end)
    }
}

impl Component for ChatPanel {
    fn name(&self) -> &str {
        "Chat"
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction {
        match key.code {
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                ComponentAction::None
            }
            KeyCode::Down => {
                if self.scroll_offset < self.messages.len().saturating_sub(1) {
                    self.scroll_offset += 1;
                }
                ComponentAction::None
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height);
                ComponentAction::None
            }
            KeyCode::PageDown => {
                let max_scroll = self.messages.len().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + self.viewport_height).min(max_scroll);
                ComponentAction::None
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                ComponentAction::None
            }
            KeyCode::End => {
                self.scroll_to_bottom();
                ComponentAction::None
            }
            KeyCode::Char('t') if self.active => {
                // Toggle thinking block for current visible message
                self.toggle_thinking(self.scroll_offset);
                ComponentAction::None
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
                ComponentAction::None
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                ComponentAction::None
            }
            KeyCode::Enter => {
                // TODO: Emit message to event bus
                self.input_buffer.clear();
                ComponentAction::None
            }
            _ => ComponentAction::None,
        }
    }

    fn handle_event(&mut self, event: &SystemEvent) {
        if let SystemEvent::Data(DataEvent::MessageReceived(msg)) = event {
            self.messages.push(msg.clone());
            // Add expanded_thinking state for new message (default: collapsed)
            self.expanded_thinking.push(false);
            self.scroll_to_bottom();
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::{
            layout::{Constraint, Direction, Layout},
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Paragraph, Wrap},
        };

        // Update viewport height based on area
        self.viewport_height = area.height.saturating_sub(4) as usize;

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);

        // Virtual scrolling: only render visible messages
        let (start, end) = self.visible_range();
        let mut text_lines = Vec::new();

        for (idx, msg) in self
            .messages
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
        {
            let role_style = match msg.role {
                MessageRole::User => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                MessageRole::Assistant => Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                MessageRole::System => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            };

            text_lines.push(Line::from(vec![Span::styled(
                format!("{:?}: ", msg.role),
                role_style,
            )]));
            text_lines.push(Line::from(msg.content.clone()));

            // Extended Thinking block with folding
            if let Some(thinking) = &msg.thinking {
                let is_expanded = self.expanded_thinking.get(idx).copied().unwrap_or(false);
                text_lines.push(Line::from(""));

                if is_expanded {
                    // Expanded view: show full thinking
                    text_lines.push(Line::from(Span::styled(
                        "┌─ Extended Thinking ──────────────────────────────┐",
                        Style::default().fg(Color::DarkGray),
                    )));

                    // Split thinking into lines and wrap
                    for line in thinking.lines() {
                        text_lines.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(line, Style::default().fg(Color::Gray)),
                        ]));
                    }

                    text_lines.push(Line::from(Span::styled(
                        "└──────────────────────────────────────────────────┘",
                        Style::default().fg(Color::DarkGray),
                    )));
                    text_lines.push(Line::from(Span::styled(
                        "  [t] to collapse",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                } else {
                    // Collapsed view: show preview
                    let preview = thinking.lines().next().unwrap_or("");
                    let preview = if preview.len() > 50 {
                        format!("{}...", &preview[..50])
                    } else {
                        preview.to_string()
                    };

                    text_lines.push(Line::from(vec![
                        Span::styled(
                            "▶ Extended Thinking: ",
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            preview,
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                    text_lines.push(Line::from(Span::styled(
                        "  [t] to expand",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }

            // Tool calls with inline display
            if !msg.tool_calls.is_empty() {
                text_lines.push(Line::from(""));
                text_lines.push(Line::from(Span::styled(
                    format!(
                        "┌─ Tool Calls ({}) ────────────────────────────────┐",
                        msg.tool_calls.len()
                    ),
                    Style::default().fg(Color::Magenta),
                )));

                for tool_call in &msg.tool_calls {
                    text_lines.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(Color::Magenta)),
                        Span::styled(
                            format!("[{}] {}", tool_call.status, tool_call.name),
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    if let Some(result) = &tool_call.result {
                        let preview = if result.len() > 60 {
                            format!("{}...", &result[..60])
                        } else {
                            result.clone()
                        };
                        text_lines.push(Line::from(vec![
                            Span::styled("│   → ", Style::default().fg(Color::Magenta)),
                            Span::styled(preview, Style::default().fg(Color::Gray)),
                        ]));
                    }
                }

                text_lines.push(Line::from(Span::styled(
                    "└──────────────────────────────────────────────────┘",
                    Style::default().fg(Color::Magenta),
                )));
            }

            text_lines.push(Line::from(""));
        }

        // Scroll indicator
        if self.messages.len() > self.viewport_height {
            text_lines.push(Line::from(Span::styled(
                format!(
                    "  [Showing {}-{} of {} messages] [↑↓ PgUp/PgDn Home/End to scroll]",
                    start + 1,
                    end,
                    self.messages.len()
                ),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        let messages_widget = Paragraph::new(text_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Chat")
                    .border_style(if self.active {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    }),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(messages_widget, chunks[0]);

        // Input area
        let input_widget = Paragraph::new(self.input_buffer.as_str())
            .block(Block::default().borders(Borders::ALL).title("Input"))
            .style(Style::default().fg(Color::White));

        frame.render_widget(input_widget, chunks[1]);
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn on_focus(&mut self) {
        self.active = true;
    }

    fn on_blur(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_panel_creation() {
        let panel = ChatPanel::new(ComponentId(1));
        assert_eq!(panel.name(), "Chat");
        assert!(!panel.is_active());
        assert!(panel.is_visible());
        assert!(panel.messages.is_empty());
    }

    #[test]
    fn test_message_handling() {
        let mut panel = ChatPanel::new(ComponentId(1));
        let msg = ChatMessage {
            role: MessageRole::User,
            content: "Hello".to_string(),
            timestamp: std::time::Instant::now(),
            thinking: None,
            tool_calls: Vec::new(),
        };

        panel.handle_event(&SystemEvent::Data(DataEvent::MessageReceived(msg)));
        assert_eq!(panel.messages.len(), 1);
    }

    #[test]
    fn test_scroll_behavior() {
        let mut panel = ChatPanel::new(ComponentId(1));
        for i in 0..10 {
            let msg = ChatMessage {
                role: MessageRole::Assistant,
                content: format!("Message {}", i),
                timestamp: std::time::Instant::now(),
                thinking: None,
                tool_calls: Vec::new(),
            };
            panel.handle_event(&SystemEvent::Data(DataEvent::MessageReceived(msg)));
        }

        assert_eq!(panel.scroll_offset, 9);

        panel.handle_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(panel.scroll_offset, 8);

        panel.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(panel.scroll_offset, 9);
    }
}
