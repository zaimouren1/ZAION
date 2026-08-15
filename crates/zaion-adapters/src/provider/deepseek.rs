//! DeepSeek provider using the OpenAI-compatible API at `https://api.deepseek.com/v1`.

use super::{CompletionRequest, CompletionResponse, LlmProvider, OpenAiProvider, ProviderType};
use crate::AdapterError;

pub struct DeepSeekProvider {
    pub api_key: String,
    /// Default model name, e.g. `"deepseek-chat"`.
    pub default_model: String,
}

impl DeepSeekProvider {
    /// Create a DeepSeek provider with the given API key and model.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            default_model: model.into(),
        }
    }

    fn as_inner(&self, model: &str) -> OpenAiProvider {
        OpenAiProvider::new("https://api.deepseek.com/v1", &self.api_key, model)
    }
}

impl LlmProvider for DeepSeekProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAiCompatible
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
