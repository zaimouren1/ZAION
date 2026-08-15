//! Agentic Loop Visualization Panel
//!
//! Inspired by cc-haha's AssistantThinkingMessage and StreamingMarkdown,
//! this panel visualizes the agent's reasoning process in real-time.
//!
//! Key features:
//! - Extended Thinking display (similar to cc-haha's thinking blocks)
//! - Tool call tracking with status indicators
//! - Reasoning step visualization
//! - Scrollable viewport with virtual rendering
//!
//! # Example
//!
//! ```no_run
//! use zaion_tui::AgenticPanel;
//!
//! let mut panel = AgenticPanel::new();
//!
//! // Add reasoning steps
//! panel.add_step("Analyze user request".to_string());
//! panel.start_step(1);
//! panel.complete_step(1, true);
//!
//! // Track tool calls
//! panel.add_tool_call("read_file".to_string());
//! panel.start_tool_call("read_file");
//! panel.complete_tool_call("read_file", true, Some("File content".to_string()));
//!
//! // Update thinking text (streaming)
//! panel.update_thinking("Considering JWT vs session auth...".to_string());
//! ```

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::time::Instant;

/// Reasoning step in the agent loop
#[derive(Debug, Clone)]
pub struct ReasoningStep {
    pub step_number: usize,
    pub description: String,
    pub status: StepStatus,
    pub timestamp: Instant,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Active,
    Completed,
    Failed,
}

impl StepStatus {
    fn color(&self) -> Color {
        match self {
            StepStatus::Pending => Color::DarkGray,
            StepStatus::Active => Color::Cyan,
            StepStatus::Completed => Color::Green,
            StepStatus::Failed => Color::Red,
        }
    }

    fn symbol(&self) -> &'static str {
        match self {
            StepStatus::Pending => "○",
            StepStatus::Active => "◐",
            StepStatus::Completed => "●",
            StepStatus::Failed => "✗",
        }
    }
}

/// Tool call tracking
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_name: String,
    pub status: ToolCallStatus,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub result_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    Queued,
    Executing,
    Success,
    Failed,
}

impl ToolCallStatus {
    fn color(&self) -> Color {
        match self {
            ToolCallStatus::Queued => Color::Yellow,
            ToolCallStatus::Executing => Color::Cyan,
            ToolCallStatus::Success => Color::Green,
            ToolCallStatus::Failed => Color::Red,
        }
    }
}

/// Main agentic panel state
pub struct AgenticPanel {
    /// Current thinking text (streaming)
    pub current_thought: Option<String>,

    /// Reasoning steps history
    pub reasoning_steps: Vec<ReasoningStep>,

    /// Tool calls history
    pub tool_calls: Vec<ToolCall>,

    /// Scroll offset for virtual rendering
    pub scroll_offset: usize,

    /// Panel visibility
    pub visible: bool,

    /// Last update timestamp
    last_update: Instant,
}

impl AgenticPanel {
    pub fn new() -> Self {
        Self {
            current_thought: None,
            reasoning_steps: Vec::new(),
            tool_calls: Vec::new(),
            scroll_offset: 0,
            visible: true,
            last_update: Instant::now(),
        }
    }

    /// Add a new reasoning step
    pub fn add_step(&mut self, description: String) {
        let step_number = self.reasoning_steps.len() + 1;
        self.reasoning_steps.push(ReasoningStep {
            step_number,
            description,
            status: StepStatus::Pending,
            timestamp: Instant::now(),
            duration_ms: None,
        });
        self.last_update = Instant::now();
    }

    /// Update current thinking text (streaming)
    pub fn update_thinking(&mut self, thought: String) {
        self.current_thought = Some(thought);
        self.last_update = Instant::now();
    }

    /// Clear current thinking
    pub fn clear_thinking(&mut self) {
        self.current_thought = None;
        self.last_update = Instant::now();
    }

    /// Start executing a reasoning step
    pub fn start_step(&mut self, step_number: usize) {
        if let Some(step) = self.reasoning_steps.get_mut(step_number.saturating_sub(1)) {
            step.status = StepStatus::Active;
            self.last_update = Instant::now();
        }
    }

    /// Complete a reasoning step
    pub fn complete_step(&mut self, step_number: usize, success: bool) {
        if let Some(step) = self.reasoning_steps.get_mut(step_number.saturating_sub(1)) {
            step.status = if success {
                StepStatus::Completed
            } else {
                StepStatus::Failed
            };
            let duration = step.timestamp.elapsed();
            step.duration_ms = Some(duration.as_millis() as u64);
            self.last_update = Instant::now();
        }
    }

    /// Add a tool call
    pub fn add_tool_call(&mut self, tool_name: String) {
        self.tool_calls.push(ToolCall {
            tool_name,
            status: ToolCallStatus::Queued,
            started_at: Instant::now(),
            completed_at: None,
            result_preview: None,
        });
        self.last_update = Instant::now();
    }

    /// Start executing a tool call
    pub fn start_tool_call(&mut self, tool_name: &str) {
        if let Some(call) = self
            .tool_calls
            .iter_mut()
            .rev()
            .find(|c| c.tool_name == tool_name)
        {
            call.status = ToolCallStatus::Executing;
            self.last_update = Instant::now();
        }
    }

    /// Complete a tool call
    pub fn complete_tool_call(
        &mut self,
        tool_name: &str,
        success: bool,
        result_preview: Option<String>,
    ) {
        if let Some(call) = self
            .tool_calls
            .iter_mut()
            .rev()
            .find(|c| c.tool_name == tool_name)
        {
            call.status = if success {
                ToolCallStatus::Success
            } else {
                ToolCallStatus::Failed
            };
            call.completed_at = Some(Instant::now());
            call.result_preview = result_preview;
            self.last_update = Instant::now();
        }
    }

    /// Clear all state (start new turn)
    pub fn reset(&mut self) {
        self.current_thought = None;
        self.reasoning_steps.clear();
        self.tool_calls.clear();
        self.scroll_offset = 0;
        self.last_update = Instant::now();
    }

    /// Toggle visibility
    pub fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }

    /// Scroll down
    pub fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.reasoning_steps.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    /// Scroll up
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Render the panel
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Agent Loop ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into sections: thinking, reasoning steps, tool calls
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // Thinking section
                Constraint::Min(10),   // Reasoning steps
                Constraint::Length(8), // Tool calls
            ])
            .split(inner);

        // Render current thinking
        self.render_thinking(frame, chunks[0]);

        // Render reasoning steps
        self.render_reasoning_steps(frame, chunks[1]);

        // Render tool calls
        self.render_tool_calls(frame, chunks[2]);
    }

    fn render_thinking(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Extended Thinking ");

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = if let Some(ref thought) = self.current_thought {
            thought.clone()
        } else {
            "Idle / 空闲".to_string()
        };

        let paragraph = Paragraph::new(content)
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, inner);
    }

    fn render_reasoning_steps(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(
                " Reasoning Steps ({}) ",
                self.reasoning_steps.len()
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = self
            .reasoning_steps
            .iter()
            .skip(self.scroll_offset)
            .map(|step| {
                let status_symbol = step.status.symbol();
                let status_color = step.status.color();

                let duration_text = if let Some(ms) = step.duration_ms {
                    format!(" ({}ms)", ms)
                } else {
                    String::new()
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("{} ", status_symbol),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        format!("Step {}: ", step.step_number),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(&step.description),
                    Span::styled(duration_text, Style::default().fg(Color::DarkGray)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_tool_calls(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" Tool Calls ({}) ", self.tool_calls.len()));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = self
            .tool_calls
            .iter()
            .rev()
            .take(5) // Show last 5 tool calls
            .map(|call| {
                let status_color = call.status.color();
                let duration = if let Some(completed) = call.completed_at {
                    let ms = completed.duration_since(call.started_at).as_millis();
                    format!(" ({}ms)", ms)
                } else {
                    String::new()
                };

                let mut spans = vec![
                    Span::styled(
                        format!("[{:?}]", call.status),
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(&call.tool_name, Style::default().fg(Color::Yellow)),
                    Span::styled(duration, Style::default().fg(Color::DarkGray)),
                ];

                if let Some(ref preview) = call.result_preview {
                    let truncated = if preview.len() > 40 {
                        format!("{}...", &preview[..40])
                    } else {
                        preview.clone()
                    };
                    spans.push(Span::raw("\n    → "));
                    spans.push(Span::styled(
                        truncated,
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }
}

impl Default for AgenticPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_creation() {
        let panel = AgenticPanel::new();
        assert!(panel.visible);
        assert!(panel.current_thought.is_none());
        assert_eq!(panel.reasoning_steps.len(), 0);
        assert_eq!(panel.tool_calls.len(), 0);
    }

    #[test]
    fn test_add_reasoning_step() {
        let mut panel = AgenticPanel::new();
        panel.add_step("Analyze user request".to_string());

        assert_eq!(panel.reasoning_steps.len(), 1);
        assert_eq!(panel.reasoning_steps[0].step_number, 1);
        assert_eq!(panel.reasoning_steps[0].status, StepStatus::Pending);
    }

    #[test]
    fn test_step_lifecycle() {
        let mut panel = AgenticPanel::new();
        panel.add_step("Test step".to_string());

        panel.start_step(1);
        assert_eq!(panel.reasoning_steps[0].status, StepStatus::Active);

        panel.complete_step(1, true);
        assert_eq!(panel.reasoning_steps[0].status, StepStatus::Completed);
        assert!(panel.reasoning_steps[0].duration_ms.is_some());
    }

    #[test]
    fn test_tool_call_lifecycle() {
        let mut panel = AgenticPanel::new();
        panel.add_tool_call("read_file".to_string());

        assert_eq!(panel.tool_calls.len(), 1);
        assert_eq!(panel.tool_calls[0].status, ToolCallStatus::Queued);

        panel.start_tool_call("read_file");
        assert_eq!(panel.tool_calls[0].status, ToolCallStatus::Executing);

        panel.complete_tool_call("read_file", true, Some("File content".to_string()));
        assert_eq!(panel.tool_calls[0].status, ToolCallStatus::Success);
        assert!(panel.tool_calls[0].completed_at.is_some());
    }

    #[test]
    fn test_reset() {
        let mut panel = AgenticPanel::new();
        panel.add_step("Step 1".to_string());
        panel.add_tool_call("tool1".to_string());
        panel.update_thinking("Thinking...".to_string());

        panel.reset();

        assert!(panel.current_thought.is_none());
        assert_eq!(panel.reasoning_steps.len(), 0);
        assert_eq!(panel.tool_calls.len(), 0);
    }

    #[test]
    fn test_scroll() {
        let mut panel = AgenticPanel::new();
        for i in 0..10 {
            panel.add_step(format!("Step {}", i));
        }

        assert_eq!(panel.scroll_offset, 0);

        panel.scroll_down(3);
        assert_eq!(panel.scroll_offset, 3);

        panel.scroll_up(1);
        assert_eq!(panel.scroll_offset, 2);

        panel.scroll_down(100); // Should clamp to max
        assert_eq!(panel.scroll_offset, 9);
    }
}
