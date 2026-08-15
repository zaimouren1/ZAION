//! Process list component - virtualized process viewer
//!
//! Displays all active processes with virtual scrolling for 100+ processes.

use super::{Component, ComponentAction, ComponentId, DataEvent, ProcessInfo, SystemEvent};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

pub struct ProcessList {
    id: ComponentId,
    processes: Vec<ProcessInfo>,
    active: bool,
    visible: bool,
    scroll_offset: usize,
    selected_index: Option<usize>,
    /// Viewport height for virtual scrolling
    viewport_height: usize,
}

impl ProcessList {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            processes: Vec::new(),
            active: false,
            visible: true,
            scroll_offset: 0,
            selected_index: None,
            viewport_height: 20,
        }
    }

    /// Get visible process range for virtual scrolling
    fn visible_range(&self) -> (usize, usize) {
        let start = self.scroll_offset;
        let end = (start + self.viewport_height).min(self.processes.len());
        (start, end)
    }

    fn select_next(&mut self) {
        if self.processes.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            Some(idx) if idx < self.processes.len() - 1 => idx + 1,
            None if !self.processes.is_empty() => 0,
            _ => return,
        };

        self.selected_index = Some(new_index);

        // Auto-scroll to keep selection visible
        if new_index >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = new_index.saturating_sub(self.viewport_height - 1);
        }
    }

    fn select_prev(&mut self) {
        if self.processes.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            Some(idx) if idx > 0 => idx - 1,
            None if !self.processes.is_empty() => 0,
            _ => return,
        };

        self.selected_index = Some(new_index);

        // Auto-scroll to keep selection visible
        if new_index < self.scroll_offset {
            self.scroll_offset = new_index;
        }
    }
}

impl Component for ProcessList {
    fn name(&self) -> &str {
        "Processes"
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                ComponentAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                ComponentAction::None
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height);
                ComponentAction::None
            }
            KeyCode::PageDown => {
                let max_scroll = self.processes.len().saturating_sub(self.viewport_height);
                self.scroll_offset = (self.scroll_offset + self.viewport_height).min(max_scroll);
                ComponentAction::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_offset = 0;
                self.selected_index = if !self.processes.is_empty() {
                    Some(0)
                } else {
                    None
                };
                ComponentAction::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.processes.is_empty() {
                    self.selected_index = Some(self.processes.len() - 1);
                    self.scroll_offset = self.processes.len().saturating_sub(self.viewport_height);
                }
                ComponentAction::None
            }
            KeyCode::Enter => {
                // View selected process details
                if let Some(_idx) = self.selected_index {
                    // TODO: Emit ViewProcessDetails event
                }
                ComponentAction::None
            }
            KeyCode::Char('d') => {
                // Delete selected process
                if let Some(_idx) = self.selected_index {
                    // TODO: Emit DeleteProcess event
                }
                ComponentAction::None
            }
            _ => ComponentAction::None,
        }
    }

    fn handle_event(&mut self, event: &SystemEvent) {
        if let SystemEvent::Data(DataEvent::ProcessesUpdated(processes)) = event {
            self.processes = processes.clone();

            // Adjust selection if it's out of bounds
            if let Some(idx) = self.selected_index {
                if idx >= self.processes.len() {
                    self.selected_index = if self.processes.is_empty() {
                        None
                    } else {
                        Some(self.processes.len() - 1)
                    };
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::{
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, List, ListItem},
        };

        // Update viewport height
        self.viewport_height = area.height.saturating_sub(3) as usize;

        let (start, end) = self.visible_range();

        let items: Vec<ListItem> = self
            .processes
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(idx, process)| {
                let is_selected = self.selected_index == Some(idx);

                let state_color = match process.state.as_str() {
                    "Active" => Color::Green,
                    "Sleeping" => Color::DarkGray,
                    _ => Color::White,
                };

                let prefix = if is_selected { "▶ " } else { "  " };

                let line = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<36}", truncate(&process.principal_id, 36)),
                        if is_selected {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<10}", process.state),
                        Style::default().fg(state_color),
                    ),
                    Span::raw(" "),
                    Span::raw(format!("{:<16}", truncate(&process.workspace, 16))),
                ]);

                ListItem::new(line)
            })
            .collect();

        let title = format!(
            "Processes ({}) [Showing {}-{}/{} | ↑↓ select | Enter view | d delete]",
            self.processes.len(),
            start + 1,
            end,
            self.processes.len()
        );

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if self.active {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        );

        frame.render_widget(list, area);
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 3 {
        format!("{}...", &s[..max - 3])
    } else {
        ".".repeat(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_list_creation() {
        let list = ProcessList::new(ComponentId(5));
        assert_eq!(list.name(), "Processes");
        assert!(!list.is_active());
        assert!(list.is_visible());
        assert!(list.processes.is_empty());
        assert_eq!(list.selected_index, None);
    }

    #[test]
    fn test_process_selection() {
        let mut list = ProcessList::new(ComponentId(5));

        // Add some processes
        let processes = vec![
            ProcessInfo {
                principal_id: "proc1".to_string(),
                state: "Active".to_string(),
                workspace: "default".to_string(),
                project: "proj1".to_string(),
            },
            ProcessInfo {
                principal_id: "proc2".to_string(),
                state: "Sleeping".to_string(),
                workspace: "test".to_string(),
                project: "proj2".to_string(),
            },
        ];

        list.handle_event(&SystemEvent::Data(DataEvent::ProcessesUpdated(processes)));
        assert_eq!(list.processes.len(), 2);

        // Test selection
        list.select_next();
        assert_eq!(list.selected_index, Some(0));

        list.select_next();
        assert_eq!(list.selected_index, Some(1));

        list.select_prev();
        assert_eq!(list.selected_index, Some(0));
    }

    #[test]
    fn test_virtual_scrolling() {
        let mut list = ProcessList::new(ComponentId(5));
        list.viewport_height = 10;

        // Add 50 processes
        let processes: Vec<ProcessInfo> = (0..50)
            .map(|i| ProcessInfo {
                principal_id: format!("proc_{}", i),
                state: "Active".to_string(),
                workspace: "default".to_string(),
                project: format!("proj_{}", i),
            })
            .collect();

        list.handle_event(&SystemEvent::Data(DataEvent::ProcessesUpdated(processes)));

        let (start, end) = list.visible_range();
        assert_eq!(start, 0);
        assert_eq!(end, 10);

        list.scroll_offset = 20;
        let (start, end) = list.visible_range();
        assert_eq!(start, 20);
        assert_eq!(end, 30);
    }
}
