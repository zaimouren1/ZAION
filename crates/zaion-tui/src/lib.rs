// TUI V2 crate - Modern terminal interface for Zaion Agent

pub mod agentic_panel;
pub mod components;
pub mod layout;
pub mod layout_renderer;
pub mod run_tui_v2;
pub mod state;
pub mod theme;
pub mod topo;
pub mod tui_app;

// Re-export key types
pub use agentic_panel::AgenticPanel;
pub use components::{ChatPanel, LogStream, MemoryViz, ProcessList, TopologyPanel};
pub use layout::{Layout, LayoutMode};
pub use layout_renderer::LayoutRenderer;
pub use state::event_bus::EventBus;
// Re-export types from components module
pub use components::{
    ChatMessage, ComponentAction, ComponentId, DataEvent, MemoryLayer, MessageRole, ProcessInfo,
    ShadowEventWrapper, SystemEvent, TimerEvent, ToolCallInfo, ToolCallStatus,
};
pub use run_tui_v2::{run_tui_v2, TuiV2Config};
pub use tui_app::TuiApp;

// Modern TUI exports

// Inline mode exports (Claude Code style)

pub mod streaming_renderer;
pub use streaming_renderer::{
    AgenticPhase,
    AutonomicResponse,
    CuriosityTrigger,
    StreamingRenderer,
    // Agentic Loop visualization types (Week 7)
    SystemStatus,
    SystemsStatus,
    ToolCallStatus as StreamingToolCallStatus,
};

// Brand: pixel "ZAION" wordmark + 9-row octopus mascot. Used by the TUI
// welcome frame (`streaming_renderer::render_welcome_condensed`) and by the
// CLI surfaces (--help, onboard, doctor) via `zaion_cli::commands::brand`.
pub mod brand;
pub use brand::{
    badge, octopus_banner, octopus_glyph, print_compact_banner, print_header, render_word_mark,
    zaion_wordmark, zaion_wordmark_lines,
};

pub mod shadow_adapter;
pub use shadow_adapter::{poll_shadow_events, ShadowEventAdapter};

// Theme system exports
pub use theme::{get_theme, ThemeName, ZaionTheme};

#[cfg(test)]
mod tests {
    // TUI V2 Integration Tests
    mod tui_v2_integration {

        use crate::components::*;
        use crate::layout::{Layout, LayoutMode};
        use crate::layout_renderer::LayoutRenderer;
        use crate::state::event_bus::EventBus;
        use ratatui::layout::Rect;
        use std::time::Instant;

        #[test]
        fn test_full_component_integration() {
            let event_bus = EventBus::new();
            let mut subscriber = event_bus.subscribe();

            let mut chat = chat_panel::ChatPanel::new(ComponentId(1));
            let mut log_stream = log_stream::LogStream::new(ComponentId(3));
            let mut topology = topology_panel::TopologyPanel::new(ComponentId(4));

            let msg = ChatMessage {
                role: MessageRole::User,
                content: "Hello, Zaion!".to_string(),
                timestamp: Instant::now(),
                thinking: Some("I'm thinking about this...".to_string()),
                tool_calls: vec![ToolCallInfo {
                    name: "read_file".to_string(),
                    status: ToolCallStatus::Success,
                    result: Some("File content here".to_string()),
                }],
            };
            event_bus.emit(SystemEvent::Data(DataEvent::MessageReceived(msg)));
            event_bus.emit(SystemEvent::Shadow(ShadowEventWrapper::ExecutorStarted));
            event_bus.emit(SystemEvent::Shadow(ShadowEventWrapper::TaskSpawned {
                task_id: "task_123".to_string(),
                name: "Test Task".to_string(),
            }));

            while let Some(event) = subscriber.try_recv() {
                chat.handle_event(&event);
                log_stream.handle_event(&event);
                topology.handle_event(&event);
            }

            assert_eq!(chat.name(), "Chat");
            assert_eq!(log_stream.name(), "Logs");
            assert_eq!(topology.name(), "Topology");
        }

        #[test]
        fn test_all_layout_modes() {
            let ids = [
                ComponentId(1),
                ComponentId(2),
                ComponentId(3),
                ComponentId(4),
                ComponentId(5),
                ComponentId(6),
            ];
            let area = Rect::new(0, 0, 120, 40);

            let layout1 = Layout::chat_only(ids[0]);
            assert_eq!(layout1.mode, LayoutMode::Fullscreen);
            let areas1 = LayoutRenderer::render(&layout1, area);
            assert_eq!(areas1.len(), 1);

            let layout2 = Layout::chat_agent(ids[0], ids[1]);
            let areas2 = LayoutRenderer::render(&layout2, area);
            assert_eq!(areas2.len(), 2);

            let layout3 = Layout::full_monitoring(ids[0], ids[1], ids[2]);
            let areas3 = LayoutRenderer::render(&layout3, area);
            assert_eq!(areas3.len(), 3);

            let layout4 = Layout::dashboard(ids[3], ids[4], ids[5], ids[2]);
            let areas4 = LayoutRenderer::render(&layout4, area);
            assert_eq!(areas4.len(), 4);
        }

        #[test]
        fn test_layout_cycling() {
            let ids = [
                ComponentId(1),
                ComponentId(2),
                ComponentId(3),
                ComponentId(4),
                ComponentId(5),
                ComponentId(6),
            ];
            let mut layout = Layout::chat_only(ids[0]);

            for expected_name in [
                "Chat Only",
                "Chat + Agent",
                "Full Monitoring",
                "Dashboard",
                "Chat Only",
            ] {
                assert_eq!(layout.mode_name(), expected_name);
                layout.cycle_mode(&ids);
            }
        }

        #[test]
        fn test_virtual_scrolling_with_large_dataset() {
            let mut log_stream = log_stream::LogStream::new(ComponentId(3));

            for i in 0..15_000 {
                let event = SystemEvent::Shadow(ShadowEventWrapper::TaskStarted {
                    task_id: format!("task_{}", i),
                    name: format!("Task {}", i),
                });
                log_stream.handle_event(&event);
            }

            assert_eq!(log_stream.name(), "Logs");
        }

        #[test]
        fn test_event_bus_broadcast() {
            let bus = EventBus::new();
            let mut sub1 = bus.subscribe();
            let mut sub2 = bus.subscribe();

            bus.emit(SystemEvent::Timer(TimerEvent::PeriodicRefresh));
            bus.emit(SystemEvent::Shadow(ShadowEventWrapper::ExecutorStarted));

            assert!(sub1.try_recv().is_some());
            assert!(sub1.try_recv().is_some());
            assert!(sub2.try_recv().is_some());
            assert!(sub2.try_recv().is_some());
        }

        #[test]
        fn test_component_focus_management() {
            let mut chat = chat_panel::ChatPanel::new(ComponentId(1));
            let mut log_stream = log_stream::LogStream::new(ComponentId(3));

            assert!(!chat.is_active());
            assert!(!log_stream.is_active());

            chat.on_focus();
            assert!(chat.is_active());

            chat.on_blur();
            log_stream.on_focus();
            assert!(!chat.is_active());
            assert!(log_stream.is_active());
        }
    }
}
