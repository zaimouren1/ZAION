//! Groq provider using the OpenAI-compatible API at `https://api.groq.com/openai/v1`.
//!
//! Groq serves open-source models (Llama, Mixtral, Gemma) via an
//! OpenAI-compatible chat completions endpoint, so this provider
//! delegates to `OpenAiProvider`.

use super::{CompletionRequest, CompletionResponse, LlmProvider, OpenAiProvider, ProviderType};
use crate::AdapterError;

/// Known Groq-hosted models.
pub const GROQ_MODELS: &[&str] = &[
    "llama-3.3-70b-versatile",
    "mixtral-8x7b-32768",
    "gemma2-9b-it",
];

pub struct GroqProvider {
    pub api_key: String,
    pub base_url: String,
    /// Default model name, e.g. `"llama-3.3-70b-versatile"`.
    pub default_model: String,
}

impl GroqProvider {
    /// Create a Groq provider with the given API key and model.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.groq.com/openai/v1".to_string(),
            default_model: model.into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Validate that a model string is a known Groq model.
    pub fn is_known_model(model: &str) -> bool {
        GROQ_MODELS.contains(&model)
    }

    fn as_inner(&self, model: &str) -> OpenAiProvider {
        OpenAiProvider::new(&self.base_url, &self.api_key, model)
    }
}

impl LlmProvider for GroqProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Groq
    }

    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        self.as_inner(&req.model).complete(req)
    }

    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        self.as_inner(&req.model).complete_stream(req, on_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, CompletionRequest, ToolChoice, ToolDefinition};

    fn make_request(model: &str) -> CompletionRequest {
        CompletionRequest {
            model: model.into(),
            messages: vec![ChatMessage::text("user", "hello")],
            max_tokens: Some(64),
            temperature: Some(0.7),
            tools: None,
            tool_choice: None,
            enable_cache: false,
        }
    }

    #[test]
    fn test_groq_provider_type() {
        let p = GroqProvider::new("gsk-test", "llama-3.3-70b-versatile");
        assert_eq!(p.provider_type(), ProviderType::Groq);
    }

    #[test]
    fn test_groq_default_model() {
        let p = GroqProvider::new("gsk-test", "mixtral-8x7b-32768");
        assert_eq!(p.default_model, "mixtral-8x7b-32768");
    }

    #[test]
    fn test_groq_model_validation() {
        assert!(GroqProvider::is_known_model("llama-3.3-70b-versatile"));
        assert!(GroqProvider::is_known_model("mixtral-8x7b-32768"));
        assert!(GroqProvider::is_known_model("gemma2-9b-it"));
        assert!(!GroqProvider::is_known_model("gpt-4o"));
        assert!(!GroqProvider::is_known_model(""));
    }

    #[test]
    fn test_groq_complete_no_server_returns_error() {
        // Without a real Groq server, complete() should return a provider error.
        let p = GroqProvider::new("gsk-fake", "llama-3.3-70b-versatile");
        let req = make_request("llama-3.3-70b-versatile");
        let result = p.complete(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_groq_stream_no_server_returns_error() {
        let p = GroqProvider::new("gsk-fake", "llama-3.3-70b-versatile");
        let req = make_request("llama-3.3-70b-versatile");
        let mut tokens = Vec::new();
        let result = p.complete_stream(&req, &mut |t| tokens.push(t.to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_groq_tools_request_builds() {
        // Verify that a request with tools can be constructed and passed
        // to the provider without panicking (the HTTP call will fail).
        let p = GroqProvider::new("gsk-fake", "llama-3.3-70b-versatile");
        let req = CompletionRequest {
            model: "llama-3.3-70b-versatile".into(),
            messages: vec![ChatMessage::text("user", "What is the weather?")],
            max_tokens: Some(128),
            temperature: None,
            tools: Some(vec![ToolDefinition::new(
                "get_weather",
                "Get current weather for a city",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }),
            )]),
            tool_choice: Some(ToolChoice::Auto),
            enable_cache: false,
        };
        // Should fail with network error, not a panic.
        let result = p.complete(&req);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("provider error") || err_msg.contains("error"),
            "unexpected error format: {}",
            err_msg
        );
    }

    #[test]
    fn test_groq_inner_base_url() {
        let p = GroqProvider::new("gsk-test", "gemma2-9b-it");
        let inner = p.as_inner("gemma2-9b-it");
        assert_eq!(inner.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(inner.api_key, "gsk-test");
        assert_eq!(inner.default_model, "gemma2-9b-it");
    }
}
