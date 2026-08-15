//! OPD Environment - Core agent loop with token-level training signals
//!
//! This module implements the core OPD environment that:
//! 1. Runs agent tool-calling loops
//! 2. Extracts training signals from tool results
//! 3. Computes token-level advantages using teacher model
//! 4. Packages trajectories with dense training signals

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::advantages::TokenAdvantages;
use crate::tool_executor::ToolExecutor;
use crate::toolset_distribution::Toolset;
use crate::trajectory::{ToolCall, ToolResult, Trajectory, TrajectoryMessage};
use crate::vllm_client::{VllmClient, VllmMessage, VllmRequest};

/// Configuration for OPD environment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdConfig {
    /// Student model endpoint (VLLM server)
    pub student_model_url: String,

    /// Student model name
    pub student_model_name: String,

    /// Teacher model endpoint (for computing advantages)
    pub teacher_model_url: String,

    /// Teacher model name
    pub teacher_model_name: String,

    /// Maximum turns per trajectory
    pub max_turns: usize,

    /// Maximum tokens per turn
    pub max_tokens: usize,

    /// Temperature for sampling
    pub temperature: f32,

    /// Top-K for teacher logprobs
    pub teacher_top_k: usize,

    /// Enable prompt logprobs (requires VLLM)
    pub enable_prompt_logprobs: bool,
}

impl Default for OpdConfig {
    fn default() -> Self {
        Self {
            student_model_url: "http://localhost:8000/v1".to_string(),
            student_model_name: "Qwen/Qwen3-4B".to_string(),
            teacher_model_url: "http://localhost:8001/v1".to_string(),
            teacher_model_name: "Qwen/Qwen3-7B".to_string(),
            max_turns: 20,
            max_tokens: 2048,
            temperature: 0.7,
            teacher_top_k: 10,
            enable_prompt_logprobs: true,
        }
    }
}

/// Result from OPD environment execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdResult {
    /// The complete trajectory
    pub trajectory: Trajectory,

    /// Token-level advantages for each assistant turn
    pub advantages: Vec<TokenAdvantages>,

    /// Total training signal quality score (0.0-1.0)
    pub signal_quality: f32,
}

/// OPD Environment
pub struct OpdEnv {
    config: OpdConfig,
    student_client: VllmClient,
    teacher_client: VllmClient,
}

impl OpdEnv {
    /// Create a new OPD environment
    pub fn new(config: OpdConfig) -> Self {
        let student_client = VllmClient::new(config.student_model_url.clone());
        let teacher_client = VllmClient::new(config.teacher_model_url.clone());
        Self {
            config,
            student_client,
            teacher_client,
        }
    }

    /// Run a single trajectory with OPD training signals
    pub async fn run_trajectory(&self, task: String) -> Result<OpdResult> {
        self.run_trajectory_with_toolset(task, None).await
    }

    /// Run a single trajectory with an optional per-trajectory toolset restriction.
    pub async fn run_trajectory_with_toolset(
        &self,
        task: String,
        toolset: Option<&Toolset>,
    ) -> Result<OpdResult> {
        let trajectory_id = uuid::Uuid::new_v4().to_string();
        info!("Starting OPD trajectory: {}", trajectory_id);

        let mut trajectory = Trajectory::new(trajectory_id.clone(), task.clone());
        let tool_executor = self.tool_executor_for_trajectory(toolset);

        // Add initial user message
        trajectory.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: task.clone(),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut advantages_list = Vec::new();

        // Main agent loop
        for turn in 0..self.config.max_turns {
            debug!("Turn {}/{}", turn + 1, self.config.max_turns);

            // Get student response
            let assistant_response = self.get_student_response(&trajectory).await?;

            // Extract tool calls if any
            let tool_calls = Self::extract_tool_calls(&tool_executor, &assistant_response)?;

            // Add assistant message
            trajectory.add_message(TrajectoryMessage {
                role: "assistant".to_string(),
                content: assistant_response.clone(),
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls.clone())
                },
                tool_call_id: None,
            });

            // If no tool calls, trajectory is complete
            if tool_calls.is_empty() {
                trajectory.success = true;
                break;
            }

            // Execute tool calls and collect results
            let mut tool_results = Vec::new();
            for tool_call in &tool_calls {
                let result = Self::execute_tool(&tool_executor, tool_call).await?;
                tool_results.push(result.clone());

                // Add tool result message
                trajectory.add_message(TrajectoryMessage {
                    role: "tool".to_string(),
                    content: result.content.clone(),
                    tool_calls: None,
                    tool_call_id: Some(result.tool_call_id.clone()),
                });

                // Update tool stats
                trajectory.update_tool_stats(tool_call.name.clone(), result.success);
            }

            // Compute token-level advantages using teacher model
            if self.config.enable_prompt_logprobs {
                let advantages = self
                    .compute_token_advantages(&trajectory, &assistant_response, &tool_results)
                    .await?;
                advantages_list.push(advantages);
            }
        }

        // Compute overall signal quality
        let signal_quality = self.compute_signal_quality(&advantages_list);

        Ok(OpdResult {
            trajectory,
            advantages: advantages_list,
            signal_quality,
        })
    }

    /// Get response from student model
    async fn get_student_response(&self, trajectory: &Trajectory) -> Result<String> {
        // Convert trajectory messages to VLLM format
        let messages: Vec<VllmMessage> = trajectory
            .messages
            .iter()
            .map(|m| VllmMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = VllmRequest {
            model: self.config.student_model_name.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            logprobs: self.config.enable_prompt_logprobs,
            top_logprobs: self.config.teacher_top_k,
        };

        let response = self.student_client.complete_with_logprobs(request).await?;
        self.student_client.get_response_text(&response)
    }

    /// Extract tool calls from assistant response
    fn tool_executor_for_trajectory(&self, toolset: Option<&Toolset>) -> ToolExecutor {
        let task_id = uuid::Uuid::new_v4().to_string();
        let executor = ToolExecutor::new(task_id);
        match toolset {
            Some(toolset) => executor.with_toolset(toolset),
            None => executor,
        }
    }

    fn extract_tool_calls(tool_executor: &ToolExecutor, response: &str) -> Result<Vec<ToolCall>> {
        tool_executor.parse_tool_calls(response)
    }

    /// Execute a tool call
    async fn execute_tool(
        tool_executor: &ToolExecutor,
        tool_call: &ToolCall,
    ) -> Result<ToolResult> {
        // Execute synchronously (tool executor is sync)
        tool_executor.execute(tool_call)
    }

    /// Compute token-level advantages using teacher model
    async fn compute_token_advantages(
        &self,
        trajectory: &Trajectory,
        assistant_response: &str,
        _tool_results: &[ToolResult],
    ) -> Result<TokenAdvantages> {
        // Build messages including the assistant response
        let mut messages: Vec<VllmMessage> = trajectory
            .messages
            .iter()
            .map(|m| VllmMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        // Add the assistant response we want to score
        messages.push(VllmMessage {
            role: "assistant".to_string(),
            content: assistant_response.to_string(),
        });

        // Get teacher model logprobs for the same response
        let teacher_request = VllmRequest {
            model: self.config.teacher_model_name.clone(),
            messages: messages.clone(),
            max_tokens: self.config.max_tokens,
            temperature: 0.0, // Use greedy for teacher scoring
            logprobs: true,
            top_logprobs: self.config.teacher_top_k,
        };

        let teacher_response = self
            .teacher_client
            .complete_with_logprobs(teacher_request)
            .await?;
        let (teacher_tokens, teacher_logprobs) =
            self.teacher_client.extract_logprobs(&teacher_response)?;

        let student_request = VllmRequest {
            model: self.config.student_model_name.clone(),
            messages,
            max_tokens: self.config.max_tokens,
            temperature: 0.0,
            logprobs: true,
            top_logprobs: self.config.teacher_top_k,
        };

        let student_response = self
            .student_client
            .complete_with_logprobs(student_request)
            .await?;
        let (student_tokens, student_logprobs) =
            self.student_client.extract_logprobs(&student_response)?;

        if teacher_tokens != student_tokens {
            bail!(
                "teacher/student token mismatch while computing OPD advantages: teacher={:?}, student={:?}",
                teacher_tokens,
                student_tokens
            );
        }

        Ok(TokenAdvantages::new(
            teacher_tokens,
            teacher_logprobs,
            student_logprobs,
        ))
    }

    /// Compute overall signal quality score
    fn compute_signal_quality(&self, advantages_list: &[TokenAdvantages]) -> f32 {
        if advantages_list.is_empty() {
            return 0.0;
        }

        let total_advantages: f32 = advantages_list.iter().flat_map(|adv| &adv.advantages).sum();

        let total_tokens: usize = advantages_list.iter().map(|adv| adv.tokens.len()).sum();

        if total_tokens == 0 {
            0.0
        } else {
            (total_advantages / total_tokens as f32).abs().min(1.0)
        }
    }
}

// Add uuid dependency
use uuid;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Json, response::IntoResponse, routing::post, Router};
    use serde_json::json;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_opd_env_creation() {
        let config = OpdConfig::default();
        let _env = OpdEnv::new(config);
        // Compile-smoke test — ensures items in scope above type-check.
    }

    #[tokio::test]
    async fn test_signal_quality_empty() {
        let config = OpdConfig::default();
        let env = OpdEnv::new(config);
        let quality = env.compute_signal_quality(&[]);
        assert_eq!(quality, 0.0);
    }

    #[tokio::test]
    async fn test_signal_quality_with_advantages() {
        let config = OpdConfig::default();
        let env = OpdEnv::new(config);

        let advantages = vec![TokenAdvantages {
            tokens: vec!["test".to_string()],
            advantages: vec![0.5],
            teacher_logprobs: vec![-1.0],
            student_logprobs: vec![-1.5],
        }];

        let quality = env.compute_signal_quality(&advantages);
        assert!(quality > 0.0 && quality <= 1.0);
    }

    #[tokio::test]
    async fn compute_token_advantages_uses_student_vllm_logprobs() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/chat/completions",
            post(|Json(request): Json<serde_json::Value>| async move {
                let model = request["model"].as_str().unwrap_or_default();
                let (id, logprobs) = if model.contains("teacher") {
                    (
                        "teacher-logprobs",
                        vec![("def", -0.10_f32), (" fizzbuzz", -0.20_f32)],
                    )
                } else {
                    (
                        "student-logprobs",
                        vec![("def", -0.90_f32), (" fizzbuzz", -1.10_f32)],
                    )
                };
                Json(json!({
                    "id": id,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "def fizzbuzz"
                        },
                        "logprobs": {
                            "content": logprobs
                                .into_iter()
                                .map(|(token, logprob)| json!({
                                    "token": token,
                                    "logprob": logprob,
                                    "top_logprobs": [{
                                        "token": token,
                                        "logprob": logprob
                                    }]
                                }))
                                .collect::<Vec<_>>()
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 8,
                        "completion_tokens": 2,
                        "total_tokens": 10
                    }
                }))
                .into_response()
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let config = OpdConfig {
            student_model_url: format!("http://{}", addr),
            teacher_model_url: format!("http://{}", addr),
            student_model_name: "student-model".to_string(),
            teacher_model_name: "teacher-model".to_string(),
            ..Default::default()
        };
        let env = OpdEnv::new(config);
        let trajectory = Trajectory::new("traj-1".to_string(), "Write fizzbuzz".to_string());

        let advantages = env
            .compute_token_advantages(&trajectory, "def fizzbuzz", &[])
            .await
            .unwrap();

        assert_eq!(advantages.tokens, vec!["def", " fizzbuzz"]);
        assert_eq!(advantages.teacher_logprobs, vec![-0.10, -0.20]);
        assert_eq!(advantages.student_logprobs, vec![-0.90, -1.10]);
        assert_eq!(advantages.advantages, vec![0.79999995, 0.90000004]);
    }
}
