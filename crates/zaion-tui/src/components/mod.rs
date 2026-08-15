//! Component system for TUI v2
//!
//! This module defines the core Component trait and related types for
//! building modular, event-driven TUI panels.

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

/// Unique identifier for a component
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId(pub u32);

/// Action returned by component event handlers
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentAction {
    /// No action needed
    None,
    /// Exit the application
    Exit,
    /// Switch focus to another component
    SwitchTo(ComponentId),
    /// Refresh data from data store
    Refresh,
    /// Toggle component visibility
    ToggleVisible,
}

/// System events that components can handle
#[derive(Debug, Clone)]
pub enum SystemEvent {
    /// ShadowExecutor events (real-time runtime updates)
    Shadow(ShadowEventWrapper),
    /// Data layer updates
    Data(DataEvent),
    /// Timer/periodic events
    Timer(TimerEvent),
}

/// Wrapper for zaion_shadow::ShadowEvent
#[derive(Debug, Clone)]
pub enum ShadowEventWrapper {
    ExecutorStarted,
    ExecutorStopped,
    TaskSpawned {
        task_id: String,
        name: String,
    },
    TaskStarted {
        task_id: String,
        name: String,
    },
    TaskCompleted {
        task_id: String,
        name: String,
        success: bool,
        duration_ms: u64,
    },
    TaskCancelled {
        task_id: String,
    },
    AciOperation {
        task_id: String,
        op: String,
        ok: bool,
    },
}

/// Data layer events
#[derive(Debug, Clone)]
pub enum DataEvent {
    /// Process list updated
    ProcessesUpdated(Vec<ProcessInfo>),
    /// Events updated for a principal
    EventsUpdated(Vec<EventInfo>),
    /// Memory layers updated
    MemoryUpdated(Vec<MemoryLayer>),
    /// New message received (for chat panel)
    MessageReceived(ChatMessage),
}

/// Timer events
#[derive(Debug, Clone)]
pub enum TimerEvent {
    /// Periodic refresh (every N seconds)
    PeriodicRefresh,
    /// Auto-scroll trigger
    AutoScroll,
}

/// Process information for display
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub principal_id: String,
    pub state: String,
    pub workspace: String,
    pub project: String,
}

/// Event information for display
#[derive(Debug, Clone)]
pub struct EventInfo {
    pub event_id: String,
    pub event_type: String,
    pub created_at: String,
}

/// Memory layer information
#[derive(Debug, Clone, Default)]
pub struct MemoryLayer {
    pub layer: u8,
    pub label: String,
    pub count: usize,
}

/// Chat message for ChatPanel
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: std::time::Instant,
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub status: ToolCallStatus,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    Success,
    Failed,
}

impl std::fmt::Display for ToolCallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallStatus::Pending => write!(f, "Pending"),
            ToolCallStatus::Success => write!(f, "✓"),
            ToolCallStatus::Failed => write!(f, "✗"),
        }
    }
}

/// Core trait for all TUI components
pub trait Component {
    /// Component name (for debugging)
    fn name(&self) -> &str;

    /// Component unique ID
    fn id(&self) -> ComponentId;

    /// Handle keyboard input
    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction {
        let _ = key;
        ComponentAction::None
    }

    /// Handle system events (ShadowEvent, data updates, timers)
    fn handle_event(&mut self, event: &SystemEvent) {
        let _ = event;
    }

    /// Render the component
    fn render(&mut self, frame: &mut Frame, area: Rect);

    /// Whether this component is active (receives keyboard input)
    fn is_active(&self) -> bool {
        false
    }

    /// Whether this component is visible
    fn is_visible(&self) -> bool {
        true
    }

    /// Called when component gains focus
    fn on_focus(&mut self) {}

    /// Called when component loses focus
    fn on_blur(&mut self) {}

    /// Called when component is mounted
    fn on_mount(&mut self) {}

    /// Called when component is unmounted
    fn on_unmount(&mut self) {}
}

/// Helper to convert zaion_shadow::ShadowEvent to wrapper
impl From<zaion_shadow::ShadowEvent> for ShadowEventWrapper {
    fn from(ev: zaion_shadow::ShadowEvent) -> Self {
        match ev {
            zaion_shadow::ShadowEvent::ExecutorStarted => ShadowEventWrapper::ExecutorStarted,
            zaion_shadow::ShadowEvent::ExecutorStopped => ShadowEventWrapper::ExecutorStopped,
            zaion_shadow::ShadowEvent::TaskSpawned { task_id, name } => {
                ShadowEventWrapper::TaskSpawned {
                    task_id: task_id.to_string(),
                    name,
                }
            }
            zaion_shadow::ShadowEvent::TaskStarted { task_id, name } => {
                ShadowEventWrapper::TaskStarted {
                    task_id: task_id.to_string(),
                    name,
                }
            }
            zaion_shadow::ShadowEvent::TaskCompleted {
                task_id,
                name,
                success,
                duration_ms,
            } => ShadowEventWrapper::TaskCompleted {
                task_id: task_id.to_string(),
                name,
                success,
                duration_ms,
            },
            zaion_shadow::ShadowEvent::TaskCancelled { task_id } => {
                ShadowEventWrapper::TaskCancelled {
                    task_id: task_id.to_string(),
                }
            }
            zaion_shadow::ShadowEvent::AciOperation { task_id, op, ok } => {
                ShadowEventWrapper::AciOperation {
                    task_id: task_id.to_string(),
                    op,
                    ok,
                }
            }
        }
    }
}

// Export component implementations
pub mod chat_panel;
pub mod log_stream;
pub mod memory_viz;
pub mod process_list;
pub mod topology_panel;

pub use chat_panel::ChatPanel;
pub use log_stream::{LogEntry, LogLevel, LogStream};
pub use memory_viz::MemoryViz;
pub use process_list::ProcessList;
pub use topology_panel::TopologyPanel;
