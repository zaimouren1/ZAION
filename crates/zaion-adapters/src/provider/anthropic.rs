//! Anthropic Claude provider (batch + streaming) with prompt-cache support.
//!
//! Implements the `system_and_3` cache strategy: injects `cache_control`
//! breakpoints on the system prompt and the last 3 non-system messages.
//! The Hermes equivalent lives in `prompt_caching.py`.

use super::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ProviderType,
    ToolCall,
};
use crate::AdapterError;
use serde::Deserialize;

pub struct AnthropicProvider {
    pub api_key: String,
    pub default_model: String,
    pub base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            default_model: default_model.into(),
            base_url: "https://api.anthropic.com".into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse_anthropic_stop_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn") => FinishReason::Stop,
        Some("tool_use") => FinishReason::ToolUse,
        Some("max_tokens") => FinishReason::MaxTokens,
        _ => FinishReason::Stop,
    }
}

/// Build Anthropic messages array, handling tool-related messages.
fn build_anthropic_messages(
    messages: &[ChatMessage],
    openai_tools_format: bool,
) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            if m.role == "tool" && openai_tools_format {
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": m.tool_call_id,
                    "content": m.content,
                })
            } else if m.role == "tool" {
                serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id,
                        "content": m.content,
                    }]
                })
            } else if !m.tool_calls.is_empty() && openai_tools_format {
                let mut assistant = serde_json::json!({
                    "role": "assistant",
                    "content": m.content,
                    "tool_calls": m.tool_calls.iter().map(|tc| serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments.to_string(),
                        },
                    })).collect::<Vec<_>>(),
                });
                if let Some(reasoning) = &m.reasoning_content {
                    assistant["reasoning_content"] = serde_json::Value::String(reasoning.clone());
                }
                assistant
            } else if !m.tool_calls.is_empty() {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();
                if !m.content.is_empty() {
                    content_blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                }
                for tc in &m.tool_calls {
                    content_blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                serde_json::json!({
                    "role": "assistant",
                    "content": content_blocks,
                })
            } else {
                serde_json::json!({ "role": m.role, "content": m.content })
            }
        })
        .collect()
}

/// Build the JSON request body for the Anthropic API.
fn build_anthropic_body(
    req: &CompletionRequest,
    system_text: Option<String>,
    human_msgs: &[&ChatMessage],
    stream: bool,
    openai_tools_format: bool,
) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = if req.enable_cache {
        build_anthropic_messages_with_cache(
            &human_msgs.iter().copied().cloned().collect::<Vec<_>>(),
        )
    } else {
        build_anthropic_messages(
            &human_msgs.iter().copied().cloned().collect::<Vec<_>>(),
            openai_tools_format,
        )
    };
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(1024),
    });

    // When cache is enabled, wrap system as a content block array with cache_control.
    if req.enable_cache {
        if let Some(sys) = system_text {
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": sys,
                "cache_control": {"type": "ephemeral"},
            }]);
        }
    } else if let Some(sys) = system_text {
        body["system"] = serde_json::Value::String(sys);
    }

    if stream {
        body["stream"] = serde_json::json!(true);
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = serde_json::Value::from(temp);
    }
    if let Some(tools) = &req.tools {
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                if openai_tools_format {
                    t.to_openai_compat_json()
                } else {
                    t.to_anthropic_json()
                }
            })
            .collect();
        body["tools"] = serde_json::json!(tools_json);
    }
    if let Some(choice) = &req.tool_choice {
        body["tool_choice"] = choice.to_anthropic_json();
    }
    body
}

/// Apply Anthropic prompt cache control breakpoints (system_and_3 strategy).
///
/// Strategy:
///   - System prompt: inject cache_control on the last system block (in build_anthropic_body).
///   - Messages: inject cache_control on the last 3 non-system messages.
///
/// Only text content blocks are wrapped — tool blocks are left unchanged.
fn build_anthropic_messages_with_cache(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut json_messages = build_anthropic_messages(messages, false);
    let total = json_messages.len();
    let breakpoint_count = 3usize.min(total);
    for i in 0..breakpoint_count {
        let idx = total - 1 - i;
        apply_cache_control_to_message(&mut json_messages[idx]);
    }
    json_messages
}

/// Inject `cache_control: {type: ephemeral}` into the last content block of a message.
fn apply_cache_control_to_message(msg: &mut serde_json::Value) {
    if let Some(content) = msg.get_mut("content") {
        match content {
            serde_json::Value::String(s) => {
                let text = s.clone();
                *content = serde_json::json!([{
                    "type": "text",
                    "text": text,
                    "cache_control": {"type": "ephemeral"},
                }]);
            }
            serde_json::Value::Array(blocks) => {
                if let Some(last) = blocks.last_mut() {
                    if last.get("type").and_then(|t| t.as_str()) == Some("text") {
                        last["cache_control"] = serde_json::json!({"type": "ephemeral"});
                    }
                }
            }
            _ => {}
        }
    }
}

// ─── Streaming tool-use accumulator ─────────────────────────────────────────

#[derive(Default)]
struct AnthropicStreamToolAccumulator {
    entries: Vec<AnthropicStreamToolEntry>,
}

struct AnthropicStreamToolEntry {
    id: String,
    name: String,
    json_buf: String,
}

impl AnthropicStreamToolAccumulator {
    fn start_block(&mut self, id: &str, name: &str) {
        self.entries.push(AnthropicStreamToolEntry {
            id: id.to_string(),
            name: name.to_string(),
            json_buf: String::new(),
        });
    }

    fn append_json(&mut self, partial: &str) {
        if let Some(entry) = self.entries.last_mut() {
            entry.json_buf.push_str(partial);
        }
    }

    fn into_tool_calls(self) -> Vec<ToolCall> {
        self.entries
            .into_iter()
            .filter(|e| !e.id.is_empty())
            .map(|e| {
                let arguments = serde_json::from_str(&e.json_buf)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                ToolCall {
                    id: e.id,
                    name: e.name,
                    arguments,
                }
            })
            .collect()
    }
}

// ─── LlmProvider impl ───────────────────────────────────────────────────────

impl LlmProvider for AnthropicProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }

    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        use std::io::{BufRead, BufReader};
        if self.api_key.is_empty() {
            return Err(AdapterError::Provider("api_key not configured".into()));
        }
        let (system_msgs, human_msgs): (Vec<_>, Vec<_>) =
            req.messages.iter().partition(|m| m.role == "system");
        let system_text: Option<String> = if system_msgs.is_empty() {
            None
        } else {
            Some(
                system_msgs
                    .iter()
                    .map(|m| m.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        let openai_tools_format = !self.base_url.contains("api.anthropic.com");
        let body =
            build_anthropic_body(req, system_text, &human_msgs, true, openai_tools_format);
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .map_err(|e| AdapterError::Provider(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Provider(format!("HTTP {}: {}", status, text)));
        }
        let reader = BufReader::new(resp);
        let mut full_content = String::new();
        let mut reasoning_content = String::new();
        let mut model_name = req.model.clone();
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut cache_read_tokens = 0u32;
        let mut cache_write_tokens = 0u32;
        let mut finish_reason = FinishReason::Stop;
        let mut tc_acc = AnthropicStreamToolAccumulator::default();
        for line in reader.lines() {
            let line = line.map_err(|e| AdapterError::Provider(e.to_string()))?;
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                match event["type"].as_str() {
                    Some("message_start") => {
                        if let Some(m) = event["message"]["model"].as_str() {
                            model_name = m.to_string();
                        }
                        if let Some(u) = event["message"]["usage"].as_object() {
                            input_tokens =
                                u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            if let Some(cr) =
                                u.get("cache_read_input_tokens").and_then(|v| v.as_u64())
                            {
                                if cr > 0 {
                                    cache_read_tokens = cr as u32;
                                }
                            }
                            if let Some(cw) = u
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                            {
                                if cw > 0 {
                                    cache_write_tokens = cw as u32;
                                }
                            }
                        }
                    }
                    Some("content_block_start") => {
                        let cb = &event["content_block"];
                        if cb["type"].as_str() == Some("tool_use") {
                            let id = cb["id"].as_str().unwrap_or("");
                            let name = cb["name"].as_str().unwrap_or("");
                            tc_acc.start_block(id, name);
                        }
                    }
                    Some("content_block_delta") => {
                        let delta = &event["delta"];
                        match delta["type"].as_str() {
                            Some("text_delta") => {
                                if let Some(text) = delta["text"].as_str() {
                                    if !text.is_empty() {
                                        full_content.push_str(text);
                                        on_token(text);
                                    }
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(partial) = delta["partial_json"].as_str() {
                                    tc_acc.append_json(partial);
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(thinking) = delta["thinking"].as_str() {
                                    reasoning_content.push_str(thinking);
                                }
                            }
                            _ => {}
                        }
                    }
                    Some("message_delta") => {
                        if let Some(u) = event["usage"].as_object() {
                            output_tokens =
                                u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        }
                        if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                            finish_reason = parse_anthropic_stop_reason(Some(reason));
                        }
                    }
                    _ => {}
                }
            }
        }
        let tool_calls = tc_acc.into_tool_calls();
        if !tool_calls.is_empty() && finish_reason == FinishReason::Stop {
            finish_reason = FinishReason::ToolUse;
        }
        Ok(CompletionResponse {
            content: full_content,
            model: model_name,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            tool_calls,
            finish_reason,
            reasoning_content,
        })
    }

    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        if self.api_key.is_empty() {
            return Err(AdapterError::Provider("api_key not configured".into()));
        }
        let (system_msgs, human_msgs): (Vec<_>, Vec<_>) =
            req.messages.iter().partition(|m| m.role == "system");
        let system_text: Option<String> = if system_msgs.is_empty() {
            None
        } else {
            Some(
                system_msgs
                    .iter()
                    .map(|m| m.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        let openai_tools_format = !self.base_url.contains("api.anthropic.com");
        let body =
            build_anthropic_body(req, system_text, &human_msgs, false, openai_tools_format);
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| AdapterError::Provider(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Provider(format!("HTTP {}: {}", status, text)));
        }
        let parsed: AnthropicResponse = resp
            .json()
            .map_err(|e| AdapterError::Provider(e.to_string()))?;
        let finish_reason = parse_anthropic_stop_reason(parsed.stop_reason.as_deref());
        let mut text_content = String::new();
        let mut tool_calls = Vec::new();
        for block in parsed.content {
            match block.content_type.as_str() {
                "text" => {
                    if let Some(t) = block.text {
                        text_content.push_str(&t);
                    }
                }
                "tool_use" => {
                    tool_calls.push(ToolCall {
                        id: block.id.unwrap_or_default(),
                        name: block.name.unwrap_or_default(),
                        arguments: block
                            .input
                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                    });
                }
                _ => {}
            }
        }
        Ok(CompletionResponse {
            content: text_content,
            model: parsed.model,
            input_tokens: parsed.usage.input_tokens,
            output_tokens: parsed.usage.output_tokens,
            cache_read_tokens: parsed.usage.cache_read_input_tokens,
            cache_write_tokens: parsed.usage.cache_creation_input_tokens,
            tool_calls,
            finish_reason,
        
    reasoning_content: String::new(),})
    }
}

// ─── Cache tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn make_msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    #[test]
    fn cache_injects_on_last_3_messages() {
        let messages = vec![
            make_msg("user", "msg1"),
            make_msg("assistant", "reply1"),
            make_msg("user", "msg2"),
            make_msg("assistant", "reply2"),
            make_msg("user", "msg3"),
        ];
        let json = build_anthropic_messages_with_cache(&messages);
        assert_eq!(json.len(), 5);
        for (i, entry) in json.iter().enumerate().skip(2).take(3) {
            let content = &entry["content"];
            let has_cache = match content {
                serde_json::Value::Array(blocks) => {
                    blocks.iter().any(|b| b.get("cache_control").is_some())
                }
                _ => false,
            };
            assert!(has_cache, "message {} should have cache_control", i);
        }
    }

    #[test]
    fn cache_handles_short_history() {
        let messages = vec![make_msg("user", "hello")];
        let json = build_anthropic_messages_with_cache(&messages);
        assert_eq!(json.len(), 1);
        let content = &json[0]["content"];
        let has_cache = match content {
            serde_json::Value::Array(blocks) => {
                blocks.iter().any(|b| b.get("cache_control").is_some())
            }
            _ => false,
        };
        assert!(has_cache);
    }

    #[test]
    fn cache_empty_messages_no_panic() {
        let messages: Vec<ChatMessage> = vec![];
        let json = build_anthropic_messages_with_cache(&messages);
        assert!(json.is_empty());
    }

    #[test]
    fn system_block_gets_cache_in_body() {
        let req = CompletionRequest {
            model: "claude-haiku-4-5".into(),
            messages: vec![make_msg("user", "hello")],
            max_tokens: Some(100),
            temperature: None,
            tools: None,
            tool_choice: None,
            enable_cache: true,
        };
        let system = Some("You are a helpful assistant".to_string());
        let human: Vec<&ChatMessage> = req.messages.iter().collect();
        let body = build_anthropic_body(&req, system, &human, false, false);
        assert!(
            body["system"].is_array(),
            "system should be a JSON array when caching"
        );
        let sys_block = &body["system"][0];
        assert_eq!(sys_block["type"], "text");
        assert_eq!(sys_block["cache_control"]["type"], "ephemeral");
    }
}
