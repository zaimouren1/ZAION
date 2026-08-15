//! OpenAI and OpenAI-compatible provider (batch + streaming).
//!
//! Handles the quirks of many OpenAI-compatible endpoints:
//! - Standard `prompt_tokens` / `completion_tokens` naming *and* the
//!   `input_tokens` / `output_tokens` aliases used by some providers.
//! - Cache-read tokens via `cache_read_input_tokens` (Anthropic-style)
//!   or `cached_tokens` (Moonshot/Kimi) or
//!   `prompt_tokens_details.cached_tokens` (Kimi K2).
//! - MiniMax response objects where content lives in `reasoning_content`,
//!   `audio_content`, or `reasoning_details[].text`.
//! - System-prompt merging: many providers (MiniMax) only accept one
//!   system message, so we concatenate consecutive system messages.

use super::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ProviderType,
    ToolCall,
};
use crate::AdapterError;
use serde::Deserialize;

fn openai_chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if lowered.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

pub struct OpenAiProvider {
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
}

impl OpenAiProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            default_model: default_model.into(),
        }
    }
}

// ─── Internal wire types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    /// MiniMax: actual response sometimes here
    audio_content: Option<String>,
    /// MiniMax: reasoning steps array — extract .text from each entry
    reasoning_details: Option<Vec<ReasoningDetail>>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize)]
struct ReasoningDetail {
    text: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunction,
}

#[derive(Deserialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize, Default)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cached_tokens: u32,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

impl OpenAiUsage {
    fn prompt_tokens_normalized(&self) -> u32 {
        let total = if self.prompt_tokens > 0 {
            self.prompt_tokens
        } else {
            self.input_tokens
        };
        total
            .saturating_sub(self.cache_read_tokens())
            .saturating_sub(self.cache_write_tokens())
    }
    fn completion_tokens_normalized(&self) -> u32 {
        if self.completion_tokens > 0 {
            self.completion_tokens
        } else {
            self.output_tokens
        }
    }
    fn cache_read_tokens(&self) -> u32 {
        if self.cache_read_input_tokens > 0 {
            self.cache_read_input_tokens
        } else if self.cached_tokens > 0 {
            self.cached_tokens
        } else if let Some(ref d) = self.prompt_tokens_details {
            d.cached_tokens
        } else {
            0
        }
    }
    fn cache_write_tokens(&self) -> u32 {
        self.cache_creation_input_tokens
    }
}

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    model: Option<String>,
    choices: Vec<OpenAiStreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiStreamDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    audio_content: Option<String>,
    tool_calls: Option<Vec<OpenAiStreamToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiStreamToolCall {
    index: usize,
    id: Option<String>,
    function: Option<OpenAiStreamFunction>,
}

#[derive(Deserialize)]
struct OpenAiStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

// ─── Body / message builders ────────────────────────────────────────────────

fn build_openai_body(req: &CompletionRequest, stream: bool) -> serde_json::Value {
    let messages = build_openai_messages(&req.messages);
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
    });
    if stream {
        body["stream"] = serde_json::json!(true);
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }
    if let Some(tools) = &req.tools {
        let tools_json: Vec<serde_json::Value> = tools.iter().map(|t| t.to_openai_json()).collect();
        body["tools"] = serde_json::json!(tools_json);
    }
    if let Some(choice) = &req.tool_choice {
        body["tool_choice"] = choice.to_openai_json();
    }
    body
}

/// Public re-export kept for existing external callers that expected
/// this name from the monolithic `provider` module.
pub fn build_openai_messages_pub(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    build_openai_messages(messages)
}

fn build_openai_messages(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    let mut merged: Vec<&ChatMessage> = Vec::with_capacity(messages.len());
    let mut system_parts: Vec<&str> = Vec::new();
    let mut past_system = false;
    for m in messages {
        if !past_system && m.role == "system" {
            if !m.content.is_empty() {
                system_parts.push(&m.content);
            }
        } else {
            past_system = true;
            merged.push(m);
        }
    }
    let mut result: Vec<serde_json::Value> = Vec::new();
    if !system_parts.is_empty() {
        result.push(serde_json::json!({
            "role": "system",
            "content": system_parts.join("\n\n"),
        }));
    }
    for m in merged {
        let mut msg = serde_json::json!({
            "role": m.role,
            "content": m.content,
        });
        if !m.tool_calls.is_empty() {
            let tcs: Vec<serde_json::Value> = m
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments.to_string(),
                        }
                    })
                })
                .collect();
            msg["tool_calls"] = serde_json::json!(tcs);
        }
        if let Some(ref id) = m.tool_call_id {
            msg["tool_call_id"] = serde_json::json!(id);
        }
        result.push(msg);
    }
    result
}

fn parse_openai_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("tool_calls") => FinishReason::ToolUse,
        Some("length") => FinishReason::MaxTokens,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

fn parse_openai_tool_calls(raw: &[OpenAiToolCall]) -> Vec<ToolCall> {
    raw.iter()
        .map(|tc| {
            let arguments = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments,
            }
        })
        .collect()
}

fn extract_text_from_openai_message(msg: &OpenAiMessage) -> String {
    if let Some(content) = msg.content.as_deref().filter(|text| !text.is_empty()) {
        return content.to_string();
    }
    if let Some(reasoning) = msg
        .reasoning_content
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        return reasoning.to_string();
    }
    if let Some(audio) = msg.audio_content.as_deref().filter(|text| !text.is_empty()) {
        return audio.to_string();
    }
    msg.reasoning_details
        .as_deref()
        .and_then(|v| {
            v.iter()
                .filter_map(|d| d.text.as_deref())
                .find(|s| !s.is_empty())
        })
        .map(str::to_string)
        .unwrap_or_default()
}

fn append_stream_text(full_content: &mut String, text: &str, on_token: &mut dyn FnMut(&str)) {
    if text.is_empty() {
        return;
    }
    full_content.push_str(text);
    on_token(text);
}

fn apply_final_text_snapshot(
    full_content: &mut String,
    text: &str,
    on_token: &mut dyn FnMut(&str),
) {
    if text.is_empty() {
        return;
    }
    if full_content.is_empty() {
        full_content.push_str(text);
        on_token(text);
    } else if let Some(suffix) = text.strip_prefix(full_content.as_str()) {
        if !suffix.is_empty() {
            full_content.push_str(suffix);
            on_token(suffix);
        }
    }
}

fn extract_responses_output_text(response: &serde_json::Value) -> Option<String> {
    response
        .get("output_text")
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| {
            response
                .get("output")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .flat_map(|item| {
                            item.get("content")
                                .and_then(|value| value.as_array())
                                .into_iter()
                                .flatten()
                        })
                        .filter_map(|part| {
                            part.get("text")
                                .or_else(|| part.get("output_text"))
                                .and_then(|value| value.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join("")
                })
                .filter(|text| !text.is_empty())
        })
}

fn responses_usage_tokens(response: &serde_json::Value) -> (u32, u32, u32, u32) {
    let Some(usage) = response.get("usage") else {
        return (0, 0, 0, 0);
    };
    let parse = |key: &str| {
        usage
            .get(key)
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };
    let input = parse("input_tokens");
    let output = parse("output_tokens");
    let cache_read = usage
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    (input.saturating_sub(cache_read), output, cache_read, 0)
}

#[allow(clippy::too_many_arguments)]
fn apply_usage_tokens(
    input_tokens: &mut u32,
    output_tokens: &mut u32,
    cache_read_tokens: &mut u32,
    cache_write_tokens: &mut u32,
    input: u32,
    output: u32,
    cache_read: u32,
    cache_write: u32,
) {
    if input > 0 {
        *input_tokens = input;
    }
    if output > 0 {
        *output_tokens = output;
    }
    if cache_read > 0 {
        *cache_read_tokens = cache_read;
    }
    if cache_write > 0 {
        *cache_write_tokens = cache_write;
    }
}

// ─── Streaming tool-call accumulator ────────────────────────────────────────

#[derive(Default)]
struct StreamToolCallAccumulator {
    entries: Vec<StreamToolCallEntry>,
}

struct StreamToolCallEntry {
    id: String,
    name: String,
    arguments_buf: String,
}

impl StreamToolCallAccumulator {
    fn process_delta(&mut self, delta: &OpenAiStreamToolCall) {
        while self.entries.len() <= delta.index {
            self.entries.push(StreamToolCallEntry {
                id: String::new(),
                name: String::new(),
                arguments_buf: String::new(),
            });
        }
        let entry = &mut self.entries[delta.index];
        if let Some(ref id) = delta.id {
            entry.id = id.clone();
        }
        if let Some(ref func) = delta.function {
            if let Some(ref name) = func.name {
                entry.name = name.clone();
            }
            if let Some(ref args) = func.arguments {
                entry.arguments_buf.push_str(args);
            }
        }
    }

    fn into_tool_calls(self) -> Vec<ToolCall> {
        self.entries
            .into_iter()
            .filter(|e| !e.id.is_empty())
            .map(|e| {
                let arguments = serde_json::from_str(&e.arguments_buf)
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

impl LlmProvider for OpenAiProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAiCompatible
    }

    fn complete_stream(
        &self,
        req: &CompletionRequest,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<CompletionResponse, AdapterError> {
        use std::io::{BufRead, BufReader};
        let url = openai_chat_completions_url(&self.base_url);
        let body = build_openai_body(req, true);
        let client = reqwest::blocking::Client::new();
        let mut builder = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream");
        if !self.api_key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", self.api_key));
        }
        let resp = builder
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
        let mut model_name = req.model.clone();
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut cache_read_tokens = 0u32;
        let mut cache_write_tokens = 0u32;
        let mut finish_reason = FinishReason::Stop;
        let mut tc_acc = StreamToolCallAccumulator::default();
        for line in reader.lines() {
            let line = line.map_err(|e| AdapterError::Provider(e.to_string()))?;
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];
            if data == "[DONE]" {
                break;
            }
            if let Ok(chunk) = serde_json::from_str::<OpenAiStreamChunk>(data) {
                if let Some(m) = chunk.model {
                    model_name = m;
                }
                if let Some(usage) = chunk.usage {
                    apply_usage_tokens(
                        &mut input_tokens,
                        &mut output_tokens,
                        &mut cache_read_tokens,
                        &mut cache_write_tokens,
                        usage.prompt_tokens_normalized(),
                        usage.completion_tokens_normalized(),
                        usage.cache_read_tokens(),
                        usage.cache_write_tokens(),
                    );
                }
                if let Some(choice) = chunk.choices.into_iter().next() {
                    if let Some(ref reason) = choice.finish_reason {
                        finish_reason = parse_openai_finish_reason(Some(reason.as_str()));
                    }
                    if let Some(token) = choice
                        .delta
                        .content
                        .or(choice.delta.audio_content)
                        .or(choice.delta.reasoning_content)
                    {
                        append_stream_text(&mut full_content, &token, on_token);
                    }
                    if let Some(tcs) = choice.delta.tool_calls {
                        for tc_delta in &tcs {
                            tc_acc.process_delta(tc_delta);
                        }
                    }
                }
            }
            if let Ok(full) = serde_json::from_str::<OpenAiResponse>(data) {
                if let Some(usage) = full.usage {
                    apply_usage_tokens(
                        &mut input_tokens,
                        &mut output_tokens,
                        &mut cache_read_tokens,
                        &mut cache_write_tokens,
                        usage.prompt_tokens_normalized(),
                        usage.completion_tokens_normalized(),
                        usage.cache_read_tokens(),
                        usage.cache_write_tokens(),
                    );
                }
                if let Some(choice) = full.choices.into_iter().next() {
                    if choice.finish_reason.is_some() {
                        finish_reason = parse_openai_finish_reason(choice.finish_reason.as_deref());
                    }
                    if full_content.is_empty() {
                        let text = extract_text_from_openai_message(&choice.message);
                        apply_final_text_snapshot(&mut full_content, &text, on_token);
                    }
                }
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                match event.get("type").and_then(|value| value.as_str()) {
                    Some("response.output_text.delta") => {
                        if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                            append_stream_text(&mut full_content, delta, on_token);
                        }
                    }
                    Some("response.output_text.done") => {
                        if let Some(text) = event.get("text").and_then(|value| value.as_str()) {
                            apply_final_text_snapshot(&mut full_content, text, on_token);
                        }
                    }
                    Some("response.completed") => {
                        if let Some(response) = event.get("response") {
                            if let Some(m) = response.get("model").and_then(|value| value.as_str())
                            {
                                model_name = m.to_string();
                            }
                            if let Some(text) = extract_responses_output_text(response) {
                                apply_final_text_snapshot(&mut full_content, &text, on_token);
                            }
                            let (input, output, cache_read, cache_write) =
                                responses_usage_tokens(response);
                            apply_usage_tokens(
                                &mut input_tokens,
                                &mut output_tokens,
                                &mut cache_read_tokens,
                                &mut cache_write_tokens,
                                input,
                                output,
                                cache_read,
                                cache_write,
                            );
                            finish_reason = FinishReason::Stop;
                        }
                    }
                    Some("response.failed") => {
                        let message = event
                            .get("response")
                            .and_then(|response| response.get("error"))
                            .and_then(|error| {
                                error
                                    .get("message")
                                    .or_else(|| error.get("code"))
                                    .and_then(|value| value.as_str())
                            })
                            .or_else(|| {
                                event
                                    .get("error")
                                    .and_then(|error| error.get("message"))
                                    .and_then(|value| value.as_str())
                            })
                            .unwrap_or("responses api stream failed");
                        return Err(AdapterError::Provider(message.to_string()));
                    }
                    _ => {}
                }
            }
        }
        let tool_calls = tc_acc.into_tool_calls();
        if !tool_calls.is_empty() && finish_reason == FinishReason::Stop {
            finish_reason = FinishReason::ToolUse;
        }
        if full_content.trim().is_empty() && tool_calls.is_empty() {
            return Err(AdapterError::Provider(
                "provider returned no visible assistant content".to_string(),
            ));
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
        
    reasoning_content: String::new(),})
    }

    fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, AdapterError> {
        let url = openai_chat_completions_url(&self.base_url);
        let body = build_openai_body(req, false);
        let client = reqwest::blocking::Client::new();
        let mut builder = client.post(&url).header("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", self.api_key));
        }
        let resp = builder
            .json(&body)
            .send()
            .map_err(|e| AdapterError::Provider(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Provider(format!("HTTP {}: {}", status, text)));
        }
        let raw_body = resp
            .text()
            .map_err(|e| AdapterError::Provider(e.to_string()))?;
        if raw_body.contains("\"choices\":null") {
            let msg = serde_json::from_str::<serde_json::Value>(&raw_body)
                .ok()
                .and_then(|v| v["base_resp"]["status_msg"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| raw_body[..raw_body.len().min(200)].to_string());
            return Err(AdapterError::Provider(format!("API error: {}", msg)));
        }
        let raw_json: serde_json::Value = serde_json::from_str(&raw_body)
            .map_err(|e| AdapterError::Provider(format!("decode error: {}", e)))?;
        if raw_json.get("choices").is_none() {
            if let Some(content) = extract_responses_output_text(&raw_json) {
                if content.trim().is_empty() {
                    return Err(AdapterError::Provider(
                        "provider returned no visible assistant content".to_string(),
                    ));
                }
                let (input_tokens, output_tokens, cache_read, cache_write) =
                    responses_usage_tokens(&raw_json);
                return Ok(CompletionResponse {
                    content,
                    model: raw_json
                        .get("model")
                        .and_then(|value| value.as_str())
                        .unwrap_or(&req.model)
                        .to_string(),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens: cache_read,
                    cache_write_tokens: cache_write,
                    tool_calls: Vec::new(),
                    finish_reason: FinishReason::Stop,
                
    reasoning_content: String::new(),});
            }
            let preview = raw_body[..raw_body.len().min(200)].to_string();
            return Err(AdapterError::Provider(format!(
                "decode error: expected OpenAI chat completions or responses API body: {}",
                preview
            )));
        }
        let parsed: OpenAiResponse = serde_json::from_value(raw_json)
            .map_err(|e| AdapterError::Provider(format!("decode error: {}", e)))?;
        let OpenAiResponse {
            choices,
            usage,
            model,
        } = parsed;
        let choice = choices.into_iter().next();
        let finish_reason =
            parse_openai_finish_reason(choice.as_ref().and_then(|c| c.finish_reason.as_deref()));
        let (content, tool_calls) = match choice {
            Some(c) => {
                let msg = c.message;
                let text = extract_text_from_openai_message(&msg);
                let tcs = msg
                    .tool_calls
                    .map(|raw| parse_openai_tool_calls(&raw))
                    .unwrap_or_default();
                (text, tcs)
            }
            None => (String::new(), Vec::new()),
        };
        if content.trim().is_empty() && tool_calls.is_empty() {
            return Err(AdapterError::Provider(
                "provider returned no visible assistant content".to_string(),
            ));
        }
        let (input_tokens, output_tokens, cache_read, cache_write) = usage
            .map(|u| {
                (
                    u.prompt_tokens_normalized(),
                    u.completion_tokens_normalized(),
                    u.cache_read_tokens(),
                    u.cache_write_tokens(),
                )
            })
            .unwrap_or((0, 0, 0, 0));
        Ok(CompletionResponse {
            content,
            model: model.unwrap_or_else(|| req.model.clone()),
            input_tokens,
            output_tokens,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            tool_calls,
            finish_reason,
        
    reasoning_content: String::new(),})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;

    fn request() -> CompletionRequest {
        CompletionRequest {
            model: "gpt-5.5".to_string(),
            messages: vec![ChatMessage::text("user", "hello")],
            max_tokens: Some(32),
            temperature: Some(0.1),
            tools: None,
            tool_choice: None,
            enable_cache: false,
        }
    }

    fn spawn_responses_stream_mock() -> (SocketAddr, thread::JoinHandle<()>) {
        spawn_responses_stream_mock_expect_path("/v1/chat/completions")
    }

    fn spawn_responses_stream_mock_expect_path(
        expected_path: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_responses_stream_request(stream, expected_path);
        });
        (addr, handle)
    }

    fn handle_responses_stream_request(mut stream: TcpStream, expected_path: &str) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        let mut content_length = 0usize;
        let mut request_path = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if request_path.is_empty() {
                request_path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.trim_end().split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }
        if request_path != expected_path {
            let body = format!("wrong request path: {request_path}");
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            return;
        }

        let mut request_body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut request_body).unwrap();
        }
        let request_json: serde_json::Value = serde_json::from_slice(&request_body).unwrap();
        assert_eq!(request_json["stream"], true);

        let body = format!(
            "data: {}\n\n\
             data: {}\n\n\
             data: {}\n\n",
            serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "starship ",
            }),
            serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "ready",
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "model": "gpt-5.5",
                    "output_text": "starship ready",
                    "usage": {
                        "input_tokens": 3,
                        "output_tokens": 4
                    },
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": "starship ready"
                        }]
                    }]
                }
            })
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    #[test]
    fn complete_stream_accepts_responses_api_output_text_delta() {
        let (addr, server) = spawn_responses_stream_mock();
        let provider = OpenAiProvider::new(format!("http://{addr}/v1"), "sk-test", "gpt-5.5");
        let mut visible = String::new();

        let response = provider
            .complete_stream(&request(), &mut |token| visible.push_str(token))
            .unwrap();

        assert_eq!(visible, "starship ready");
        assert_eq!(response.content, "starship ready");
        assert_eq!(response.input_tokens, 3);
        assert_eq!(response.output_tokens, 4);
        server.join().unwrap();
    }

    #[test]
    fn complete_stream_normalizes_root_base_url_to_openai_v1_chat_completions() {
        let (addr, server) = spawn_responses_stream_mock_expect_path("/v1/chat/completions");
        let provider = OpenAiProvider::new(format!("http://{addr}"), "sk-test", "gpt-5.5");
        let mut visible = String::new();

        let response = provider
            .complete_stream(&request(), &mut |token| visible.push_str(token))
            .unwrap();

        assert_eq!(visible, "starship ready");
        assert_eq!(response.content, "starship ready");
        server.join().unwrap();
    }
}
