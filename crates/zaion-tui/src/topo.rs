//! topo.rs — TopoPane 神经拓扑图 (minimal version for new TUI)
//!
//! Topology graph visualization for new TUI v2

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Core,
    TrinityArchitect,
    TrinityDeveloper,
    TrinityTester,
    Ouroboros,
    Aci,
    ShadowProcess(u8),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeStatus {
    Idle,
    Active,
    Success,
    Failed,
    Healing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopoNode {
    pub kind: NodeKind,
    pub label: String,
    pub status: NodeStatus,
    pub activity: String,
}

impl TopoNode {
    pub fn new(kind: NodeKind, label: impl Into<String>) -> Self {
        TopoNode {
            kind,
            label: label.into(),
            status: NodeStatus::Idle,
            activity: String::new(),
        }
    }

    pub fn color(&self) -> Color {
        match self.status {
            NodeStatus::Idle => Color::DarkGray,
            NodeStatus::Active => Color::Cyan,
            NodeStatus::Success => Color::Green,
            NodeStatus::Failed => Color::Red,
            NodeStatus::Healing => Color::Yellow,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self.kind {
            NodeKind::Core => "⬡",
            NodeKind::TrinityArchitect => "🏛",
            NodeKind::TrinityDeveloper => "⚙",
            NodeKind::TrinityTester => "🔬",
            NodeKind::Ouroboros => "🐍",
            NodeKind::Aci => "🔧",
            NodeKind::ShadowProcess(_) => "◈",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TopoGraph {
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<(usize, usize)>,
    pub selected: usize,
    pub last_event: Option<String>,
}

impl Default for TopoGraph {
    fn default() -> Self {
        Self::genesis()
    }
}

impl TopoGraph {
    pub fn genesis() -> Self {
        let nodes = vec![
            TopoNode::new(NodeKind::Core, "ZAION CORE"),
            TopoNode::new(NodeKind::Ouroboros, "Ouroboros"),
            TopoNode::new(NodeKind::Aci, "ACI 2.0"),
            TopoNode::new(NodeKind::TrinityArchitect, "Architect"),
            TopoNode::new(NodeKind::TrinityDeveloper, "Developer"),
            TopoNode::new(NodeKind::TrinityTester, "Tester"),
        ];

        let edges = vec![(0, 1), (0, 2), (0, 3), (0, 4), (0, 5), (3, 5), (4, 5)];

        TopoGraph {
            nodes,
            edges,
            selected: 0,
            last_event: None,
        }
    }

    pub fn activate_trinity(&mut self, task_desc: &str) {
        for i in 3..=5 {
            if let Some(node) = self.nodes.get_mut(i) {
                node.status = NodeStatus::Active;
                node.activity = format!("analyzing: {}", &task_desc[..task_desc.len().min(20)]);
            }
        }
        if let Some(core) = self.nodes.get_mut(0) {
            core.status = NodeStatus::Active;
            core.activity = "Trinity deliberation".into();
        }
        self.last_event = Some(format!(
            "Trinity split: {}",
            &task_desc[..task_desc.len().min(30)]
        ));
    }

    pub fn trigger_ouroboros(&mut self, error: &str) {
        if let Some(ouro) = self.nodes.get_mut(1) {
            ouro.status = NodeStatus::Healing;
            ouro.activity = format!("healing: {}", &error[..error.len().min(20)]);
        }
        self.last_event = Some(format!(
            "Ouroboros triggered: {}",
            &error[..error.len().min(30)]
        ));
    }

    pub fn ouroboros_healed(&mut self) {
        if let Some(ouro) = self.nodes.get_mut(1) {
            ouro.status = NodeStatus::Success;
            ouro.activity = "healed".into();
        }
        self.last_event = Some("Ouroboros healing complete".into());
    }

    pub fn add_shadow(&mut self, id: u8) {
        let node = TopoNode::new(NodeKind::ShadowProcess(id), format!("Shadow-{}", id));
        self.nodes.push(node);
        self.edges.push((0, self.nodes.len() - 1));
    }
}

pub struct TopoPane<'a> {
    graph: &'a TopoGraph,
}

impl<'a> TopoPane<'a> {
    pub fn new(graph: &'a TopoGraph) -> Self {
        TopoPane { graph }
    }
}

impl<'a> Widget for TopoPane<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 神经拓扑 Neural Topology ");
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 3 || inner.width < 10 {
            return;
        }

        // Render simplified list view
        let mut y = inner.y;
        for (i, node) in self.graph.nodes.iter().enumerate() {
            if y >= inner.y + inner.height {
                break;
            }

            let marker = if i == self.graph.selected { "▶" } else { " " };
            let line = format!(
                "{} {} {} - {}",
                marker,
                node.icon(),
                node.label,
                if node.activity.is_empty() {
                    "idle"
                } else {
                    &node.activity
                }
            );

            buf.set_string(
                inner.x,
                y,
                &line[..line.len().min(inner.width as usize)],
                Style::default().fg(node.color()),
            );
            y += 1;
        }

        // Last event
        if let Some(event) = &self.graph.last_event {
            if y < inner.y + inner.height {
                buf.set_string(
                    inner.x,
                    inner.y + inner.height - 1,
                    &format!("Event: {}", event)[..event.len().min(inner.width as usize)],
                    Style::default().fg(Color::Yellow),
                );
            }
        }
    }
}

pub fn topology_snapshot_lines(topo: &TopoGraph) -> Vec<String> {
    let mut lines = vec!["Zaion Neural Spine / 神经拓扑".to_string(), "━".repeat(60)];

    for node in &topo.nodes {
        let status_char = match node.status {
            NodeStatus::Idle => "○",
            NodeStatus::Active => "◉",
            NodeStatus::Success => "✓",
            NodeStatus::Failed => "✗",
            NodeStatus::Healing => "◐",
        };
        lines.push(format!(
            "  {} {} {} {}",
            status_char,
            node.icon(),
            node.label,
            if node.activity.is_empty() {
                ""
            } else {
                &node.activity
            }
        ));
    }

    if let Some(event) = &topo.last_event {
        lines.push(String::new());
        lines.push(format!("Last event: {}", event));
    }

    lines
}
