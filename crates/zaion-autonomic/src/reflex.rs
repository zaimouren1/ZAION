//! Autonomic Reflex System
//!
//! Defines reflexive responses to environmental stimuli without LLM invocation.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflexTrigger {
    /// Trigger type (e.g., "file_change", "memory_pressure", "idle_timeout")
    pub trigger_type: String,
    /// Optional pattern matching (e.g., regex for file paths)
    pub pattern: Option<String>,
    /// Threshold value for numeric triggers
    pub threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflexAction {
    /// Action type (e.g., "log_event", "compact_memory", "spawn_task")
    pub action_type: String,
    /// Action parameters as JSON
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomicReflex {
    pub id: String,
    pub name: String,
    pub trigger: ReflexTrigger,
    pub action: ReflexAction,
    pub enabled: bool,
}

pub struct ReflexRegistry {
    reflexes: HashMap<String, AutonomicReflex>,
}

impl ReflexRegistry {
    pub fn new() -> Self {
        Self {
            reflexes: HashMap::new(),
        }
    }

    pub fn register(&mut self, reflex: AutonomicReflex) {
        self.reflexes.insert(reflex.id.clone(), reflex);
    }

    pub fn get(&self, id: &str) -> Option<&AutonomicReflex> {
        self.reflexes.get(id)
    }

    pub fn count(&self) -> usize {
        self.reflexes.len()
    }

    pub fn list_enabled(&self) -> Vec<&AutonomicReflex> {
        self.reflexes.values().filter(|r| r.enabled).collect()
    }

    /// Check if any reflex should fire for a given trigger type
    pub fn match_trigger(&self, trigger_type: &str, value: Option<f64>) -> Vec<&AutonomicReflex> {
        self.reflexes
            .values()
            .filter(|r| {
                r.enabled
                    && r.trigger.trigger_type == trigger_type
                    && match (r.trigger.threshold, value) {
                        (Some(threshold), Some(v)) => v >= threshold,
                        (None, _) => true,
                        _ => false,
                    }
            })
            .collect()
    }
}

impl Default for ReflexRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_starts_empty() {
        let registry = ReflexRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn can_register_reflex() {
        let mut registry = ReflexRegistry::new();
        let reflex = AutonomicReflex {
            id: "test_reflex".to_string(),
            name: "Test Reflex".to_string(),
            trigger: ReflexTrigger {
                trigger_type: "test".to_string(),
                pattern: None,
                threshold: None,
            },
            action: ReflexAction {
                action_type: "log".to_string(),
                parameters: serde_json::json!({}),
            },
            enabled: true,
        };

        registry.register(reflex);
        assert_eq!(registry.count(), 1);
        assert!(registry.get("test_reflex").is_some());
    }

    #[test]
    fn match_trigger_respects_threshold() {
        let mut registry = ReflexRegistry::new();
        let reflex = AutonomicReflex {
            id: "memory_pressure".to_string(),
            name: "Memory Pressure Response".to_string(),
            trigger: ReflexTrigger {
                trigger_type: "memory_usage".to_string(),
                pattern: None,
                threshold: Some(0.8),
            },
            action: ReflexAction {
                action_type: "compact".to_string(),
                parameters: serde_json::json!({}),
            },
            enabled: true,
        };

        registry.register(reflex);

        // Below threshold - no match
        let matches = registry.match_trigger("memory_usage", Some(0.7));
        assert_eq!(matches.len(), 0);

        // Above threshold - match
        let matches = registry.match_trigger("memory_usage", Some(0.85));
        assert_eq!(matches.len(), 1);
    }
}
