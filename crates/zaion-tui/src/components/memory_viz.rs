//! Memory visualization component - 7-layer memory display
//!
//! Displays memory layers with bar charts and real-time updates.

use super::{Component, ComponentAction, ComponentId, DataEvent, MemoryLayer, SystemEvent};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

pub struct MemoryViz {
    id: ComponentId,
    layers: Vec<MemoryLayer>,
    active: bool,
    visible: bool,
    selected_layer: Option<u8>,
    show_details: bool,
}

impl MemoryViz {
    pub fn new(id: ComponentId) -> Self {
        Self {
            id,
            layers: vec![
                MemoryLayer {
                    layer: 0,
                    label: "L0 Working Memory".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 1,
                    label: "L1 Session Memory".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 2,
                    label: "L2 Skill Memory".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 3,
                    label: "L3 Projection".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 4,
                    label: "L4 Episodic (Ledger)".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 5,
                    label: "L5 Semantic (Vector)".to_string(),
                    count: 0,
                },
                MemoryLayer {
                    layer: 6,
                    label: "L6 Principal (Ed25519)".to_string(),
                    count: 0,
                },
            ],
            active: false,
            visible: true,
            selected_layer: None,
            show_details: false,
        }
    }

    fn select_next(&mut self) {
        let new_layer = match self.selected_layer {
            Some(layer) if layer < 6 => layer + 1,
            None => 0,
            _ => return,
        };
        self.selected_layer = Some(new_layer);
    }

    fn select_prev(&mut self) {
        let new_layer = match self.selected_layer {
            Some(layer) if layer > 0 => layer - 1,
            None => 0,
            _ => return,
        };
        self.selected_layer = Some(new_layer);
    }

    fn toggle_details(&mut self) {
        self.show_details = !self.show_details;
    }

    fn total_count(&self) -> usize {
        self.layers.iter().map(|l| l.count).sum()
    }

    fn total_size_mb(&self) -> f64 {
        // Rough estimate: 100 bytes per item average
        (self.total_count() as f64 * 100.0) / (1024.0 * 1024.0)
    }

    fn render_bar(&self, count: usize, max_count: usize, width: usize) -> String {
        if max_count == 0 {
            return "░".repeat(width);
        }

        let filled = ((count as f64 / max_count as f64) * width as f64) as usize;
        let filled = filled.min(width);
        let empty = width - filled;

        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }
}

impl Component for MemoryViz {
    fn name(&self) -> &str {
        "Memory"
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
            KeyCode::Enter | KeyCode::Char('d') => {
                self.toggle_details();
                ComponentAction::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_layer = Some(0);
                ComponentAction::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_layer = Some(6);
                ComponentAction::None
            }
            _ => ComponentAction::None,
        }
    }

    fn handle_event(&mut self, event: &SystemEvent) {
        if let SystemEvent::Data(DataEvent::MemoryUpdated(layers)) = event {
            // Update layer counts from incoming data
            for new_layer in layers {
                if let Some(existing) = self.layers.iter_mut().find(|l| l.layer == new_layer.layer)
                {
                    existing.count = new_layer.count;
                    if !new_layer.label.is_empty() {
                        existing.label = new_layer.label.clone();
                    }
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::{
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Paragraph},
        };

        let max_count = self.layers.iter().map(|l| l.count).max().unwrap_or(0);
        let bar_width = area.width.saturating_sub(35) as usize;

        let mut lines = Vec::new();

        for layer in &self.layers {
            let is_selected = self.selected_layer == Some(layer.layer);

            let color = match layer.layer {
                0 => Color::Red,
                1 => Color::Yellow,
                2 => Color::Green,
                3 => Color::Cyan,
                4 => Color::Blue,
                5 => Color::Magenta,
                _ => Color::White,
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let bar = self.render_bar(layer.count, max_count, bar_width);

            lines.push(Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<24}", layer.label),
                    if is_selected {
                        Style::default().fg(color).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(color)
                    },
                ),
                Span::raw(format!("{:>5} ", layer.count)),
                Span::styled(bar, Style::default().fg(color)),
            ]));

            // Show details for selected layer
            if is_selected && self.show_details {
                lines.push(Line::from(vec![Span::styled(
                    format!("    Layer {} details: {} items", layer.layer, layer.count),
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} items", self.total_count()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" | "),
            Span::styled(
                format!("{:.2} MB", self.total_size_mb()),
                Style::default().fg(Color::Cyan),
            ),
        ]));

        let title = format!(
            "Memory Layers {} [↑↓ select | Enter details]",
            if self.show_details { "(details)" } else { "" }
        );

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(if self.active {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        );

        frame.render_widget(paragraph, area);
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
    fn test_memory_viz_creation() {
        let viz = MemoryViz::new(ComponentId(6));
        assert_eq!(viz.name(), "Memory");
        assert!(!viz.is_active());
        assert!(viz.is_visible());
        assert_eq!(viz.layers.len(), 7);
        assert_eq!(viz.selected_layer, None);
    }

    #[test]
    fn test_layer_selection() {
        let mut viz = MemoryViz::new(ComponentId(6));

        viz.select_next();
        assert_eq!(viz.selected_layer, Some(0));

        viz.select_next();
        assert_eq!(viz.selected_layer, Some(1));

        viz.select_prev();
        assert_eq!(viz.selected_layer, Some(0));
    }

    #[test]
    fn test_memory_update() {
        let mut viz = MemoryViz::new(ComponentId(6));

        let updates = vec![
            MemoryLayer {
                layer: 2,
                label: "L2 Skill Memory".to_string(),
                count: 28,
            },
            MemoryLayer {
                layer: 5,
                label: "L5 Semantic (Vector)".to_string(),
                count: 142,
            },
        ];

        viz.handle_event(&SystemEvent::Data(DataEvent::MemoryUpdated(updates)));

        assert_eq!(viz.layers[2].count, 28);
        assert_eq!(viz.layers[5].count, 142);
        assert_eq!(viz.total_count(), 170);
    }

    #[test]
    fn test_bar_rendering() {
        let viz = MemoryViz::new(ComponentId(6));

        let bar = viz.render_bar(50, 100, 20);
        assert_eq!(bar.chars().count(), 20); // Use chars().count() for UTF-8
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));

        let bar_empty = viz.render_bar(0, 100, 20);
        assert_eq!(bar_empty, "░".repeat(20));

        let bar_full = viz.render_bar(100, 100, 20);
        assert_eq!(bar_full, "█".repeat(20));
    }

    #[test]
    fn test_details_toggle() {
        let mut viz = MemoryViz::new(ComponentId(6));

        assert!(!viz.show_details);
        viz.toggle_details();
        assert!(viz.show_details);
        viz.toggle_details();
        assert!(!viz.show_details);
    }
}
