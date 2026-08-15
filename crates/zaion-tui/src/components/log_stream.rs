//! Log stream component - real-time virtualized log viewer
//!
//! Displays streaming logs with filtering, search, and auto-scroll capabilities.
//! Uses virtual rendering to handle 10k+ log lines efficiently.

use super::{Component, ComponentAction, ComponentId, SystemEvent, TimerEvent};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn as_str(&self) -> &str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

pub struct LogStream {
    id: ComponentId,
    logs: Vec<LogEntry>,
    active: bool,
    visible: bool,
    scroll_offset: usize,
    filter_level: LogLevel,
    auto_scroll: bool,
    search_query: Option<String>,
    /// Viewport height for virtual scrolling
    viewport_height: usize,
    /// Maximum logs to keep in memory (circular buffer)
    max_logs: usize,
}

impl LogStream {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            logs: Vec::new(),
            active: false,
            visible: true,
            scroll_offset: 0,
            filter_level: LogLevel::Info,
            auto_scroll: true,
            search_query: None,
            viewport_height: 20,
            max_logs: 10_000, // Keep up to 10k logs in memory
        }
    }

    fn add_log(&mut self, entry: LogEntry) {
        self.logs.push(entry);

        // Circular buffer: remove oldest logs if exceeding max
        if self.logs.len() > self.max_logs {
            self.logs.drain(0..(self.logs.len() - self.max_logs));
            // Adjust scroll offset after draining
            self.scroll_offset = self
                .scroll_offset
                .saturating_sub(self.logs.len() - self.max_logs);
        }

        if self.auto_scroll {
            self.scroll_offset = self.logs.len().saturating_sub(1);
        }
    }

    /// Add log from ShadowEvent
    fn add_shadow_log(&mut self, message: String, level: LogLevel) {
        let timestamp = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        self.add_log(LogEntry {
            timestamp,
            level,
            target: "shadow_exec".to_string(),
            message,
        });
    }

    /// Get visible log range for virtual scrolling
    fn visible_range(&self, filtered: &[&LogEntry]) -> (usize, usize) {
        let start = self.scroll_offset.min(filtered.len().saturating_sub(1));
        let end = (start + self.viewport_height).min(filtered.len());
        (start, end)
    }

    fn filtered_logs(&self) -> Vec<&LogEntry> {
        self.logs
            .iter()
            .filter(|log| {
                let level_ok = match self.filter_level {
                    LogLevel::Error => matches!(log.level, LogLevel::Error),
                    LogLevel::Warn => matches!(log.level, LogLevel::Warn | LogLevel::Error),
                    LogLevel::Info => {
                        matches!(log.level, LogLevel::Info | LogLevel::Warn | LogLevel::Error)
                    }
                    LogLevel::Debug => matches!(
                        log.level,
                        LogLevel::Debug | LogLevel::Info | LogLevel::Warn | LogLevel::Error
                    ),
                    LogLevel::Trace => true,
                };

                let search_ok = self
                    .search_query
                    .as_ref()
                    .is_none_or(|query| log.message.contains(query) || log.target.contains(query));

                level_ok && search_ok
            })
            .collect()
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.logs.len().saturating_sub(1);
    }

    fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    fn cycle_filter_level(&mut self) {
        self.filter_level = match self.filter_level {
            LogLevel::Trace => LogLevel::Debug,
            LogLevel::Debug => LogLevel::Info,
            LogLevel::Info => LogLevel::Warn,
            LogLevel::Warn => LogLevel::Error,
            LogLevel::Error => LogLevel::Trace,
        };
    }
}

impl Component for LogStream {
    fn name(&self) -> &str {
        "Logs"
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction {
        match key.code {
            KeyCode::Up => {
                self.auto_scroll = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                ComponentAction::None
            }
            KeyCode::Down => {
                self.auto_scroll = false;
                if self.scroll_offset < self.logs.len().saturating_sub(1) {
                    self.scroll_offset += 1;
                }
                ComponentAction::None
            }
            KeyCode::PageUp => {
                self.auto_scroll = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height);
                ComponentAction::None
            }
            KeyCode::PageDown => {
                self.auto_scroll = false;
                let max_scroll = self.logs.len().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + self.viewport_height).min(max_scroll);
                ComponentAction::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_offset = 0;
                self.auto_scroll = false;
                ComponentAction::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_to_bottom();
                self.auto_scroll = false;
                ComponentAction::None
            }
            KeyCode::Char('a') => {
                self.toggle_auto_scroll();
                ComponentAction::None
            }
            KeyCode::Char('l') => {
                self.cycle_filter_level();
                ComponentAction::None
            }
            KeyCode::Char('c') => {
                // Clear all logs
                self.logs.clear();
                self.scroll_offset = 0;
                ComponentAction::None
            }
            _ => ComponentAction::None,
        }
    }

    fn handle_event(&mut self, event: &SystemEvent) {
        match event {
            SystemEvent::Timer(TimerEvent::PeriodicRefresh) => {
                // Periodic refresh - no action needed for logs
            }
            SystemEvent::Shadow(shadow_event) => {
                use super::ShadowEventWrapper;

                match shadow_event {
                    ShadowEventWrapper::ExecutorStarted => {
                        self.add_shadow_log("Executor started".to_string(), LogLevel::Info);
                    }
                    ShadowEventWrapper::ExecutorStopped => {
                        self.add_shadow_log("Executor stopped".to_string(), LogLevel::Info);
                    }
                    ShadowEventWrapper::TaskSpawned { task_id, name } => {
                        self.add_shadow_log(
                            format!("Task spawned: {} ({})", name, task_id),
                            LogLevel::Debug,
                        );
                    }
                    ShadowEventWrapper::TaskStarted { task_id, name } => {
                        self.add_shadow_log(
                            format!("Task started: {} ({})", name, task_id),
                            LogLevel::Info,
                        );
                    }
                    ShadowEventWrapper::TaskCompleted {
                        task_id,
                        name,
                        success,
                        duration_ms,
                    } => {
                        let level = if *success {
                            LogLevel::Info
                        } else {
                            LogLevel::Warn
                        };
                        self.add_shadow_log(
                            format!(
                                "Task completed: {} ({}) in {}ms - {}",
                                name,
                                task_id,
                                duration_ms,
                                if *success { "success" } else { "failed" }
                            ),
                            level,
                        );
                    }
                    ShadowEventWrapper::TaskCancelled { task_id } => {
                        self.add_shadow_log(format!("Task cancelled: {}", task_id), LogLevel::Warn);
                    }
                    ShadowEventWrapper::AciOperation { task_id, op, ok } => {
                        let level = if *ok {
                            LogLevel::Debug
                        } else {
                            LogLevel::Error
                        };
                        self.add_shadow_log(
                            format!(
                                "ACI operation: {} - {} ({})",
                                op,
                                if *ok { "success" } else { "failed" },
                                task_id
                            ),
                            level,
                        );
                    }
                }
            }
            _ => {}
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

        let filtered = self.filtered_logs();
        let total_count = self.logs.len();
        let visible_count = filtered.len();

        // Virtual scrolling: only render visible logs
        let (start, end) = self.visible_range(&filtered);

        let items: Vec<ListItem> = filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(_idx, log)| {
                let level_style = match log.level {
                    LogLevel::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    LogLevel::Warn => Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                    LogLevel::Info => Style::default().fg(Color::Green),
                    LogLevel::Debug => Style::default().fg(Color::Cyan),
                    LogLevel::Trace => Style::default().fg(Color::DarkGray),
                };

                let line = Line::from(vec![
                    Span::styled(&log.timestamp, Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(format!("{:5}", log.level.as_str()), level_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:20}", truncate(&log.target, 20)),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::raw(" "),
                    Span::raw(&log.message),
                ]);

                ListItem::new(line)
            })
            .collect();

        let title = format!(
            "Logs [Filter: {} | Showing {}-{}/{} ({} total) | Auto-scroll: {} | [a]uto [l]evel [c]lear [g/G] top/bottom]",
            self.filter_level.as_str(),
            start + 1,
            end,
            visible_count,
            total_count,
            if self.auto_scroll { "✓" } else { "✗" }
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
    fn test_log_stream_creation() {
        let stream = LogStream::new(ComponentId(1));
        assert_eq!(stream.name(), "Logs");
        assert!(!stream.is_active());
        assert!(stream.is_visible());
        assert!(stream.logs.is_empty());
        assert!(stream.auto_scroll);
    }

    #[test]
    fn test_log_filtering() {
        let mut stream = LogStream::new(ComponentId(1));

        stream.add_log(LogEntry {
            timestamp: "12:00:00".to_string(),
            level: LogLevel::Debug,
            target: "test".to_string(),
            message: "Debug message".to_string(),
        });

        stream.add_log(LogEntry {
            timestamp: "12:00:01".to_string(),
            level: LogLevel::Info,
            target: "test".to_string(),
            message: "Info message".to_string(),
        });

        stream.add_log(LogEntry {
            timestamp: "12:00:02".to_string(),
            level: LogLevel::Error,
            target: "test".to_string(),
            message: "Error message".to_string(),
        });

        assert_eq!(stream.logs.len(), 3);

        // Filter to Info level (should show Info, Warn, Error)
        stream.filter_level = LogLevel::Info;
        let filtered = stream.filtered_logs();
        assert_eq!(filtered.len(), 2);

        // Filter to Error level (should show only Error)
        stream.filter_level = LogLevel::Error;
        let filtered = stream.filtered_logs();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_auto_scroll() {
        let mut stream = LogStream::new(ComponentId(1));

        for i in 0..10 {
            stream.add_log(LogEntry {
                timestamp: format!("12:00:{:02}", i),
                level: LogLevel::Info,
                target: "test".to_string(),
                message: format!("Message {}", i),
            });
        }

        assert_eq!(stream.scroll_offset, 9);

        stream.handle_key(KeyEvent::from(KeyCode::Up));
        assert!(!stream.auto_scroll);
        assert_eq!(stream.scroll_offset, 8);

        stream.toggle_auto_scroll();
        assert!(stream.auto_scroll);
        assert_eq!(stream.scroll_offset, 9);
    }

    #[test]
    fn test_filter_level_cycling() {
        let mut stream = LogStream::new(ComponentId(1));
        assert_eq!(stream.filter_level, LogLevel::Info);

        stream.cycle_filter_level();
        assert_eq!(stream.filter_level, LogLevel::Warn);

        stream.cycle_filter_level();
        assert_eq!(stream.filter_level, LogLevel::Error);

        stream.cycle_filter_level();
        assert_eq!(stream.filter_level, LogLevel::Trace);
    }
}
