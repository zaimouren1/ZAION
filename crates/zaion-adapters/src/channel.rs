use crate::AdapterError;
use serde::{Deserialize, Serialize};
use zaion_types::envelope::{ingest as ingest_envelope, CanonicalEnvelope, CanonicalEnvelopeError};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel_id: String,
    pub thread_id: String,
    pub sender_id: String,
    pub text: String,
    pub message_id: String,
    pub timestamp: String,
    pub metadata: serde_json::Value,
}

impl InboundMessage {
    pub fn to_canonical_envelope(
        &self,
        source: impl Into<String>,
        principal: PrincipalId,
    ) -> Result<CanonicalEnvelope, CanonicalEnvelopeError> {
        let envelope = CanonicalEnvelope::new(
            source,
            principal,
            ChannelId(self.channel_id.clone()),
            ThreadId(self.thread_id.clone()),
            self.message_id.clone(),
            self.text.clone(),
            None,
        )
        .map(|envelope| {
            envelope
                .with_metadata("sender_id", serde_json::json!(self.sender_id))
                .with_metadata("transport_timestamp", serde_json::json!(self.timestamp))
                .with_metadata("adapter_metadata", self.metadata.clone())
        })?;
        ingest_envelope(&envelope)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel_id: String,
    pub thread_id: String,
    pub text: String,
    pub reply_to: Option<String>,
    pub metadata: serde_json::Value,
    /// Enable MarkdownV2 formatting for Telegram (C3 feature)
    pub parse_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Telegram,
    Terminal,
    Http,
    Slack,
    WeChat,
    WhatsApp,
    Matrix,
    Mattermost,
    Signal,
    HomeAssistant,
    Email,
    Sms,
}

pub trait ChannelAdapter: Send + Sync {
    fn channel_type(&self) -> ChannelType;
    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError>;
    fn send(&self, msg: &OutboundMessage) -> Result<(), AdapterError>;
}

#[derive(Deserialize)]
struct TgGetUpdatesResponse {
    ok: bool,
    result: Vec<TgUpdate>,
}

#[derive(Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Deserialize)]
struct TgMessage {
    message_id: i64,
    from: Option<TgUser>,
    chat: TgChat,
    text: Option<String>,
    date: i64,
}

#[derive(Deserialize)]
struct TgUser {
    id: i64,
    username: Option<String>,
}

#[derive(Deserialize)]
struct TgChat {
    id: i64,
}

pub struct TelegramAdapter {
    pub bot_token: String,
    pub channel_id: ChannelId,
    proxy_url: Option<String>,
    offset: std::sync::Mutex<i64>,
}

impl TelegramAdapter {
    pub fn new(bot_token: impl Into<String>, channel_id: ChannelId) -> Self {
        Self {
            bot_token: bot_token.into(),
            channel_id,
            proxy_url: None,
            offset: std::sync::Mutex::new(0),
        }
    }

    /// Set an HTTP/SOCKS5 proxy for Telegram API access.
    pub fn with_proxy(mut self, proxy_url: impl Into<String>) -> Self {
        let url = proxy_url.into();
        if !url.is_empty() {
            self.proxy_url = Some(url);
        }
        self
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.bot_token, method)
    }

    fn build_client(&self) -> Result<reqwest::blocking::Client, AdapterError> {
        let mut builder = reqwest::blocking::Client::builder();
        if let Some(ref proxy_url) = self.proxy_url {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|e| AdapterError::Channel(format!("invalid proxy: {}", e)))?;
            builder = builder.proxy(proxy);
        }
        builder
            .build()
            .map_err(|e| AdapterError::Channel(e.to_string()))
    }
}

impl ChannelAdapter for TelegramAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Telegram
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        if self.bot_token.is_empty() {
            return Err(AdapterError::Channel("bot_token not configured".into()));
        }
        let offset = *self.offset.lock().unwrap_or_else(|e| e.into_inner());
        let client = self.build_client()?;
        let resp = client
            .get(self.api_url("getUpdates"))
            .query(&[("timeout", "30"), ("offset", &offset.to_string())])
            .send()
            .map_err(|e| AdapterError::Channel(e.to_string()))?;
        let parsed: TgGetUpdatesResponse = resp
            .json()
            .map_err(|e| AdapterError::Channel(e.to_string()))?;
        if !parsed.ok {
            return Err(AdapterError::Channel(
                "telegram getUpdates returned ok=false".into(),
            ));
        }
        let mut messages = Vec::new();
        let mut max_update_id = offset;
        for update in parsed.result {
            if update.update_id >= max_update_id {
                max_update_id = update.update_id + 1;
            }
            if let Some(msg) = update.message {
                let text = msg.text.unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                let sender_id = msg
                    .from
                    .as_ref()
                    .map(|u| u.username.clone().unwrap_or_else(|| u.id.to_string()))
                    .unwrap_or_else(|| "unknown".into());
                messages.push(InboundMessage {
                    channel_id: "telegram".into(),
                    thread_id: msg.chat.id.to_string(),
                    sender_id,
                    text,
                    message_id: msg.message_id.to_string(),
                    timestamp: msg.date.to_string(),
                    metadata: serde_json::json!({ "chat_id": msg.chat.id }),
                });
            }
        }
        *self.offset.lock().unwrap_or_else(|e| e.into_inner()) = max_update_id;
        Ok(messages)
    }

    fn send(&self, msg: &OutboundMessage) -> Result<(), AdapterError> {
        if self.bot_token.is_empty() {
            return Err(AdapterError::Channel("bot_token not configured".into()));
        }

        // C3: Chunk message if it exceeds Telegram's 4096 char limit
        let chunks = chunk_message(&msg.text, 4096);
        let client = self.build_client()?;

        let mut reply_to = msg.reply_to.clone();
        for (i, chunk_text) in chunks.into_iter().enumerate() {
            let body = telegram_send_body(
                &msg.thread_id,
                &chunk_text,
                msg.parse_mode.as_deref(),
                if i == 0 { reply_to.as_deref() } else { None },
            );
            let resp = client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .map_err(|e| AdapterError::Channel(e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                return Err(AdapterError::Channel(format!("HTTP {}: {}", status, text)));
            }
            let json: serde_json::Value = resp
                .json()
                .map_err(|e| AdapterError::Channel(e.to_string()))?;
            if !json.get("ok").and_then(|ok| ok.as_bool()).unwrap_or(false) {
                return Err(AdapterError::Channel(format!(
                    "telegram sendMessage returned ok=false: {}",
                    json.get("description")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown error")
                )));
            }
            // After first chunk, use the response message_id as reply_to for threading
            reply_to = None;
        }
        Ok(())
    }
}

fn telegram_send_body(
    chat_id: &str,
    text: &str,
    parse_mode: Option<&str>,
    reply_to: Option<&str>,
) -> serde_json::Value {
    let parse_mode = parse_mode.and_then(|mode| {
        let trimmed = mode.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let text = if parse_mode == Some("MarkdownV2") {
        escape_markdown_v2(text)
    } else {
        text.to_string()
    };

    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
    });
    if let Some(parse_mode) = parse_mode {
        body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
    }
    if let Some(reply_to) = reply_to {
        body["reply_to_message_id"] = serde_json::Value::String(reply_to.to_string());
    }
    body
}

/// C3: Chunk message to fit within Telegram's 4096 character limit.
/// Breaks on newlines when possible to avoid mid-line splits.
fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_with_newline = format!("{}\n", line);
        if current.len() + line_with_newline.len() <= max_len {
            current.push_str(&line_with_newline);
        } else {
            if !current.is_empty() {
                chunks.push(current.trim_end().to_string());
            }
            current = line_with_newline;
            // If a single line exceeds max_len, force chunk it (char-safe)
            while current.len() > max_len {
                let split_at = current
                    .char_indices()
                    .take_while(|(i, _)| *i <= max_len)
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(max_len);
                let split_at = if split_at == 0 {
                    current
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| i)
                        .unwrap_or(current.len())
                } else {
                    split_at
                };
                chunks.push(current[..split_at].to_string());
                current = current[split_at..].to_string();
            }
        }
    }
    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }

    chunks
}

/// Escape special characters for Telegram MarkdownV2 format.
pub fn escape_markdown_v2(text: &str) -> String {
    // MarkdownV2 special chars: _ * [ ] ( ) ~ ` > # + - = | { } . !
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|'
            | '{' | '}' | '.' | '!' => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    result
}

pub struct TerminalAdapter {
    pub channel_id: ChannelId,
}

impl TerminalAdapter {
    pub fn new(channel_id: ChannelId) -> Self {
        Self { channel_id }
    }
}

impl ChannelAdapter for TerminalAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Terminal
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        Ok(vec![])
    }

    fn send(&self, msg: &OutboundMessage) -> Result<(), AdapterError> {
        println!("[{}] {}", msg.channel_id, msg.text);
        Ok(())
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn telegram_plain_text_send_body_omits_parse_mode() {
        let body = telegram_send_body("42", "Hello_world.", None, Some("7"));
        assert_eq!(body["chat_id"], "42");
        assert_eq!(body["text"], "Hello_world.");
        assert_eq!(body["reply_to_message_id"], "7");
        assert!(body.get("parse_mode").is_none());
    }

    #[test]
    fn telegram_markdown_v2_body_escapes_text() {
        let body = telegram_send_body("42", "Hello_world.", Some("MarkdownV2"), None);
        assert_eq!(body["parse_mode"], "MarkdownV2");
        assert_eq!(body["text"], "Hello\\_world\\.");
    }
}
