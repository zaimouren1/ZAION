//! Layout renderer for TUI v2
//!
//! Converts Layout configurations into actual Ratatui layout chunks.

use crate::layout::{Layout, LayoutMode};
use ratatui::layout::{Constraint, Direction, Rect};

/// Render a layout into Ratatui layout chunks
pub struct LayoutRenderer;

impl LayoutRenderer {
    /// Split area according to layout mode and return component areas
    pub fn render(layout: &Layout, area: Rect) -> Vec<Rect> {
        match &layout.mode {
            LayoutMode::Fullscreen => vec![area],
            LayoutMode::SideBySide { ratio } => Self::render_side_by_side(layout, area, *ratio),
            LayoutMode::Stacked { main_width } => Self::render_stacked(layout, area, *main_width),
            LayoutMode::Grid { rows } => Self::render_grid(rows, area),
        }
    }

    fn render_side_by_side(_layout: &Layout, area: Rect, ratio: (u16, u16)) -> Vec<Rect> {
        let total = ratio.0 + ratio.1;
        let left_pct = (ratio.0 as f32 / total as f32 * 100.0) as u16;
        let right_pct = 100 - left_pct;

        let chunks = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(left_pct),
                Constraint::Percentage(right_pct),
            ])
            .split(area);

        vec![chunks[0], chunks[1]]
    }

    fn render_stacked(layout: &Layout, area: Rect, main_width: u16) -> Vec<Rect> {
        let side_width = 100 - main_width;

        let horizontal = ratatui::layout::Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(main_width),
                Constraint::Percentage(side_width),
            ])
            .split(area);

        let main_area = horizontal[0];
        let side_area = horizontal[1];

        // Split side area vertically based on side_panels
        let mut constraints = Vec::new();
        for panel in &layout.side_panels {
            constraints.push(panel.height);
        }

        if constraints.is_empty() {
            return vec![main_area];
        }

        let side_chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(side_area);

        let mut result = vec![main_area];
        result.extend(side_chunks.iter().copied());
        result
    }

    fn render_grid(rows: &[Vec<crate::components::ComponentId>], area: Rect) -> Vec<Rect> {
        if rows.is_empty() {
            return vec![area];
        }

        let row_count = rows.len();
        let row_constraints = vec![Constraint::Percentage((100 / row_count) as u16); row_count];

        let row_chunks = ratatui::layout::Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(area);

        let mut result = Vec::new();
        for (row_idx, row) in rows.iter().enumerate() {
            let col_count = row.len();
            if col_count == 0 {
                continue;
            }

            let col_constraints = vec![Constraint::Percentage((100 / col_count) as u16); col_count];

            let col_chunks = ratatui::layout::Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints)
                .split(row_chunks[row_idx]);

            result.extend(col_chunks.iter().copied());
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::ComponentId;
    use crate::layout::Layout;

    #[test]
    fn test_fullscreen_layout() {
        let layout = Layout::chat_only(ComponentId(1));
        let area = Rect::new(0, 0, 100, 50);
        let areas = LayoutRenderer::render(&layout, area);
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0], area);
    }

    #[test]
    fn test_side_by_side_layout() {
        let layout = Layout::chat_agent(ComponentId(1), ComponentId(2));
        let area = Rect::new(0, 0, 100, 50);
        let areas = LayoutRenderer::render(&layout, area);
        assert_eq!(areas.len(), 2);
        // Should be split roughly 50/50
        assert!(areas[0].width >= 45 && areas[0].width <= 55);
        assert!(areas[1].width >= 45 && areas[1].width <= 55);
    }

    #[test]
    fn test_stacked_layout() {
        let layout = Layout::full_monitoring(ComponentId(1), ComponentId(2), ComponentId(3));
        let area = Rect::new(0, 0, 100, 50);
        let areas = LayoutRenderer::render(&layout, area);
        assert_eq!(areas.len(), 3);
        // Main area should be 50% width
        assert_eq!(areas[0].width, 50);
        // Two side panels should split the remaining height
        assert!(areas[1].height > 0);
        assert!(areas[2].height > 0);
    }

    #[test]
    fn test_grid_layout() {
        let layout = Layout::dashboard(
            ComponentId(4),
            ComponentId(5),
            ComponentId(6),
            ComponentId(3),
        );
        let area = Rect::new(0, 0, 100, 50);
        let areas = LayoutRenderer::render(&layout, area);
        assert_eq!(areas.len(), 4);
        // Should be 2x2 grid
        for area in &areas {
            assert!(area.width > 0);
            assert!(area.height > 0);
        }
    }
}
