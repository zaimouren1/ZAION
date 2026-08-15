//! VLLM Client for token-level logprobs extraction
//!
//! This module implements the VLLM API client that:
//! 1. Calls VLLM server with prompt_logprobs enabled
//! 2. Extracts per-token logprobs from response
//! 3. Supports both student and teacher model inference

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// VLLM API request
#[derive(Debug, Clone, Serialize)]
pub struct VllmRequest {
    pub model: String,
    pub messages: Vec<VllmMessage>,
    pub max_tokens: usize,
    pub temperature: f32,
    pub logprobs: bool,
    pub top_logprobs: usize,
}

/// VLLM message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VllmMessage {
    pub role: String,
    pub content: String,
}

/// VLLM API response
#[derive(Debug, Clone, Deserialize)]
pub struct VllmResponse {
    pub id: String,
    pub choices: Vec<VllmChoice>,
    pub usage: VllmUsage,
}

/// VLLM choice
#[derive(Debug, Clone, Deserialize)]
pub struct VllmChoice {
    pub index: usize,
    pub message: VllmMessage,
    pub logprobs: Option<VllmLogprobs>,
    pub finish_reason: String,
}

/// VLLM logprobs structure
#[derive(Debug, Clone, Deserialize)]
pub struct VllmLogprobs {
    pub content: Vec<VllmTokenLogprob>,
}

/// Per-token logprob
#[derive(Debug, Clone, Deserialize)]
pub struct VllmTokenLogprob {
    pub token: String,
    pub logprob: f32,
    pub top_logprobs: Vec<VllmTopLogprob>,
}

/// Top-K logprobs for a token position
#[derive(Debug, Clone, Deserialize)]
pub struct VllmTopLogprob {
    pub token: String,
    pub logprob: f32,
}

/// VLLM usage stats
#[derive(Debug, Clone, Deserialize)]
pub struct VllmUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// VLLM Client
pub struct VllmClient {
    base_url: String,
    http_client: reqwest::Client,
}

impl VllmClient {
    /// Create a new VLLM client
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http_client: reqwest::Client::new(),
        }
    }

    /// Call VLLM API with logprobs enabled
    pub async fn complete_with_logprobs(&self, request: VllmRequest) -> Result<VllmResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send VLLM request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("VLLM API error {}: {}", status, body);
        }

        let vllm_response: VllmResponse = response
            .json()
            .await
            .context("Failed to parse VLLM response")?;

        Ok(vllm_response)
    }

    /// Extract tokens and logprobs from response
    pub fn extract_logprobs(&self, response: &VllmResponse) -> Result<(Vec<String>, Vec<f32>)> {
        let choice = response
            .choices
            .first()
            .context("No choices in VLLM response")?;

        let logprobs = choice
            .logprobs
            .as_ref()
            .context("No logprobs in VLLM response")?;

        let tokens: Vec<String> = logprobs.content.iter().map(|t| t.token.clone()).collect();

        let logprob_values: Vec<f32> = logprobs.content.iter().map(|t| t.logprob).collect();

        Ok((tokens, logprob_values))
    }

    /// Get response text
    pub fn get_response_text(&self, response: &VllmResponse) -> Result<String> {
        let choice = response
            .choices
            .first()
            .context("No choices in VLLM response")?;

        Ok(choice.message.content.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vllm_client_creation() {
        let client = VllmClient::new("http://localhost:8000/v1".to_string());
        assert_eq!(client.base_url, "http://localhost:8000/v1");
    }

    #[test]
    fn test_extract_logprobs() {
        let client = VllmClient::new("http://localhost:8000/v1".to_string());

        let response = VllmResponse {
            id: "test".to_string(),
            choices: vec![VllmChoice {
                index: 0,
                message: VllmMessage {
                    role: "assistant".to_string(),
                    content: "Hello world".to_string(),
                },
                logprobs: Some(VllmLogprobs {
                    content: vec![
                        VllmTokenLogprob {
                            token: "Hello".to_string(),
                            logprob: -1.0,
                            top_logprobs: vec![],
                        },
                        VllmTokenLogprob {
                            token: " world".to_string(),
                            logprob: -0.5,
                            top_logprobs: vec![],
                        },
                    ],
                }),
                finish_reason: "stop".to_string(),
            }],
            usage: VllmUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
            },
        };

        let (tokens, logprobs) = client.extract_logprobs(&response).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], "Hello");
        assert_eq!(logprobs[0], -1.0);
    }

    #[test]
    fn test_get_response_text() {
        let client = VllmClient::new("http://localhost:8000/v1".to_string());

        let response = VllmResponse {
            id: "test".to_string(),
            choices: vec![VllmChoice {
                index: 0,
                message: VllmMessage {
                    role: "assistant".to_string(),
                    content: "Test response".to_string(),
                },
                logprobs: None,
                finish_reason: "stop".to_string(),
            }],
            usage: VllmUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
            },
        };

        let text = client.get_response_text(&response).unwrap();
        assert_eq!(text, "Test response");
    }
}
