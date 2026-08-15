//! Mistral AI provider using the Mistral API at `https://api.mistral.ai/v1`.
//!
//! Mistral's chat completions endpoint is OpenAI-compatible in wire format,
//! so this provider delegates to `OpenAiProvider` with the Mistral base URL.
//! Tool calling uses the same OpenAI-style `tools` / `tool_choice` format
//! that Mistral natively supports.

use super::{CompletionRequest, CompletionResponse, LlmProvider, OpenAiProvider, ProviderType};
use crate::AdapterError;

/// Known Mistral-hosted models.
pub const MISTRAL_MODELS: &[&str] = &[
    "mistral-large-latest",
    "mistral-medium-latest",
    "mistral-small-latest",
];

pub struct MistralProvider {
    pub api_key: String,
    pub base_url: String,
    /// Default model name, e.g. `"mistral-large-latest"`.
    pub default_model: String,
}

impl MistralProvider {
    /// Create a Mistral provider with the given API key and model.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.mistral.ai/v1".to_string(),
            default_model: model.into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Validate that a model string is a known Mistral model.
    pub fn is_known_model(model: &str) -> bool {
        MISTRAL_MODELS.contains(&model)
    }

    fn as_inner(&self, model: &str) -> OpenAiProvider {
        OpenAiProvider::new(&self.base_url, &self.api_key, model)
    }
}

impl LlmProvider for MistralProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Mistral
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
    fn test_mistral_provider_type() {
        let p = MistralProvider::new("mk-test", "mistral-large-latest");
        assert_eq!(p.provider_type(), ProviderType::Mistral);
    }

    #[test]
    fn test_mistral_default_model() {
        let p = MistralProvider::new("mk-test", "mistral-small-latest");
        assert_eq!(p.default_model, "mistral-small-latest");
    }

    #[test]
    fn test_mistral_model_validation() {
        assert!(MistralProvider::is_known_model("mistral-large-latest"));
        assert!(MistralProvider::is_known_model("mistral-medium-latest"));
        assert!(MistralProvider::is_known_model("mistral-small-latest"));
        assert!(!MistralProvider::is_known_model("gpt-4o"));
        assert!(!MistralProvider::is_known_model(""));
    }

    #[test]
    fn test_mistral_complete_no_server_returns_error() {
        let p = MistralProvider::new("mk-fake", "mistral-large-latest");
        let req = make_request("mistral-large-latest");
        let result = p.complete(&req);
        assert!(result.is_err());
    }

    #[test]
    fn test_mistral_stream_no_server_returns_error() {
        let p = MistralProvider::new("mk-fake", "mistral-large-latest");
        let req = make_request("mistral-large-latest");
        let mut tokens = Vec::new();
        let result = p.complete_stream(&req, &mut |t| tokens.push(t.to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_mistral_tools_request_builds() {
        let p = MistralProvider::new("mk-fake", "mistral-large-latest");
        let req = CompletionRequest {
            model: "mistral-large-latest".into(),
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
    fn test_mistral_inner_base_url() {
        let p = MistralProvider::new("mk-test", "mistral-medium-latest");
        let inner = p.as_inner("mistral-medium-latest");
        assert_eq!(inner.base_url, "https://api.mistral.ai/v1");
        assert_eq!(inner.api_key, "mk-test");
        assert_eq!(inner.default_model, "mistral-medium-latest");
    }
}
