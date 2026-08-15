//! Toolset distribution sampling for diverse trajectory generation
//!
//! Implements weighted sampling of tool combinations to ensure training data
//! covers diverse tool usage patterns. Based on Hermes batch_runner.py
//! toolset_distributions logic.
//!
//! Key features:
//! - Weighted sampling of tool combinations
//! - Configurable tool availability per trajectory
//! - Statistics tracking for distribution balance

use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

/// A single toolset configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Toolset {
    /// Name/ID of this toolset
    pub name: String,

    /// List of available tools in this toolset
    pub tools: Vec<String>,

    /// Sampling weight (higher = more likely to be selected)
    pub weight: f32,
}

impl Toolset {
    /// Canonical allow-list of tool names for this toolset.
    ///
    /// `execute_terminal` is normalized to the actual runtime tool name
    /// `terminal` so the sampled toolset and executor policy stay aligned.
    pub fn allowed_tools(&self) -> Vec<String> {
        let tools: BTreeSet<String> = self
            .tools
            .iter()
            .map(|tool| canonical_tool_name(tool).to_string())
            .collect();
        tools.into_iter().collect()
    }

    /// Canonical allow-list as a set.
    pub fn allowed_tool_set(&self) -> HashSet<String> {
        self.allowed_tools().into_iter().collect()
    }
}

/// Toolset distribution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsetDistribution {
    /// Available toolsets with weights
    pub toolsets: Vec<Toolset>,

    /// Total weight (computed from toolsets)
    #[serde(skip)]
    total_weight: f32,
}

impl ToolsetDistribution {
    /// Create a new toolset distribution
    pub fn new(toolsets: Vec<Toolset>) -> Self {
        let total_weight = toolsets.iter().map(|t| t.weight).sum();
        Self {
            toolsets,
            total_weight,
        }
    }

    /// Sample a toolset according to weights
    pub fn sample(&self) -> Result<&Toolset> {
        if self.toolsets.is_empty() {
            anyhow::bail!("No toolsets available");
        }

        let mut rng = rand::thread_rng();
        let mut threshold = rng.gen::<f32>() * self.total_weight;

        for toolset in &self.toolsets {
            threshold -= toolset.weight;
            if threshold <= 0.0 {
                return Ok(toolset);
            }
        }

        // Fallback to last toolset (handles floating point edge cases)
        Ok(&self.toolsets[self.toolsets.len() - 1])
    }

    /// Sample N toolsets (with replacement)
    pub fn sample_n(&self, n: usize) -> Result<Vec<Toolset>> {
        let mut samples = Vec::with_capacity(n);
        for _ in 0..n {
            samples.push(self.sample()?.clone());
        }
        Ok(samples)
    }

    /// Get statistics about toolset usage from samples
    pub fn compute_stats(&self, samples: &[Toolset]) -> ToolsetStats {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for sample in samples {
            *counts.entry(sample.name.clone()).or_insert(0) += 1;
        }

        ToolsetStats {
            total_samples: samples.len(),
            toolset_counts: counts,
        }
    }

    /// Create default distribution (all tools available)
    pub fn default_full_toolset() -> Self {
        Self::new(vec![Toolset {
            name: "full".to_string(),
            tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "list_directory".to_string(),
                "execute_terminal".to_string(),
                "search_files".to_string(),
            ],
            weight: 1.0,
        }])
    }

    /// Create Hermes-style distribution (varied tool availability)
    pub fn hermes_style() -> Self {
        Self::new(vec![
            // Full toolset (most common)
            Toolset {
                name: "full".to_string(),
                tools: vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "list_directory".to_string(),
                    "execute_terminal".to_string(),
                    "search_files".to_string(),
                ],
                weight: 0.5,
            },
            // Read-only (no write/execute)
            Toolset {
                name: "read_only".to_string(),
                tools: vec![
                    "read_file".to_string(),
                    "list_directory".to_string(),
                    "search_files".to_string(),
                ],
                weight: 0.2,
            },
            // No terminal (safer environment)
            Toolset {
                name: "no_terminal".to_string(),
                tools: vec![
                    "read_file".to_string(),
                    "write_file".to_string(),
                    "list_directory".to_string(),
                    "search_files".to_string(),
                ],
                weight: 0.2,
            },
            // Minimal (only read)
            Toolset {
                name: "minimal".to_string(),
                tools: vec!["read_file".to_string()],
                weight: 0.1,
            },
        ])
    }
}

pub(crate) fn canonical_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "execute_terminal" => "terminal",
        other => other,
    }
}

/// Statistics about toolset sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsetStats {
    /// Total number of samples
    pub total_samples: usize,

    /// Count of each toolset sampled
    pub toolset_counts: HashMap<String, usize>,
}

impl ToolsetStats {
    /// Get sampling frequency for a toolset
    pub fn frequency(&self, toolset_name: &str) -> f32 {
        if self.total_samples == 0 {
            return 0.0;
        }
        let count = self.toolset_counts.get(toolset_name).copied().unwrap_or(0);
        count as f32 / self.total_samples as f32
    }

    /// Check if distribution is balanced (no toolset > 70% or < 5%)
    pub fn is_balanced(&self) -> bool {
        for count in self.toolset_counts.values() {
            let freq = *count as f32 / self.total_samples as f32;
            if !(0.05..=0.7).contains(&freq) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toolset_creation() {
        let toolset = Toolset {
            name: "test".to_string(),
            tools: vec!["read".to_string(), "write".to_string()],
            weight: 1.0,
        };
        assert_eq!(toolset.name, "test");
        assert_eq!(toolset.tools.len(), 2);
    }

    #[test]
    fn test_distribution_sample() {
        let dist = ToolsetDistribution::new(vec![
            Toolset {
                name: "a".to_string(),
                tools: vec!["tool1".to_string()],
                weight: 1.0,
            },
            Toolset {
                name: "b".to_string(),
                tools: vec!["tool2".to_string()],
                weight: 1.0,
            },
        ]);

        // Sample should return one of the toolsets
        let sample = dist.sample().unwrap();
        assert!(sample.name == "a" || sample.name == "b");
    }

    #[test]
    fn test_distribution_sample_n() {
        let dist = ToolsetDistribution::new(vec![Toolset {
            name: "only".to_string(),
            tools: vec!["tool".to_string()],
            weight: 1.0,
        }]);

        let samples = dist.sample_n(10).unwrap();
        assert_eq!(samples.len(), 10);
        assert!(samples.iter().all(|s| s.name == "only"));
    }

    #[test]
    fn test_weighted_sampling() {
        let dist = ToolsetDistribution::new(vec![
            Toolset {
                name: "heavy".to_string(),
                tools: vec!["tool1".to_string()],
                weight: 9.0,
            },
            Toolset {
                name: "light".to_string(),
                tools: vec!["tool2".to_string()],
                weight: 1.0,
            },
        ]);

        // Sample many times and check distribution
        let samples = dist.sample_n(1000).unwrap();
        let stats = dist.compute_stats(&samples);

        let heavy_freq = stats.frequency("heavy");
        let light_freq = stats.frequency("light");

        // Heavy should be ~90%, light ~10% (with some variance)
        assert!(heavy_freq > 0.8 && heavy_freq < 0.95);
        assert!(light_freq > 0.05 && light_freq < 0.2);
    }

    #[test]
    fn test_stats_frequency() {
        let samples = vec![
            Toolset {
                name: "a".to_string(),
                tools: vec![],
                weight: 1.0,
            },
            Toolset {
                name: "a".to_string(),
                tools: vec![],
                weight: 1.0,
            },
            Toolset {
                name: "b".to_string(),
                tools: vec![],
                weight: 1.0,
            },
        ];

        let dist = ToolsetDistribution::new(vec![]);
        let stats = dist.compute_stats(&samples);

        assert_eq!(stats.total_samples, 3);
        assert!((stats.frequency("a") - 0.666).abs() < 0.01);
        assert!((stats.frequency("b") - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_default_full_toolset() {
        let dist = ToolsetDistribution::default_full_toolset();
        assert_eq!(dist.toolsets.len(), 1);
        assert_eq!(dist.toolsets[0].name, "full");
        assert_eq!(dist.toolsets[0].tools.len(), 5);
    }

    #[test]
    fn test_hermes_style_distribution() {
        let dist = ToolsetDistribution::hermes_style();
        assert_eq!(dist.toolsets.len(), 4);

        // Check weights sum to 1.0
        let total: f32 = dist.toolsets.iter().map(|t| t.weight).sum();
        assert!((total - 1.0).abs() < 0.01);

        // Check toolset names
        let names: Vec<&str> = dist.toolsets.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"full"));
        assert!(names.contains(&"read_only"));
        assert!(names.contains(&"no_terminal"));
        assert!(names.contains(&"minimal"));
    }

    #[test]
    fn test_toolset_allowed_tools_normalize_execute_terminal_alias() {
        let dist = ToolsetDistribution::hermes_style();
        let full = &dist.toolsets[0];

        let allowed_tools = full.allowed_tools();
        assert!(allowed_tools.contains(&"terminal".to_string()));
        assert!(!allowed_tools.contains(&"execute_terminal".to_string()));
    }

    #[test]
    fn test_empty_distribution_fails() {
        let dist = ToolsetDistribution::new(vec![]);
        assert!(dist.sample().is_err());
    }

    #[test]
    fn test_stats_is_balanced() {
        // Balanced distribution
        let balanced_samples = vec![
            Toolset {
                name: "a".to_string(),
                tools: vec![],
                weight: 1.0,
            },
            Toolset {
                name: "b".to_string(),
                tools: vec![],
                weight: 1.0,
            },
            Toolset {
                name: "c".to_string(),
                tools: vec![],
                weight: 1.0,
            },
        ];

        let dist = ToolsetDistribution::new(vec![]);
        let stats = dist.compute_stats(&balanced_samples);
        assert!(stats.is_balanced());

        // Unbalanced distribution (one toolset dominates)
        let mut unbalanced_samples = vec![];
        for _ in 0..80 {
            unbalanced_samples.push(Toolset {
                name: "dominant".to_string(),
                tools: vec![],
                weight: 1.0,
            });
        }
        for _ in 0..20 {
            unbalanced_samples.push(Toolset {
                name: "rare".to_string(),
                tools: vec![],
                weight: 1.0,
            });
        }

        let stats = dist.compute_stats(&unbalanced_samples);
        assert!(!stats.is_balanced());
    }
}
