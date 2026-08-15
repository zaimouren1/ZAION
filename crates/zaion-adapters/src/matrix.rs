//! Matrix platform adapter implementation.
//!
//! This adapter targets the Matrix Client-Server API send path so webhook
//! delivery probes can verify Matrix backend delivery with isolated mock
//! homeservers.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use std::sync::Mutex;
use std::time::Duration;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MATRIX_API_BASE: &str = "https://matrix.org";
const MATRIX_MESSAGE_LIMIT: usize = 4000;

pub struct MatrixAdapter {
    access_token: Zeroizing<String>,
    api_base_url: String,
    client: reqwest::blocking::Client,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixDeliveryReport {
    pub room_id: String,
    pub event_id: Option<String>,
    pub chunk_count: usize,
    pub character_count: usize,
}

impl MatrixAdapter {
    pub fn new(access_token: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed");

        Self {
            access_token: Zeroizing::new(access_token.into()),
            api_base_url: MATRIX_API_BASE.to_string(),
            client,
            inbound_buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .no_proxy()
                .build()
            {
                self.client = client;
            }
        }
        self
    }

    fn validate_token(&self) -> Result<(), AdapterError> {
        if self.access_token.trim().is_empty() {
            return Err(AdapterError::Channel(
                "Matrix access_token not configured".into(),
            ));
        }
        Ok(())
    }

    fn room_send_url(&self, room_id: &str, txn_id: &str) -> Result<String, AdapterError> {
        let mut url = reqwest::Url::parse(&self.api_base_url)
            .map_err(|e| AdapterError::Channel(format!("Matrix API base URL invalid: {}", e)))?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Matrix API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend([
                "_matrix",
                "client",
                "v3",
                "rooms",
                room_id,
                "send",
                "m.room.message",
                txn_id,
            ]);
        }
        Ok(url.to_string())
    }

    fn redact_error(&self, text: impl Into<String>) -> String {
        redact_sensitive_values(text.into(), &[self.access_token.as_str()])
    }

    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<MatrixDeliveryReport, AdapterError> {
        self.validate_token()?;

        let chunks = chunk_matrix_message(&msg.text, MATRIX_MESSAGE_LIMIT);
        let mut event_ids = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let txn_id = format!("zaion-{}-{}", uuid::Uuid::new_v4().simple(), index);
            let url = self.room_send_url(&msg.thread_id, &txn_id)?;
            let body = serde_json::json!({
                "msgtype": "m.text",
                "body": chunk,
                "zaion": {
                    "source": msg.metadata.get("source"),
                    "subscription": msg.metadata.get("subscription"),
                    "event": msg.metadata.get("event"),
                }
            });
            let resp = self
                .client
                .put(url)
                .bearer_auth(self.access_token.as_str())
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| {
                    AdapterError::Channel(
                        self.redact_error(format!("Matrix API request failed: {}", e)),
                    )
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                return Err(AdapterError::Channel(format!(
                    "Matrix API HTTP {}: {}",
                    status,
                    self.redact_error(text)
                )));
            }

            let json: serde_json::Value = resp.json().map_err(|e| {
                AdapterError::Channel(format!("Matrix response parse error: {}", e))
            })?;
            if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
                return Err(AdapterError::Channel(format!(
                    "Matrix API error: {}",
                    self.redact_error(error)
                )));
            }
            if let Some(event_id) = json.get("event_id").and_then(|value| value.as_str()) {
                event_ids.push(event_id.to_string());
            }
        }

        Ok(MatrixDeliveryReport {
            room_id: msg.thread_id.clone(),
            event_id: event_ids.into_iter().next(),
            chunk_count: chunks.len(),
            character_count: msg.text.chars().count(),
        })
    }
}

fn chunk_matrix_message(text: &str, max_len: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= max_len {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn redact_sensitive_values(text: String, values: &[&str]) -> String {
    let mut redacted = text;
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        redacted = redacted.replace(value, "[REDACTED]");
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        if encoded != value {
            redacted = redacted.replace(&encoded, "[REDACTED]");
        }
    }
    redacted
}

impl ChannelAdapter for MatrixAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Matrix
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("matrix inbound buffer lock poisoned".into()))?;
        let messages = buf.drain(..).collect();
        Ok(messages)
    }

    fn send(&self, msg: &OutboundMessage) -> Result<(), AdapterError> {
        self.send_with_report(msg)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_adapter_creation() {
        let adapter = MatrixAdapter::new("matrix-token");
        assert_eq!(adapter.access_token.as_str(), "matrix-token");
    }

    #[test]
    fn matrix_channel_type() {
        let adapter = MatrixAdapter::new("matrix-token");
        assert_eq!(adapter.channel_type(), ChannelType::Matrix);
    }

    #[test]
    fn matrix_send_url_encodes_room_and_event_segments() {
        let adapter = MatrixAdapter::new("token").with_api_base_url("http://127.0.0.1:9918/");
        let url = adapter
            .room_send_url("!research:matrix.example", "txn:1")
            .unwrap();
        assert!(url.contains("/_matrix/client/v3/rooms/"));
        assert!(url.contains("!research:matrix.example"));
        assert!(url.contains("/send/m.room.message/txn:1"));
    }

    #[test]
    fn matrix_receive_drains_buffer() {
        let adapter = MatrixAdapter::new("matrix-token");
        adapter.push_inbound(InboundMessage {
            channel_id: "matrix".into(),
            thread_id: "!room:example".into(),
            message_id: "$event".into(),
            sender_id: "@user:example".into(),
            text: "hello".into(),
            timestamp: "1710000000".into(),
            metadata: serde_json::json!({}),
        });
        let messages = adapter.receive().unwrap();
        assert_eq!(messages.len(), 1);
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn matrix_api_errors_redact_access_token() {
        let token = "matrix-secret-token";
        let err = MatrixAdapter::new(token).redact_error(format!("token={token}"));
        assert!(!err.contains(token));
        assert!(err.contains("[REDACTED]"));
    }
}
