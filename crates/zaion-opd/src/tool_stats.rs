//! Tool statistics tracking and aggregation

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use crate::trajectory::ToolUsage;

/// Aggregated tool statistics across multiple trajectories
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolStats {
    /// Per-tool usage statistics
    pub tools: HashMap<String, ToolUsage>,

    /// Total number of tool calls
    pub total_calls: u32,

    /// Total successful calls
    pub total_success: u32,

    /// Total failed calls
    pub total_failure: u32,
}

impl ToolStats {
    /// Create new empty tool stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Add usage from a single tool
    pub fn add_tool_usage(&mut self, tool_name: String, usage: ToolUsage) {
        let entry = self.tools.entry(tool_name).or_default();
        entry.count += usage.count;
        entry.success += usage.success;
        entry.failure += usage.failure;

        self.total_calls += usage.count;
        self.total_success += usage.success;
        self.total_failure += usage.failure;
    }

    /// Merge another ToolStats into this one
    pub fn merge(&mut self, other: &ToolStats) {
        for (tool_name, usage) in &other.tools {
            self.add_tool_usage(tool_name.clone(), usage.clone());
        }
    }

    /// Get success rate (0.0-1.0)
    pub fn success_rate(&self) -> f32 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_success as f32 / self.total_calls as f32
        }
    }

    /// Get most used tools (sorted by count)
    pub fn top_tools(&self, n: usize) -> Vec<(String, ToolUsage)> {
        let mut tools: Vec<_> = self
            .tools
            .iter()
            .map(|(name, usage)| (name.clone(), usage.clone()))
            .collect();

        tools.sort_by(|a, b| b.1.count.cmp(&a.1.count));
        tools.truncate(n);
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_stats_creation() {
        let stats = ToolStats::new();
        assert_eq!(stats.total_calls, 0);
        assert_eq!(stats.tools.len(), 0);
    }

    #[test]
    fn test_add_tool_usage() {
        let mut stats = ToolStats::new();
        stats.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 5,
                success: 4,
                failure: 1,
            },
        );

        assert_eq!(stats.total_calls, 5);
        assert_eq!(stats.total_success, 4);
        assert_eq!(stats.total_failure, 1);
    }

    #[test]
    fn test_success_rate() {
        let mut stats = ToolStats::new();
        stats.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 10,
                success: 8,
                failure: 2,
            },
        );

        assert_eq!(stats.success_rate(), 0.8);
    }

    #[test]
    fn test_merge() {
        let mut stats1 = ToolStats::new();
        stats1.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 5,
                success: 4,
                failure: 1,
            },
        );

        let mut stats2 = ToolStats::new();
        stats2.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 3,
                success: 2,
                failure: 1,
            },
        );

        stats1.merge(&stats2);
        assert_eq!(stats1.total_calls, 8);
        assert_eq!(stats1.tools.get("read_file").unwrap().count, 8);
    }

    #[test]
    fn test_top_tools() {
        let mut stats = ToolStats::new();
        stats.add_tool_usage(
            "tool_a".to_string(),
            ToolUsage {
                count: 10,
                success: 10,
                failure: 0,
            },
        );
        stats.add_tool_usage(
            "tool_b".to_string(),
            ToolUsage {
                count: 5,
                success: 5,
                failure: 0,
            },
        );
        stats.add_tool_usage(
            "tool_c".to_string(),
            ToolUsage {
                count: 15,
                success: 15,
                failure: 0,
            },
        );

        let top = stats.top_tools(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "tool_c");
        assert_eq!(top[1].0, "tool_a");
    }
}
