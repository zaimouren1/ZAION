//! End-to-end webhook → agent execution integration tests
//!
//! This module provides comprehensive E2E tests for the webhook runtime,
//! verifying the complete flow from webhook event reception to agent execution
//! and response generation.

#[cfg(test)]
mod tests {
    use crate::webhook_runtime::{AgentTriggerConfig, WebhookAgentEvent, WebhookRuntimeManager};

    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock agent executor that records invocations
    #[derive(Clone)]
    struct MockAgentExecutor {
        invocations: Arc<Mutex<Vec<String>>>,
    }

    impl MockAgentExecutor {
        fn new() -> Self {
            Self {
                invocations: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn execute(&self, prompt: &str) -> Result<String, String> {
            let mut invocations = self.invocations.lock().await;
            invocations.push(prompt.to_string());
            Ok(format!("Agent response to: {}", prompt))
        }

        async fn get_invocations(&self) -> Vec<String> {
            self.invocations.lock().await.clone()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_webhook_to_agent_e2e_basic() {
        let manager = WebhookRuntimeManager::new();
        let executor = MockAgentExecutor::new();

        // Register webhook trigger
        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Process webhook event: {{event_type}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_test".to_string(), config)
            .await;

        // Create webhook event
        let event = WebhookAgentEvent {
            webhook_id: "webhook_test".to_string(),
            event_type: "github.push".to_string(),
            payload: serde_json::json!({"repo": "zaion-rust", "branch": "main"}),
            timestamp: 1713312000,
            delivery_id: "delivery_123".to_string(),
        };

        // Execute webhook → agent flow
        let executor_clone = executor.clone();
        let result = manager
            .process_event(event, move |prompt| {
                let executor = executor_clone.clone();
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(executor.execute(prompt))
                })
            })
            .await;

        // Verify execution
        assert!(result.success);
        assert!(result.response.is_some());
        assert!(result.error.is_none());

        // Verify agent was invoked with correct prompt
        let invocations = executor.get_invocations().await;
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0], "Process webhook event: github.push");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_webhook_to_agent_e2e_template_rendering() {
        let manager = WebhookRuntimeManager::new();
        let executor = MockAgentExecutor::new();

        // Register webhook trigger with complex template
        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Event: {{event_type}}, Webhook: {{webhook_id}}, Delivery: {{delivery_id}}, Payload: {{payload}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_complex".to_string(), config)
            .await;

        // Create webhook event
        let event = WebhookAgentEvent {
            webhook_id: "webhook_complex".to_string(),
            event_type: "deployment.success".to_string(),
            payload: serde_json::json!({"env": "production", "version": "v1.2.3"}),
            timestamp: 1713312000,
            delivery_id: "delivery_456".to_string(),
        };

        // Execute webhook → agent flow
        let executor_clone = executor.clone();
        let result = manager
            .process_event(event, move |prompt| {
                let executor = executor_clone.clone();
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(executor.execute(prompt))
                })
            })
            .await;

        // Verify execution
        assert!(result.success);

        // Verify template was rendered correctly
        let invocations = executor.get_invocations().await;
        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].contains("Event: deployment.success"));
        assert!(invocations[0].contains("Webhook: webhook_complex"));
        assert!(invocations[0].contains("Delivery: delivery_456"));
        assert!(invocations[0].contains("production"));
    }

    #[tokio::test]
    async fn test_webhook_to_agent_e2e_agent_failure() {
        let manager = WebhookRuntimeManager::new();

        // Register webhook trigger
        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Process: {{event_type}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_fail".to_string(), config)
            .await;

        // Create webhook event
        let event = WebhookAgentEvent {
            webhook_id: "webhook_fail".to_string(),
            event_type: "test.event".to_string(),
            payload: serde_json::json!({}),
            timestamp: 1713312000,
            delivery_id: "delivery_789".to_string(),
        };

        // Execute webhook → agent flow with failing executor
        let result = manager
            .process_event(event, |_prompt| {
                Err("Agent execution failed: timeout".to_string())
            })
            .await;

        // Verify failure handling
        assert!(!result.success);
        assert!(result.response.is_none());
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("Agent execution failed"));
    }

    #[tokio::test]
    async fn test_webhook_to_agent_e2e_no_trigger_registered() {
        let manager = WebhookRuntimeManager::new();

        // Create webhook event for unregistered webhook
        let event = WebhookAgentEvent {
            webhook_id: "unknown_webhook".to_string(),
            event_type: "test.event".to_string(),
            payload: serde_json::json!({}),
            timestamp: 1713312000,
            delivery_id: "delivery_999".to_string(),
        };

        // Execute webhook → agent flow
        let result = manager
            .process_event(event, |_prompt| Ok("Should not be called".to_string()))
            .await;

        // Verify no trigger error
        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("No trigger registered"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_webhook_to_agent_e2e_multiple_events() {
        let manager = WebhookRuntimeManager::new();
        let executor = MockAgentExecutor::new();

        // Register webhook trigger
        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Event {{event_type}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_multi".to_string(), config)
            .await;

        // Process multiple events
        for i in 1..=3 {
            let event = WebhookAgentEvent {
                webhook_id: "webhook_multi".to_string(),
                event_type: format!("event_{}", i),
                payload: serde_json::json!({"index": i}),
                timestamp: 1713312000 + i,
                delivery_id: format!("delivery_{}", i),
            };

            let executor_clone = executor.clone();
            let result = manager
                .process_event(event, move |prompt| {
                    let executor = executor_clone.clone();
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(executor.execute(prompt))
                    })
                })
                .await;

            assert!(result.success);
        }

        // Verify all events were processed
        let invocations = executor.get_invocations().await;
        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[0], "Event event_1");
        assert_eq!(invocations[1], "Event event_2");
        assert_eq!(invocations[2], "Event event_3");
    }

    #[tokio::test]
    async fn test_webhook_to_agent_e2e_execution_time_tracking() {
        let manager = WebhookRuntimeManager::new();

        // Register webhook trigger
        let config = AgentTriggerConfig {
            principal_id: "test_principal".to_string(),
            prompt_template: "Process: {{event_type}}".to_string(),
            background: false,
            timeout_secs: 30,
        };

        manager
            .register_trigger("webhook_timing".to_string(), config)
            .await;

        // Create webhook event
        let event = WebhookAgentEvent {
            webhook_id: "webhook_timing".to_string(),
            event_type: "test.event".to_string(),
            payload: serde_json::json!({}),
            timestamp: 1713312000,
            delivery_id: "delivery_timing".to_string(),
        };

        // Execute webhook → agent flow with simulated delay
        let result = manager
            .process_event(event, |_prompt| {
                std::thread::sleep(std::time::Duration::from_millis(50));
                Ok("Response".to_string())
            })
            .await;

        // Verify execution time was tracked
        assert!(result.success);
        assert!(result.execution_time_ms >= 50);
        assert!(result.execution_time_ms < 1000); // Should complete quickly
    }
}
