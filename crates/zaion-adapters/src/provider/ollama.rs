//! Local Ollama provider using the OpenAI-compatible `/v1` endpoint.
//!
//! Ollama exposes an OpenAI-compatible API at `http://localhost:11434/v1`,
//! so this provider delegates to `OpenAiProvider` with an empty API key.

use super::{CompletionRequest, CompletionResponse, LlmProvider, OpenAiProvider, ProviderType};
use crate::AdapterError;

pub struct OllamaProvider {
    /// Base URL for the Ollama server's OpenAI-compatible endpoint.
    /// Defaults to `http://localhost:11434/v1`.
    pub base_url: String,
    /// Default model name, e.g. `"llama3.2"`.
    pub default_model: String,
}

impl OllamaProvider {
    /// Create an Ollama provider with an explicit base URL and model.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            default_model: model.into(),
        }
    }

    /// Shorthand: create an Ollama provider pointing at `localhost:11434`.
    pub fn local(model: impl Into<String>) -> Self {
        Self::new("http://localhost:11434/v1", model)
    }

    fn as_inner(&self, model: &str) -> OpenAiProvider {
        OpenAiProvider::new(&self.base_url, "", model)
    }
}

impl LlmProvider for OllamaProvider {
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
