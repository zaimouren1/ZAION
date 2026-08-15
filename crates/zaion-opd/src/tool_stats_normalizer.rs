//! Tool statistics normalization
//!
//! Ensures consistent tool statistics schema across all trajectories
//! by normalizing to a fixed set of all possible tools.
//!
//! Based on Hermes batch_runner.py tool statistics normalization logic

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::tool_stats::{ToolStats, ToolUsage};

/// Normalized tool statistics with fixed schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedToolStats {
    /// Total tool calls across all tools
    pub total_calls: usize,

    /// Total successful calls
    pub total_success: usize,

    /// Per-tool statistics (all possible tools included)
    pub tools: HashMap<String, ToolUsage>,
}

impl NormalizedToolStats {
    /// Get success rate (0.0-1.0)
    pub fn success_rate(&self) -> f32 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_success as f32 / self.total_calls as f32
        }
    }
}

/// Tool statistics normalizer
pub struct ToolStatsNormalizer {
    /// All possible tool names (fixed schema)
    all_tools: Vec<String>,
}

impl ToolStatsNormalizer {
    /// Create normalizer with default tool set
    pub fn new() -> Self {
        Self {
            all_tools: Self::default_tool_set(),
        }
    }

    /// Create normalizer with custom tool set
    pub fn with_tools(tools: Vec<String>) -> Self {
        Self { all_tools: tools }
    }

    /// Default tool set (common tools across trajectories)
    fn default_tool_set() -> Vec<String> {
        vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "list_directory".to_string(),
            "execute_terminal".to_string(),
            "search_files".to_string(),
            "create_directory".to_string(),
            "delete_file".to_string(),
            "move_file".to_string(),
            "copy_file".to_string(),
            "get_file_info".to_string(),
        ]
    }

    /// Normalize tool statistics to fixed schema
    pub fn normalize(&self, stats: &ToolStats) -> NormalizedToolStats {
        let mut tools = HashMap::new();

        // Initialize all tools with zero usage
        for tool_name in &self.all_tools {
            tools.insert(
                tool_name.clone(),
                ToolUsage {
                    count: 0,
                    success: 0,
                    failure: 0,
                },
            );
        }

        // Fill in actual usage from stats
        for (tool_name, usage) in &stats.tools {
            if self.all_tools.contains(tool_name) {
                tools.insert(tool_name.clone(), usage.clone());
            } else {
                // Tool not in schema - add to "other" category or skip
                // For now, we skip unknown tools to maintain fixed schema
            }
        }

        NormalizedToolStats {
            total_calls: stats.total_calls as usize,
            total_success: stats.total_success as usize,
            tools,
        }
    }

    /// Normalize multiple tool statistics
    pub fn normalize_batch(&self, stats_list: &[ToolStats]) -> Vec<NormalizedToolStats> {
        stats_list.iter().map(|s| self.normalize(s)).collect()
    }

    /// Get all tool names in schema
    pub fn tool_names(&self) -> &[String] {
        &self.all_tools
    }

    /// Check if a tool is in the schema
    pub fn has_tool(&self, tool_name: &str) -> bool {
        self.all_tools.iter().any(|t| t == tool_name)
    }

    /// Add a tool to the schema
    pub fn add_tool(&mut self, tool_name: String) {
        if !self.has_tool(&tool_name) {
            self.all_tools.push(tool_name);
        }
    }

    /// Merge tool statistics from multiple trajectories
    pub fn merge_normalized(&self, stats_list: &[NormalizedToolStats]) -> NormalizedToolStats {
        let mut merged_tools = HashMap::new();

        // Initialize all tools with zero
        for tool_name in &self.all_tools {
            merged_tools.insert(
                tool_name.clone(),
                ToolUsage {
                    count: 0,
                    success: 0,
                    failure: 0,
                },
            );
        }

        let mut total_calls = 0;
        let mut total_success = 0;

        // Aggregate from all stats
        for stats in stats_list {
            total_calls += stats.total_calls;
            total_success += stats.total_success;

            for (tool_name, usage) in &stats.tools {
                if let Some(merged_usage) = merged_tools.get_mut(tool_name) {
                    merged_usage.count += usage.count;
                    merged_usage.success += usage.success;
                    merged_usage.failure += usage.failure;
                }
            }
        }

        NormalizedToolStats {
            total_calls,
            total_success,
            tools: merged_tools,
        }
    }
}

impl Default for ToolStatsNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalizer_creation() {
        let normalizer = ToolStatsNormalizer::new();
        assert_eq!(normalizer.tool_names().len(), 10);
        assert!(normalizer.has_tool("read_file"));
        assert!(normalizer.has_tool("write_file"));
    }

    #[test]
    fn test_normalize_empty_stats() {
        let normalizer = ToolStatsNormalizer::new();
        let stats = ToolStats::new();

        let normalized = normalizer.normalize(&stats);

        assert_eq!(normalized.total_calls, 0);
        assert_eq!(normalized.total_success, 0);
        assert_eq!(normalized.tools.len(), 10);

        // All tools should have zero usage
        for tool_name in normalizer.tool_names() {
            let usage = normalized.tools.get(tool_name).unwrap();
            assert_eq!(usage.count, 0);
            assert_eq!(usage.success, 0);
            assert_eq!(usage.failure, 0);
        }
    }

    #[test]
    fn test_normalize_with_usage() {
        let normalizer = ToolStatsNormalizer::new();
        let mut stats = ToolStats::new();

        stats.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 5,
                success: 4,
                failure: 1,
            },
        );

        let normalized = normalizer.normalize(&stats);

        assert_eq!(normalized.total_calls, 5);
        assert_eq!(normalized.total_success, 4);

        let read_usage = normalized.tools.get("read_file").unwrap();
        assert_eq!(read_usage.count, 5);
        assert_eq!(read_usage.success, 4);
        assert_eq!(read_usage.failure, 1);

        // Other tools should be zero
        let write_usage = normalized.tools.get("write_file").unwrap();
        assert_eq!(write_usage.count, 0);
    }

    #[test]
    fn test_normalize_unknown_tool() {
        let normalizer = ToolStatsNormalizer::new();
        let mut stats = ToolStats::new();

        // Add unknown tool
        stats.add_tool_usage(
            "unknown_tool".to_string(),
            ToolUsage {
                count: 3,
                success: 2,
                failure: 1,
            },
        );

        let normalized = normalizer.normalize(&stats);

        // Unknown tool should be skipped
        assert!(!normalized.tools.contains_key("unknown_tool"));
        assert_eq!(normalized.tools.len(), 10);
    }

    #[test]
    fn test_custom_tool_set() {
        let normalizer =
            ToolStatsNormalizer::with_tools(vec!["tool_a".to_string(), "tool_b".to_string()]);

        assert_eq!(normalizer.tool_names().len(), 2);
        assert!(normalizer.has_tool("tool_a"));
        assert!(normalizer.has_tool("tool_b"));
        assert!(!normalizer.has_tool("read_file"));
    }

    #[test]
    fn test_add_tool() {
        let mut normalizer = ToolStatsNormalizer::new();
        let initial_count = normalizer.tool_names().len();

        normalizer.add_tool("new_tool".to_string());

        assert_eq!(normalizer.tool_names().len(), initial_count + 1);
        assert!(normalizer.has_tool("new_tool"));

        // Adding same tool again should not duplicate
        normalizer.add_tool("new_tool".to_string());
        assert_eq!(normalizer.tool_names().len(), initial_count + 1);
    }

    #[test]
    fn test_normalize_batch() {
        let normalizer = ToolStatsNormalizer::new();

        let mut stats1 = ToolStats::new();
        stats1.add_tool_usage(
            "read_file".to_string(),
            ToolUsage {
                count: 2,
                success: 2,
                failure: 0,
            },
        );

        let mut stats2 = ToolStats::new();
        stats2.add_tool_usage(
            "write_file".to_string(),
            ToolUsage {
                count: 3,
                success: 3,
                failure: 0,
            },
        );

        let normalized = normalizer.normalize_batch(&[stats1, stats2]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].tools.get("read_file").unwrap().count, 2);
        assert_eq!(normalized[1].tools.get("write_file").unwrap().count, 3);
    }

    #[test]
    fn test_merge_normalized() {
        let normalizer = ToolStatsNormalizer::new();

        let mut stats1 = NormalizedToolStats {
            total_calls: 5,
            total_success: 4,
            tools: HashMap::new(),
        };
        stats1.tools.insert(
            "read_file".to_string(),
            ToolUsage {
                count: 5,
                success: 4,
                failure: 1,
            },
        );

        let mut stats2 = NormalizedToolStats {
            total_calls: 3,
            total_success: 3,
            tools: HashMap::new(),
        };
        stats2.tools.insert(
            "read_file".to_string(),
            ToolUsage {
                count: 3,
                success: 3,
                failure: 0,
            },
        );

        let merged = normalizer.merge_normalized(&[stats1, stats2]);

        assert_eq!(merged.total_calls, 8);
        assert_eq!(merged.total_success, 7);
        assert_eq!(merged.tools.get("read_file").unwrap().count, 8);
        assert_eq!(merged.tools.get("read_file").unwrap().success, 7);
        assert_eq!(merged.tools.get("read_file").unwrap().failure, 1);
    }
}
