//! Layout system for TUI v2
//!
//! Manages panel arrangement and provides 4 default layout modes:
//! - Chat Only: Fullscreen chat
//! - Chat + Agent: Side-by-side development
//! - Full Monitoring: Chat with agent and logs
//! - Dashboard: 2x2 grid of all panels

use crate::components::ComponentId;
use ratatui::layout::Constraint;

/// Layout manager for component arrangement
#[derive(Debug, Clone)]
pub struct Layout {
    pub mode: LayoutMode,
    pub main_panel: ComponentId,
    pub side_panels: Vec<SidePanel>,
}

/// Layout mode variants
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutMode {
    /// Single panel fullscreen
    Fullscreen,
    /// Main panel + right side panel
    SideBySide { ratio: (u16, u16) },
    /// Main panel + stacked right panels
    Stacked { main_width: u16 },
    /// Custom grid layout
    Grid { rows: Vec<Vec<ComponentId>> },
}

/// Side panel configuration
#[derive(Debug, Clone)]
pub struct SidePanel {
    pub component: ComponentId,
    pub height: Constraint,
}

impl Layout {
    /// Create Chat Only layout (Mode 1)
    pub fn chat_only(chat_id: ComponentId) -> Self {
        Self {
            mode: LayoutMode::Fullscreen,
            main_panel: chat_id,
            side_panels: vec![],
        }
    }

    /// Create Chat + Agent layout (Mode 2)
    pub fn chat_agent(chat_id: ComponentId, agent_id: ComponentId) -> Self {
        Self {
            mode: LayoutMode::SideBySide { ratio: (1, 1) },
            main_panel: chat_id,
            side_panels: vec![SidePanel {
                component: agent_id,
                height: Constraint::Percentage(100),
            }],
        }
    }

    /// Create Full Monitoring layout (Mode 3)
    pub fn full_monitoring(
        chat_id: ComponentId,
        agent_id: ComponentId,
        log_id: ComponentId,
    ) -> Self {
        Self {
            mode: LayoutMode::Stacked { main_width: 50 },
            main_panel: chat_id,
            side_panels: vec![
                SidePanel {
                    component: agent_id,
                    height: Constraint::Percentage(50),
                },
                SidePanel {
                    component: log_id,
                    height: Constraint::Percentage(50),
                },
            ],
        }
    }

    /// Create Dashboard layout (Mode 4)
    pub fn dashboard(
        topology_id: ComponentId,
        process_id: ComponentId,
        memory_id: ComponentId,
        log_id: ComponentId,
    ) -> Self {
        Self {
            mode: LayoutMode::Grid {
                rows: vec![vec![topology_id, process_id], vec![memory_id, log_id]],
            },
            main_panel: topology_id, // Arbitrary choice for main_panel
            side_panels: vec![],     // Grid mode doesn't use side_panels
        }
    }

    /// Get all visible component IDs in this layout
    pub fn visible_components(&self) -> Vec<ComponentId> {
        match &self.mode {
            LayoutMode::Fullscreen => vec![self.main_panel],
            LayoutMode::SideBySide { .. } | LayoutMode::Stacked { .. } => {
                let mut components = vec![self.main_panel];
                components.extend(self.side_panels.iter().map(|sp| sp.component));
                components
            }
            LayoutMode::Grid { rows } => rows.iter().flat_map(|row| row.iter().copied()).collect(),
        }
    }

    /// Cycle to next layout mode
    pub fn cycle_mode(&mut self, all_component_ids: &[ComponentId; 6]) {
        // Assuming IDs in order: [chat, agent, log, topology, process, memory]
        let [chat_id, agent_id, log_id, topology_id, process_id, memory_id] = all_component_ids;

        *self = match &self.mode {
            LayoutMode::Fullscreen => Self::chat_agent(*chat_id, *agent_id),
            LayoutMode::SideBySide { .. } => Self::full_monitoring(*chat_id, *agent_id, *log_id),
            LayoutMode::Stacked { .. } => {
                Self::dashboard(*topology_id, *process_id, *memory_id, *log_id)
            }
            LayoutMode::Grid { .. } => Self::chat_only(*chat_id),
        };
    }

    /// Get human-readable layout name
    pub fn mode_name(&self) -> &str {
        match &self.mode {
            LayoutMode::Fullscreen => "Chat Only",
            LayoutMode::SideBySide { .. } => "Chat + Agent",
            LayoutMode::Stacked { .. } => "Full Monitoring",
            LayoutMode::Grid { .. } => "Dashboard",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_only_layout() {
        let layout = Layout::chat_only(ComponentId(1));
        assert_eq!(layout.mode, LayoutMode::Fullscreen);
        assert_eq!(layout.main_panel, ComponentId(1));
        assert!(layout.side_panels.is_empty());
        assert_eq!(layout.visible_components(), vec![ComponentId(1)]);
    }

    #[test]
    fn test_chat_agent_layout() {
        let layout = Layout::chat_agent(ComponentId(1), ComponentId(2));
        assert_eq!(layout.mode, LayoutMode::SideBySide { ratio: (1, 1) });
        assert_eq!(layout.main_panel, ComponentId(1));
        assert_eq!(layout.side_panels.len(), 1);
        assert_eq!(
            layout.visible_components(),
            vec![ComponentId(1), ComponentId(2)]
        );
    }

    #[test]
    fn test_full_monitoring_layout() {
        let layout = Layout::full_monitoring(ComponentId(1), ComponentId(2), ComponentId(3));
        assert_eq!(layout.mode, LayoutMode::Stacked { main_width: 50 });
        assert_eq!(layout.main_panel, ComponentId(1));
        assert_eq!(layout.side_panels.len(), 2);
        assert_eq!(
            layout.visible_components(),
            vec![ComponentId(1), ComponentId(2), ComponentId(3)]
        );
    }

    #[test]
    fn test_dashboard_layout() {
        let layout = Layout::dashboard(
            ComponentId(4),
            ComponentId(5),
            ComponentId(6),
            ComponentId(3),
        );
        assert!(matches!(layout.mode, LayoutMode::Grid { .. }));
        assert_eq!(layout.visible_components().len(), 4);
    }

    #[test]
    fn test_cycle_mode() {
        let ids = [
            ComponentId(1), // chat
            ComponentId(2), // agent
            ComponentId(3), // log
            ComponentId(4), // topology
            ComponentId(5), // process
            ComponentId(6), // memory
        ];

        let mut layout = Layout::chat_only(ids[0]);
        assert_eq!(layout.mode_name(), "Chat Only");

        layout.cycle_mode(&ids);
        assert_eq!(layout.mode_name(), "Chat + Agent");

        layout.cycle_mode(&ids);
        assert_eq!(layout.mode_name(), "Full Monitoring");

        layout.cycle_mode(&ids);
        assert_eq!(layout.mode_name(), "Dashboard");

        layout.cycle_mode(&ids);
        assert_eq!(layout.mode_name(), "Chat Only");
    }

    #[test]
    fn test_mode_names() {
        let layout1 = Layout::chat_only(ComponentId(1));
        assert_eq!(layout1.mode_name(), "Chat Only");

        let layout2 = Layout::chat_agent(ComponentId(1), ComponentId(2));
        assert_eq!(layout2.mode_name(), "Chat + Agent");

        let layout3 = Layout::full_monitoring(ComponentId(1), ComponentId(2), ComponentId(3));
        assert_eq!(layout3.mode_name(), "Full Monitoring");

        let layout4 = Layout::dashboard(
            ComponentId(4),
            ComponentId(5),
            ComponentId(6),
            ComponentId(3),
        );
        assert_eq!(layout4.mode_name(), "Dashboard");
    }
}
