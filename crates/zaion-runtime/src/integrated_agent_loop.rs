//! Integrated agent loop combining webhook, memory, and OPD capabilities
//!
//! This module provides a unified agent execution loop that integrates:
//! - Webhook event triggering
//! - Memory-augmented context
//! - OPD training signal collection
//! - Real agent execution

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::turn_proof::TurnRuntimeMemoryEvidence;
use crate::webhook_runtime::{AgentTriggerResult, WebhookAgentEvent, WebhookRuntimeManager};
use zaion_memory::runtime_integration::{MemoryManager, MemoryRuntimeConfig};

/// Function that executes an agent turn given a prompt string and returns the
/// assistant response or an error message.
pub type AgentExecutor = Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegratedAgentExecutionReport {
    pub response: String,
    pub memory_context_size: usize,
    pub runtime_memory_evidence: Option<TurnRuntimeMemoryEvidence>,
    pub memory_tool_schemas_loaded: usize,
}

/// Integrated agent loop configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegratedAgentConfig {
    /// Enable memory integration
    pub enable_memory: bool,

    /// Enable OPD training signal collection
    pub enable_opd: bool,

    /// Enable webhook triggering
    pub enable_webhooks: bool,

    /// Memory runtime config
    pub memory_config: MemoryRuntimeConfig,
}

impl Default for IntegratedAgentConfig {
    fn default() -> Self {
        Self {
            enable_memory: true,
            enable_opd: false,
            enable_webhooks: true,
            memory_config: MemoryRuntimeConfig::default(),
        }
    }
}

/// Integrated agent loop
pub struct IntegratedAgentLoop {
    /// Configuration
    config: IntegratedAgentConfig,

    /// Webhook runtime manager
    webhook_manager: Arc<WebhookRuntimeManager>,

    /// Memory manager
    memory_manager: Arc<MemoryManager>,

    /// Current session ID
    session_id: String,
}

impl IntegratedAgentLoop {
    /// Create new integrated agent loop
    pub fn new(
        config: IntegratedAgentConfig,
        webhook_manager: Arc<WebhookRuntimeManager>,
        memory_manager: Arc<MemoryManager>,
        session_id: String,
    ) -> Self {
        Self {
            config,
            webhook_manager,
            memory_manager,
            session_id,
        }
    }

    /// Execute agent with full integration
    pub async fn execute(
        &self,
        user_message: &str,
        agent_executor: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) -> Result<String, String> {
        Ok(self
            .execute_with_report(user_message, agent_executor)
            .await?
            .response)
    }

    pub async fn execute_with_report(
        &self,
        user_message: &str,
        agent_executor: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) -> Result<IntegratedAgentExecutionReport, String> {
        // Step 1: Prefetch memory context if enabled
        let memory_context = if self.config.enable_memory && self.config.memory_config.auto_prefetch
        {
            self.memory_manager
                .prefetch_all(user_message, &self.session_id)
                .await
        } else {
            String::new()
        };

        // Step 2: Build augmented prompt
        let augmented_prompt = if !memory_context.is_empty() {
            format!(
                "# Relevant Memories\n\n{}\n\n# User Message\n\n{}",
                memory_context, user_message
            )
        } else {
            user_message.to_string()
        };
        let memory_context_size = memory_context.len();
        let runtime_memory_evidence =
            TurnRuntimeMemoryEvidence::from_context(self.config.enable_memory, &memory_context);
        let memory_tool_schemas_loaded = if self.config.enable_memory {
            self.memory_manager.get_all_tool_schemas().await.len()
        } else {
            0
        };

        // Step 3: Execute agent
        let assistant_response = agent_executor(&augmented_prompt)?;

        // Step 4: Sync to memory if enabled
        if self.config.enable_memory && self.config.memory_config.auto_sync {
            self.memory_manager
                .sync_all(user_message, &assistant_response, &self.session_id)
                .await;
            self.memory_manager
                .queue_prefetch_all(user_message, &self.session_id)
                .await;
        }

        // Step 5: Collect OPD training signals if enabled
        if self.config.enable_opd {
            // TODO: Integrate with zaion-opd for training signal collection
        }

        Ok(IntegratedAgentExecutionReport {
            response: assistant_response,
            memory_context_size,
            runtime_memory_evidence,
            memory_tool_schemas_loaded,
        })
    }

    /// Process webhook event with full integration
    pub async fn process_webhook_event(
        &self,
        event: WebhookAgentEvent,
        agent_executor: AgentExecutor,
    ) -> AgentTriggerResult {
        if !self.config.enable_webhooks {
            return AgentTriggerResult {
                success: false,
                response: None,
                error: Some("Webhooks disabled".to_string()),
                execution_time_ms: 0,
            };
        }

        self.webhook_manager
            .process_event(event, agent_executor)
            .await
    }

    /// Get memory context
    pub async fn get_memory_context(&self, query: &str) -> Result<String, String> {
        if !self.config.enable_memory {
            return Ok(String::new());
        }

        Ok(self
            .memory_manager
            .prefetch_all(query, &self.session_id)
            .await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_integrated_agent_loop_creation() {
        let config = IntegratedAgentConfig::default();
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let loop_instance = IntegratedAgentLoop::new(
            config,
            webhook_manager,
            memory_manager,
            "test_session".to_string(),
        );

        assert_eq!(loop_instance.session_id, "test_session");
    }

    #[tokio::test]
    async fn test_execute_without_memory() {
        let config = IntegratedAgentConfig {
            enable_memory: false,
            ..IntegratedAgentConfig::default()
        };

        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let loop_instance = IntegratedAgentLoop::new(
            config,
            webhook_manager,
            memory_manager,
            "test_session".to_string(),
        );

        let result = loop_instance
            .execute("Hello", |prompt| {
                assert_eq!(prompt, "Hello");
                Ok("Hi there!".to_string())
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hi there!");
    }

    #[tokio::test]
    async fn test_get_memory_context_disabled() {
        let config = IntegratedAgentConfig {
            enable_memory: false,
            ..IntegratedAgentConfig::default()
        };

        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        let loop_instance = IntegratedAgentLoop::new(
            config,
            webhook_manager,
            memory_manager,
            "test_session".to_string(),
        );

        let context = loop_instance.get_memory_context("test").await;
        assert!(context.is_ok());
        assert_eq!(context.unwrap(), "");
    }

    struct StaticMemoryProvider;

    impl zaion_memory::runtime_integration::MemoryProvider for StaticMemoryProvider {
        fn name(&self) -> &str {
            "static-test"
        }

        fn system_prompt_block(&self) -> String {
            String::new()
        }

        fn prefetch(&self, _query: &str, _session_id: &str) -> anyhow::Result<String> {
            Ok("<memory-context>remembered fact</memory-context>".to_string())
        }

        fn sync_turn(
            &self,
            _user_content: &str,
            _assistant_content: &str,
            _session_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn get_tool_schemas(&self) -> Vec<serde_json::Value> {
            vec![serde_json::json!({
                "name": "memory_static_lookup",
                "description": "static test memory lookup",
                "parameters": { "type": "object" }
            })]
        }

        fn handle_tool_call(
            &self,
            _tool_name: &str,
            _args: &serde_json::Value,
        ) -> anyhow::Result<String> {
            Ok("{}".to_string())
        }
    }

    #[tokio::test]
    async fn execute_with_report_returns_real_memory_and_tool_counts() {
        let config = IntegratedAgentConfig::default();
        let webhook_manager = Arc::new(WebhookRuntimeManager::new());
        let memory_manager = Arc::new(MemoryManager::new());
        memory_manager
            .add_provider(Box::new(StaticMemoryProvider))
            .await;
        let loop_instance = IntegratedAgentLoop::new(
            config,
            webhook_manager,
            memory_manager,
            "test_session".to_string(),
        );

        let report = loop_instance
            .execute_with_report("Hello", |prompt| {
                assert!(prompt.contains("# Relevant Memories"));
                assert!(prompt.contains("remembered fact"));
                Ok("Hi there!".to_string())
            })
            .await
            .unwrap();

        assert_eq!(report.response, "Hi there!");
        assert!(report.memory_context_size > 0);
        let evidence = report
            .runtime_memory_evidence
            .expect("runtime memory evidence");
        assert_eq!(evidence.schema, "zaion.runtime_memory_evidence.v1");
        assert!(evidence.fenced_context);
        assert_eq!(evidence.memory_context_bytes, report.memory_context_size);
        assert_eq!(evidence.memory_context_hash.len(), 64);
        assert_eq!(evidence.evidence_hash.len(), 64);
        assert_eq!(report.memory_tool_schemas_loaded, 1);
    }
}
