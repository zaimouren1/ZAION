//! Topology panel component - refactored from v1
//!
//! Displays the neural topology graph with real-time updates from ShadowExecutor

use super::{Component, ComponentAction, ComponentId, ShadowEventWrapper, SystemEvent};
use crate::topo::{NodeKind, NodeStatus, TopoGraph};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

pub struct TopologyPanel {
    id: ComponentId,
    graph: TopoGraph,
    active: bool,
    visible: bool,
}

impl TopologyPanel {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            graph: TopoGraph::genesis(),
            active: false,
            visible: true,
        }
    }

    /// Apply shadow event to topology graph
    fn apply_shadow_event(&mut self, ev: &ShadowEventWrapper) {
        match ev {
            ShadowEventWrapper::ExecutorStarted => {
                if let Some(core) = self.graph.nodes.get_mut(0) {
                    core.status = NodeStatus::Active;
                    core.activity = "ShadowExecutor online".into();
                }
                self.graph.last_event = Some("ShadowExecutor started".into());
            }
            ShadowEventWrapper::ExecutorStopped => {
                if let Some(core) = self.graph.nodes.get_mut(0) {
                    core.status = NodeStatus::Idle;
                    core.activity = String::new();
                }
                self.graph.last_event = Some("ShadowExecutor stopped".into());
            }
            ShadowEventWrapper::TaskSpawned { task_id, name } => {
                let slot_id = task_id.as_bytes()[15.min(task_id.len().saturating_sub(1))] % 8;
                let exists =
                    self.graph.nodes.iter().any(
                        |node| matches!(node.kind, NodeKind::ShadowProcess(id) if id == slot_id),
                    );
                if !exists {
                    self.graph.add_shadow(slot_id);
                }
                if let Some(node) =
                    self.graph.nodes.iter_mut().find(
                        |node| matches!(node.kind, NodeKind::ShadowProcess(id) if id == slot_id),
                    )
                {
                    node.status = NodeStatus::Active;
                    node.activity = format!("queued: {}", truncate(name, 18));
                    node.label = format!("Shadow-{slot_id}");
                }
                self.graph.last_event = Some(format!("spawned: {}", truncate(name, 24)));
            }
            ShadowEventWrapper::TaskStarted { task_id, name } => {
                let slot_id = task_id.as_bytes()[15.min(task_id.len().saturating_sub(1))] % 8;
                if let Some(node) =
                    self.graph.nodes.iter_mut().find(
                        |node| matches!(node.kind, NodeKind::ShadowProcess(id) if id == slot_id),
                    )
                {
                    node.status = NodeStatus::Active;
                    node.activity = format!("running: {}", truncate(name, 18));
                }
                if let Some(aci) = self
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.kind == NodeKind::Aci)
                {
                    aci.status = NodeStatus::Active;
                    aci.activity = format!("gate: {}", truncate(name, 16));
                }
                self.graph.last_event = Some(format!("started: {}", truncate(name, 24)));
            }
            ShadowEventWrapper::TaskCompleted {
                task_id,
                name,
                success,
                duration_ms,
            } => {
                let slot_id = task_id.as_bytes()[15.min(task_id.len().saturating_sub(1))] % 8;
                if let Some(node) =
                    self.graph.nodes.iter_mut().find(
                        |node| matches!(node.kind, NodeKind::ShadowProcess(id) if id == slot_id),
                    )
                {
                    node.status = if *success {
                        NodeStatus::Success
                    } else {
                        NodeStatus::Failed
                    };
                    node.activity = format!(
                        "{}ms {}",
                        duration_ms,
                        if *success { "ok" } else { "failed" }
                    );
                }
                if let Some(aci) = self
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.kind == NodeKind::Aci)
                {
                    aci.status = if *success {
                        NodeStatus::Success
                    } else {
                        NodeStatus::Failed
                    };
                    aci.activity = if *success {
                        "gate passed".into()
                    } else {
                        "gate blocked".into()
                    };
                }
                self.graph.last_event = Some(format!(
                    "{} {} ({}ms)",
                    if *success { "completed" } else { "failed" },
                    name,
                    duration_ms
                ));
            }
            ShadowEventWrapper::TaskCancelled { task_id } => {
                let slot_id = task_id.as_bytes()[15.min(task_id.len().saturating_sub(1))] % 8;
                if let Some(node) =
                    self.graph.nodes.iter_mut().find(
                        |node| matches!(node.kind, NodeKind::ShadowProcess(id) if id == slot_id),
                    )
                {
                    node.status = NodeStatus::Idle;
                    node.activity = "cancelled".into();
                }
                self.graph.last_event = Some(format!("cancelled shadow-{slot_id}"));
            }
            ShadowEventWrapper::AciOperation { task_id: _, op, ok } => {
                if let Some(aci) = self
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|node| node.kind == NodeKind::Aci)
                {
                    aci.status = if *ok {
                        NodeStatus::Active
                    } else {
                        NodeStatus::Failed
                    };
                    aci.activity =
                        format!("{} {}", if *ok { "ok" } else { "failed" }, truncate(op, 18));
                }
                self.graph.last_event = Some(format!(
                    "ACI {} {}",
                    if *ok { "ok" } else { "FAIL" },
                    truncate(op, 28)
                ));
            }
        }
    }
}

impl Component for TopologyPanel {
    fn name(&self) -> &str {
        "Topology"
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction {
        match key.code {
            KeyCode::Char('t') => {
                self.graph.activate_trinity("Example refactor task");
                ComponentAction::None
            }
            KeyCode::Char('o') => {
                self.graph.trigger_ouroboros("Config parse error");
                ComponentAction::None
            }
            KeyCode::Char('h') => {
                self.graph.ouroboros_healed();
                ComponentAction::None
            }
            _ => ComponentAction::None,
        }
    }

    fn handle_event(&mut self, event: &SystemEvent) {
        if let SystemEvent::Shadow(ev) = event {
            self.apply_shadow_event(ev);
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use crate::topo::TopoPane;
        frame.render_widget(TopoPane::new(&self.graph), area);
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
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_creation() {
        let panel = TopologyPanel::new(ComponentId(1));
        assert_eq!(panel.name(), "Topology");
        assert!(!panel.is_active());
        assert!(panel.is_visible());
    }

    #[test]
    fn test_shadow_event_handling() {
        let mut panel = TopologyPanel::new(ComponentId(1));
        let event = SystemEvent::Shadow(ShadowEventWrapper::ExecutorStarted);
        panel.handle_event(&event);

        assert_eq!(
            panel.graph.last_event,
            Some("ShadowExecutor started".to_string())
        );
    }
}
