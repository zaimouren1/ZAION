//! LLM provider abstractions and implementations.
//!
//! H11 split: originally a single 1440-LoC `provider.rs`, now broken into
//! focused sub-modules:
//! - `mod.rs` — shared types (messages, requests, responses, trait)
//! - `openai.rs` — OpenAI / OpenAI-compatible provider (streaming + batch)
//! - `anthropic.rs` — Anthropic Claude provider + prompt-cache logic
//! - `ollama.rs` — local Ollama delegator (OpenAI-compatible)
//! - `deepseek.rs` — DeepSeek delegator (OpenAI-compatible)
//! - `embedding.rs` — OpenAI-compatible embeddings API

use crate::AdapterError;
use serde::{Deserialize, Serialize};

mod anthropic;
mod deepseek;
mod embedding;
mod groq;
mod mistral;
mod ollama;
mod openai;

pub use anthropic::AnthropicProvider;
pub use deepseek::DeepSeekProvider;
pub use embedding::{embed_text, EmbeddingRequest};
pub use groq::GroqProvider;
pub use mistral::MistralProvider;
pub use ollama::OllamaProvider;
pub use openai::{build_openai_messages_pub, OpenAiProvider};

// ─── Tool Calling Types ─────────────────────────────────────────────────────

/// A tool definition sent to the LLM.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing tool parameters.
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    /// Format this tool definition for the OpenAI tools API.
    pub fn to_openai_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }

    /// Format this tool definition for the Anthropic tools API.
    pub fn to_anthropic_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters,
        })
    }

    /// OpenAI-style tools array element (some gateway endpoints accept the
    /// Anthropic messages path but expect OpenAI tool shapes).
    pub fn to_openai_compat_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        })
    }
}

/// Controls which tools the LLM may call.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    /// Force a specific tool by name.
    Specific(String),
}

impl ToolChoice {
    pub fn to_openai_json(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Specific(name) => serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }),
        }
    }

    pub fn to_anthropic_json(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!({"type": "auto"}),
            ToolChoice::None => serde_json::json!({"type": "none"}),
            ToolChoice::Required => serde_json::json!({"type": "any"}),
            ToolChoice::Specific(name) => serde_json::json!({
                "type": "tool",
                "name": name
            }),
        }
    }
}

/// A tool call returned by the LLM.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Why the LLM stopped generating.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FinishReason {
    #[default]
    Stop,
    ToolUse,
    MaxTokens,
    ContentFilter,
}

// ─── Chat Message ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// Primary text content (backward compatible).
    pub content: String,
    /// Non-empty when role=assistant and LLM wants to call tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Set when role=tool — identifies which tool call this result is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// DeepSeek thinking-mode reasoning text to echo back (assistant turns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Thinking block signature to echo back (thinking-mode endpoints).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_signature: Option<String>,
}

impl ChatMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
            reasoning_signature: None,
        }
    }

    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: String::new(),
            tool_calls,
            tool_call_id: None,
            reasoning_content: None,
            reasoning_signature: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            reasoning_content: None,
            reasoning_signature: None,
        }
    }
}

// ─── Completion Request / Response ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Enable Anthropic prompt caching (system_and_3 strategy).
    #[serde(default)]
    pub enable_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_read_tokens: u32,
    #[serde(default)]
    pub cache_write_tokens: u32,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub finish_reason: FinishReason,
    /// DeepSeek thinking-mode reasoning text (must be echoed back on the
    /// next assistant turn for multi-turn tool calls).
    #[serde(default)]
    pub reasoning_content: String,
    /// Signature of the thinking block (echoed back alongside reasoning_content
    /// so thinking-mode endpoints accept the multi-turn request).
    #[serde(default)]
    pub reasoning_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    OpenAiCompatible,
    Anthropic,
    Groq,
    Mistral,
}

pub trait LlmProvider: Send + Sync {
    fn provider_type(&self) -> ProviderType;
    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError>;
    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        let resp = self.complete(req)?;
        for word in resp.content.split_inclusive(' ') {
            on_token(word);
        }
        Ok(resp)
    }
}

impl LlmProvider for Box<dyn LlmProvider> {
    fn provider_type(&self) -> ProviderType {
        (**self).provider_type()
    }
    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        (**self).complete(req)
    }
    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        (**self).complete_stream(req, on_token)
    }
}

impl<T: LlmProvider + ?Sized> LlmProvider for &T {
    fn provider_type(&self) -> ProviderType {
        (**self).provider_type()
    }
    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        (**self).complete(req)
    }
    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        (**self).complete_stream(req, on_token)
    }
}
