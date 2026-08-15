//! TUI V2 Application - Component-based TUI with EventBus
//!
//! This module integrates all 6 panels with the EventBus for real-time updates.

use crossterm::event::KeyEvent;
use ratatui::{backend::Backend, Frame, Terminal};
use std::io;
use zaion_shadow::ShadowEventRx;

use crate::agentic_panel::AgenticPanel;
use crate::components::{
    ChatPanel, Component, ComponentId, LogStream, MemoryViz, ProcessList, ShadowEventWrapper,
    SystemEvent, TopologyPanel,
};
use crate::layout::{Layout, LayoutMode};
use crate::layout_renderer::LayoutRenderer;
use crate::run_tui_v2::TuiV2Config;
use crate::shadow_adapter::poll_shadow_events;
use crate::state::event_bus::EventBus;

/// Main TUI V2 application with component-based architecture
pub struct TuiApp {
    /// Configuration
    config: TuiV2Config,

    /// Event bus for broadcasting updates
    event_bus: EventBus,

    /// All 6 components
    chat_panel: ChatPanel,
    agent_panel: AgenticPanel,
    log_stream: LogStream,
    topology_panel: TopologyPanel,
    process_list: ProcessList,
    memory_viz: MemoryViz,

    /// Current layout mode
    layout: Layout,

    /// Component IDs for layout cycling
    component_ids: [ComponentId; 6],

    /// Currently focused component index (0-5)
    focused_component: usize,

    /// Shadow event receiver (optional)
    shadow_rx: Option<ShadowEventRx>,
}

impl TuiApp {
    /// Create a new TUI V2 application
    pub fn new(config: TuiV2Config, shadow_rx: Option<ShadowEventRx>) -> Self {
        let event_bus = EventBus::new();

        // Create component IDs
        let component_ids = [
            ComponentId(1), // ChatPanel
            ComponentId(2), // AgenticPanel
            ComponentId(3), // LogStream
            ComponentId(4), // TopologyPanel
            ComponentId(5), // ProcessList
            ComponentId(6), // MemoryViz
        ];

        // Create components
        let chat_panel = ChatPanel::new(component_ids[0]);
        let agent_panel = AgenticPanel::new();
        let log_stream = LogStream::new(component_ids[2]);
        let topology_panel = TopologyPanel::new(component_ids[3]);
        let process_list = ProcessList::new(component_ids[4]);
        let memory_viz = MemoryViz::new(component_ids[5]);

        // Start with Chat + Agent layout
        let layout = Layout::chat_agent(component_ids[0], component_ids[1]);

        Self {
            config,
            event_bus,
            chat_panel,
            agent_panel,
            log_stream,
            topology_panel,
            process_list,
            memory_viz,
            layout,
            component_ids,
            focused_component: 0,
            shadow_rx,
        }
    }

    /// Main event loop
    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()>
    where
        std::io::Error: From<<B as Backend>::Error>,
    {
        loop {
            // Poll shadow events
            self.poll_shadow_events();

            // Render
            terminal.draw(|frame| self.render(frame))?;

            // Handle input
            if crossterm::event::poll(std::time::Duration::from_millis(16))? {
                if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
                    if self.handle_key(key)? {
                        return Ok(()); // Exit requested
                    }
                }
            }
        }
    }

    /// Poll shadow events and broadcast to components
    fn poll_shadow_events(&mut self) {
        poll_shadow_events(&mut self.shadow_rx, |ev| {
            let wrapper: ShadowEventWrapper = ev.into();
            self.event_bus.emit(SystemEvent::Shadow(wrapper));
        });
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Global shortcuts
        match (key.modifiers, key.code) {
            // Quit
            (_, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                return Ok(true);
            }

            // Layout switching (Ctrl+1 through Ctrl+4)
            (KeyModifiers::CONTROL, KeyCode::Char('1')) => {
                self.layout = Layout::chat_only(self.component_ids[0]);
                self.update_focus();
                return Ok(false);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('2')) => {
                self.layout = Layout::chat_agent(self.component_ids[0], self.component_ids[1]);
                self.update_focus();
                return Ok(false);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('3')) => {
                self.layout = Layout::full_monitoring(
                    self.component_ids[0],
                    self.component_ids[1],
                    self.component_ids[2],
                );
                self.update_focus();
                return Ok(false);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('4')) => {
                self.layout = Layout::dashboard(
                    self.component_ids[3],
                    self.component_ids[4],
                    self.component_ids[5],
                    self.component_ids[2],
                );
                self.update_focus();
                return Ok(false);
            }

            // Cycle layout (Tab)
            (_, KeyCode::Tab) => {
                self.layout.cycle_mode(&self.component_ids);
                self.update_focus();
                return Ok(false);
            }

            // Focus next component (Ctrl+Right or Ctrl+n)
            (KeyModifiers::CONTROL, KeyCode::Right)
            | (KeyModifiers::CONTROL, KeyCode::Char('n')) => {
                self.focus_next();
                return Ok(false);
            }

            // Focus previous component (Ctrl+Left or Ctrl+p)
            (KeyModifiers::CONTROL, KeyCode::Left)
            | (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                self.focus_prev();
                return Ok(false);
            }

            // Refresh (Ctrl+r)
            (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
                self.refresh();
                return Ok(false);
            }

            _ => {}
        }

        // Forward input to focused component
        let action = match self.focused_component {
            0 => self.chat_panel.handle_key(key),
            1 => crate::components::ComponentAction::None, // AgenticPanel doesn't implement Component trait yet
            2 => self.log_stream.handle_key(key),
            3 => self.topology_panel.handle_key(key),
            4 => self.process_list.handle_key(key),
            5 => self.memory_viz.handle_key(key),
            _ => crate::components::ComponentAction::None,
        };

        // Handle component actions
        use crate::components::ComponentAction;
        match action {
            ComponentAction::Exit => return Ok(true),
            ComponentAction::Refresh => self.refresh(),
            ComponentAction::SwitchTo(id) => {
                // Find component index by ID
                if let Some(idx) = self.component_ids.iter().position(|&cid| cid == id) {
                    self.set_focus(idx);
                }
            }
            _ => {}
        }

        Ok(false)
    }

    /// Render all components
    fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();

        // Render title bar
        self.render_title_bar(frame);

        // Compute component areas from layout
        let content_area = ratatui::layout::Rect {
            x: size.x,
            y: size.y + 1,
            width: size.width,
            height: size.height.saturating_sub(2),
        };

        let areas = LayoutRenderer::render(&self.layout, content_area);

        // Broadcast events to all components
        let mut subscriber = self.event_bus.subscribe();
        while let Some(event) = subscriber.try_recv() {
            self.chat_panel.handle_event(&event);
            self.log_stream.handle_event(&event);
            self.topology_panel.handle_event(&event);
            self.process_list.handle_event(&event);
            self.memory_viz.handle_event(&event);
        }

        // Render visible components based on layout mode
        match self.layout.mode {
            LayoutMode::Fullscreen => {
                // Only ChatPanel
                if !areas.is_empty() {
                    self.chat_panel.render(frame, areas[0]);
                }
            }
            LayoutMode::SideBySide { .. } => {
                // ChatPanel + AgenticPanel
                if areas.len() >= 2 {
                    self.chat_panel.render(frame, areas[0]);
                    self.agent_panel.render(frame, areas[1]);
                }
            }
            LayoutMode::Stacked { .. } => {
                // ChatPanel + AgenticPanel + LogStream
                if areas.len() >= 3 {
                    self.chat_panel.render(frame, areas[0]);
                    self.agent_panel.render(frame, areas[1]);
                    self.log_stream.render(frame, areas[2]);
                }
            }
            LayoutMode::Grid { .. } => {
                // Dashboard: TopologyPanel + ProcessList + MemoryViz + LogStream
                if areas.len() >= 4 {
                    self.topology_panel.render(frame, areas[0]);
                    self.process_list.render(frame, areas[1]);
                    self.memory_viz.render(frame, areas[2]);
                    self.log_stream.render(frame, areas[3]);
                }
            }
        }

        // Render status bar
        self.render_status_bar(frame);
    }

    /// Render title bar
    fn render_title_bar(&self, frame: &mut Frame) {
        use ratatui::{
            layout::Rect,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };

        let title_area = Rect {
            x: 0,
            y: 0,
            width: frame.area().width,
            height: 1,
        };

        let title = Line::from(vec![
            Span::styled(
                " ZAION TUI V2 ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("PID: {}", &self.config.pid[..8.min(self.config.pid.len())]),
                Style::default().fg(Color::Blue),
            ),
            Span::raw(" | "),
            Span::styled(&self.config.provider, Style::default().fg(Color::Magenta)),
            Span::raw(" | "),
            Span::styled(self.layout.mode_name(), Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled(
                format!("Focus: {}", self.focused_component_name()),
                Style::default().fg(Color::Green),
            ),
        ]);

        frame.render_widget(Paragraph::new(title), title_area);
    }

    /// Render status bar
    fn render_status_bar(&self, frame: &mut Frame) {
        use ratatui::{
            layout::Rect,
            style::{Color, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };

        let status_area = Rect {
            x: 0,
            y: frame.area().height.saturating_sub(1),
            width: frame.area().width,
            height: 1,
        };

        let status = Line::from(vec![
            Span::styled(" [Ctrl+1-4] Layout ", Style::default().fg(Color::Yellow)),
            Span::raw(" | "),
            Span::styled("[Tab] Cycle", Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled("[Ctrl+n/p] Focus", Style::default().fg(Color::Green)),
            Span::raw(" | "),
            Span::styled("[Ctrl+r] Refresh", Style::default().fg(Color::Blue)),
            Span::raw(" | "),
            Span::styled("[q] Quit", Style::default().fg(Color::Red)),
        ]);

        frame.render_widget(Paragraph::new(status), status_area);
    }

    /// Get name of currently focused component
    fn focused_component_name(&self) -> &str {
        match self.focused_component {
            0 => "Chat",
            1 => "Agent",
            2 => "Logs",
            3 => "Topology",
            4 => "Processes",
            5 => "Memory",
            _ => "Unknown",
        }
    }

    /// Focus next component in visible set
    fn focus_next(&mut self) {
        let visible_count = self.visible_component_count();
        if visible_count > 0 {
            self.focused_component = (self.focused_component + 1) % visible_count;
            self.update_focus();
        }
    }

    /// Focus previous component in visible set
    fn focus_prev(&mut self) {
        let visible_count = self.visible_component_count();
        if visible_count > 0 {
            self.focused_component = if self.focused_component == 0 {
                visible_count - 1
            } else {
                self.focused_component - 1
            };
            self.update_focus();
        }
    }

    /// Set focus to specific component index
    fn set_focus(&mut self, index: usize) {
        let visible_count = self.visible_component_count();
        if index < visible_count {
            self.focused_component = index;
            self.update_focus();
        }
    }

    /// Update component focus states
    fn update_focus(&mut self) {
        // Blur all components
        self.chat_panel.on_blur();
        self.log_stream.on_blur();
        self.topology_panel.on_blur();
        self.process_list.on_blur();
        self.memory_viz.on_blur();

        // Focus the active component
        match self.focused_component {
            0 => self.chat_panel.on_focus(),
            1 => {} // AgenticPanel doesn't implement Component trait yet
            2 => self.log_stream.on_focus(),
            3 => self.topology_panel.on_focus(),
            4 => self.process_list.on_focus(),
            5 => self.memory_viz.on_focus(),
            _ => {}
        }
    }

    /// Get count of visible components in current layout
    fn visible_component_count(&self) -> usize {
        match self.layout.mode {
            LayoutMode::Fullscreen => 1,
            LayoutMode::SideBySide { .. } => 2,
            LayoutMode::Stacked { .. } => 3,
            LayoutMode::Grid { .. } => {
                // Count unique components in grid
                match &self.layout.mode {
                    LayoutMode::Grid { rows } => {
                        let mut unique = std::collections::HashSet::new();
                        for row in rows {
                            for id in row {
                                unique.insert(id);
                            }
                        }
                        unique.len()
                    }
                    _ => 0,
                }
            }
        }
    }

    /// Refresh all components with latest data
    fn refresh(&mut self) {
        use crate::components::TimerEvent;

        // Emit refresh event
        self.event_bus
            .emit(SystemEvent::Timer(TimerEvent::PeriodicRefresh));

        // TODO: Load fresh data from data layer
        // - Load processes from database
        // - Load memory layers
        // - Load recent chat messages
        // - Load recent events
    }

    /// Get event bus reference for external event injection
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_app_creation() {
        use crate::run_tui_v2::TuiV2Config;

        let config = TuiV2Config {
            pid: "test_pid".to_string(),
            provider: "ollama".to_string(),
            model: None,
            memory: true,
            mcp: false,
            cache: false,
            smart_route: false,
        };

        let app = TuiApp::new(config, None);
        assert_eq!(app.focused_component, 0);
        assert!(matches!(app.layout.mode, LayoutMode::SideBySide { .. }));
        assert_eq!(app.component_ids.len(), 6);
    }

    #[test]
    fn test_focus_navigation() {
        use crate::run_tui_v2::TuiV2Config;

        let config = TuiV2Config {
            pid: "test_pid".to_string(),
            provider: "ollama".to_string(),
            model: None,
            memory: true,
            mcp: false,
            cache: false,
            smart_route: false,
        };

        let mut app = TuiApp::new(config, None);

        // Start with ChatAgent layout (2 visible components)
        assert_eq!(app.focused_component, 0);

        app.focus_next();
        assert_eq!(app.focused_component, 1);

        app.focus_next();
        assert_eq!(app.focused_component, 0); // Wraps around

        app.focus_prev();
        assert_eq!(app.focused_component, 1);
    }

    #[test]
    fn test_layout_switching() {
        use crate::run_tui_v2::TuiV2Config;

        let config = TuiV2Config {
            pid: "test_pid".to_string(),
            provider: "ollama".to_string(),
            model: None,
            memory: true,
            mcp: false,
            cache: false,
            smart_route: false,
        };

        let mut app = TuiApp::new(config, None);

        // Switch to Fullscreen
        app.layout = Layout::chat_only(app.component_ids[0]);
        assert_eq!(app.visible_component_count(), 1);

        // Switch to Dashboard
        app.layout = Layout::dashboard(
            app.component_ids[3],
            app.component_ids[4],
            app.component_ids[5],
            app.component_ids[2],
        );
        assert_eq!(app.visible_component_count(), 4);
    }

    #[test]
    fn test_event_bus_integration() {
        use crate::run_tui_v2::TuiV2Config;

        let config = TuiV2Config {
            pid: "test_pid".to_string(),
            provider: "ollama".to_string(),
            model: None,
            memory: true,
            mcp: false,
            cache: false,
            smart_route: false,
        };

        let app = TuiApp::new(config, None);
        let mut subscriber = app.event_bus().subscribe();

        app.event_bus()
            .emit(SystemEvent::Shadow(ShadowEventWrapper::ExecutorStarted));

        let event = subscriber.try_recv();
        assert!(event.is_some());
    }
}
