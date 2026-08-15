//! Webhook runtime integration for agent triggering
//!
//! This module implements the runtime integration layer that connects webhook events
//! to agent execution, enabling webhooks to trigger autonomous agent workflows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Webhook event that triggers agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookAgentEvent {
    /// Webhook ID
    pub webhook_id: String,

    /// Event type
    pub event_type: String,

    /// Payload data
    pub payload: serde_json::Value,

    /// Timestamp
    pub timestamp: u64,

    /// Delivery ID
    pub delivery_id: String,
}

/// Agent trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTriggerConfig {
    /// Agent principal ID to trigger
    pub principal_id: String,

    /// Prompt template (can use {{event_type}}, {{payload}} placeholders)
    pub prompt_template: String,

    /// Whether to run in background
    pub background: bool,

    /// Timeout in seconds
    pub timeout_secs: u64,
}

/// Webhook agent trigger result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTriggerResult {
    /// Whether trigger succeeded
    pub success: bool,

    /// Agent response (if synchronous)
    pub response: Option<String>,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedAgentTrigger {
    pub principal_id: String,
    pub prompt: String,
    pub background: bool,
    pub timeout_secs: u64,
}

/// Webhook runtime manager
pub struct WebhookRuntimeManager {
    /// Webhook ID -> Agent trigger config
    triggers: Arc<RwLock<HashMap<String, AgentTriggerConfig>>>,
}

impl WebhookRuntimeManager {
    /// Create new webhook runtime manager
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register agent trigger for webhook
    pub async fn register_trigger(&self, webhook_id: String, config: AgentTriggerConfig) {
        let mut triggers = self.triggers.write().await;
        triggers.insert(webhook_id, config);
    }

    /// Unregister agent trigger
    pub async fn unregister_trigger(&self, webhook_id: &str) -> bool {
        let mut triggers = self.triggers.write().await;
        triggers.remove(webhook_id).is_some()
    }

    /// Get trigger config for webhook
    pub async fn get_trigger(&self, webhook_id: &str) -> Option<AgentTriggerConfig> {
        let triggers = self.triggers.read().await;
        triggers.get(webhook_id).cloned()
    }

    /// List all registered triggers
    pub async fn list_triggers(&self) -> Vec<(String, AgentTriggerConfig)> {
        let triggers = self.triggers.read().await;
        triggers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Process webhook event and trigger agent
    pub async fn process_event(
        &self,
        event: WebhookAgentEvent,
        agent_executor: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) -> AgentTriggerResult {
        let start = std::time::Instant::now();

        // Get trigger config
        let config = match self.get_trigger(&event.webhook_id).await {
            Some(c) => c,
            None => {
                return AgentTriggerResult {
                    success: false,
                    response: None,
                    error: Some(format!(
                        "No trigger registered for webhook '{}'",
                        event.webhook_id
                    )),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        // Render prompt from template
        let prompt = self.render_prompt(&config.prompt_template, &event);

        // Execute agent
        let response = match agent_executor(&prompt) {
            Ok(r) => r,
            Err(e) => {
                return AgentTriggerResult {
                    success: false,
                    response: None,
                    error: Some(format!("Agent execution failed: {}", e)),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        AgentTriggerResult {
            success: true,
            response: Some(response),
            error: None,
            execution_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    pub async fn prepare_event(
        &self,
        event: WebhookAgentEvent,
    ) -> Result<PreparedAgentTrigger, String> {
        let config = self
            .get_trigger(&event.webhook_id)
            .await
            .ok_or_else(|| format!("No trigger registered for webhook '{}'", event.webhook_id))?;
        let prompt = self.render_prompt(&config.prompt_template, &event);
        Ok(PreparedAgentTrigger {
            principal_id: config.principal_id,
            prompt,
            background: config.background,
            timeout_secs: config.timeout_secs,
        })
    }

    /// Render prompt template with event data
    fn render_prompt(&self, template: &str, event: &WebhookAgentEvent) -> String {
        template
            .replace("{{event_type}}", &event.event_type)
            .replace("{{payload}}", &event.payload.to_string())
            .replace("{{webhook_id}}", &event.webhook_id)
            .replace("{{delivery_id}}", &event.delivery_id)
    }
}

impl Default for WebhookRuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get_trigger() {
        let manager = WebhookRuntimeManager::new();

        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Process event: {{event_type}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_1".to_string(), config.clone())
            .await;

        let retrieved = manager.get_trigger("webhook_1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().principal_id, "test_principal");
    }

    #[tokio::test]
    async fn test_unregister_trigger() {
        let manager = WebhookRuntimeManager::new();

        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Test".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_1".to_string(), config)
            .await;
        assert!(manager.unregister_trigger("webhook_1").await);
        assert!(!manager.unregister_trigger("webhook_1").await);
    }

    #[tokio::test]
    async fn test_list_triggers() {
        let manager = WebhookRuntimeManager::new();

        let config1 = AgentTriggerConfig {
            principal_id: "principal_1".to_string(),
            prompt_template: "Test 1".to_string(),
            background: false,
            timeout_secs: 30,
        };

        let config2 = AgentTriggerConfig {
            principal_id: "principal_2".to_string(),
            prompt_template: "Test 2".to_string(),
            background: true,
            timeout_secs: 60,
        };

        manager
            .register_trigger("webhook_1".to_string(), config1)
            .await;
        manager
            .register_trigger("webhook_2".to_string(), config2)
            .await;

        let triggers = manager.list_triggers().await;
        assert_eq!(triggers.len(), 2);
    }

    #[tokio::test]
    async fn test_process_event_no_trigger() {
        let manager = WebhookRuntimeManager::new();

        let event = WebhookAgentEvent {
            webhook_id: "unknown_webhook".to_string(),
            event_type: "test_event".to_string(),
            payload: serde_json::json!({"key": "value"}),
            timestamp: 1234567890,
            delivery_id: "delivery_1".to_string(),
        };

        let result = manager
            .process_event(event, |_| Ok("test".to_string()))
            .await;
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_process_event_with_trigger() {
        let manager = WebhookRuntimeManager::new();

        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Process {{event_type}} from {{webhook_id}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_1".to_string(), config)
            .await;

        let event = WebhookAgentEvent {
            webhook_id: "webhook_1".to_string(),
            event_type: "push".to_string(),
            payload: serde_json::json!({"repo": "zaion-rust"}),
            timestamp: 1234567890,
            delivery_id: "delivery_1".to_string(),
        };

        let result = manager
            .process_event(event, |prompt| {
                assert!(prompt.contains("push"));
                assert!(prompt.contains("webhook_1"));
                Ok("Agent executed successfully".to_string())
            })
            .await;

        assert!(result.success);
        assert!(result.response.is_some());
        assert_eq!(result.response.unwrap(), "Agent executed successfully");
    }

    #[tokio::test]
    async fn test_prepare_event_returns_principal_prompt_and_budget() {
        let manager = WebhookRuntimeManager::new();
        manager
            .register_trigger(
                "webhook_1".to_string(),
                AgentTriggerConfig {
                    principal_id: "principal_1".to_string(),
                    prompt_template: "Review {{event_type}} {{payload}}".to_string(),
                    background: true,
                    timeout_secs: 12,
                },
            )
            .await;

        let prepared = manager
            .prepare_event(WebhookAgentEvent {
                webhook_id: "webhook_1".to_string(),
                event_type: "paper.found".to_string(),
                payload: serde_json::json!({"title": "context"}),
                timestamp: 123,
                delivery_id: "delivery_1".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(prepared.principal_id, "principal_1");
        assert!(prepared.prompt.contains("paper.found"));
        assert!(prepared.prompt.contains("context"));
        assert!(prepared.background);
        assert_eq!(prepared.timeout_secs, 12);
    }

    #[test]
    fn test_render_prompt() {
        let manager = WebhookRuntimeManager::new();

        let event = WebhookAgentEvent {
            webhook_id: "webhook_1".to_string(),
            event_type: "push".to_string(),
            payload: serde_json::json!({"repo": "test"}),
            timestamp: 1234567890,
            delivery_id: "delivery_1".to_string(),
        };

        let template = "Event: {{event_type}}, Webhook: {{webhook_id}}, Delivery: {{delivery_id}}";
        let rendered = manager.render_prompt(template, &event);

        assert!(rendered.contains("Event: push"));
        assert!(rendered.contains("Webhook: webhook_1"));
        assert!(rendered.contains("Delivery: delivery_1"));
    }
}
