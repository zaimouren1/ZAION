//! Mattermost platform adapter implementation.
//!
//! This adapter targets the Mattermost REST API v4 post path so webhook
//! delivery probes can verify Mattermost backend delivery with isolated mock
//! servers or an explicit `MATTERMOST_URL` deployment target.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use std::sync::Mutex;
use std::time::Duration;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MATTERMOST_MESSAGE_LIMIT: usize = 4000;

pub struct MattermostAdapter {
    access_token: Zeroizing<String>,
    api_base_url: String,
    client: reqwest::blocking::Client,
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MattermostDeliveryReport {
    pub channel_id: String,
    pub post_id: Option<String>,
    pub chunk_count: usize,
    pub character_count: usize,
}

impl MattermostAdapter {
    pub fn new(access_token: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed");
        let api_base_url = std::env::var("MATTERMOST_URL")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();

        Self {
            access_token: Zeroizing::new(access_token.into()),
            api_base_url,
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
                "Mattermost access_token not configured".into(),
            ));
        }
        Ok(())
    }

    fn posts_url(&self) -> Result<String, AdapterError> {
        if self.api_base_url.trim().is_empty() {
            return Err(AdapterError::Channel(
                "Mattermost API base URL not configured; set MATTERMOST_URL".into(),
            ));
        }
        let mut url = reqwest::Url::parse(&self.api_base_url).map_err(|e| {
            AdapterError::Channel(format!("Mattermost API base URL invalid: {}", e))
        })?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                AdapterError::Channel("Mattermost API base URL cannot be path-segmented".into())
            })?;
            segments.pop_if_empty();
            segments.extend(["api", "v4", "posts"]);
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
    ) -> Result<MattermostDeliveryReport, AdapterError> {
        self.validate_token()?;
        let posts_url = self.posts_url()?;
        let chunks = chunk_mattermost_message(&msg.text, MATTERMOST_MESSAGE_LIMIT);
        let mut post_ids = Vec::new();

        for chunk in &chunks {
            let mut body = serde_json::json!({
                "channel_id": msg.thread_id,
                "message": chunk,
                "props": {
                    "zaion_source": msg.metadata.get("source"),
                    "zaion_subscription": msg.metadata.get("subscription"),
                    "zaion_event": msg.metadata.get("event"),
                }
            });
            if let Some(reply_to) = msg.reply_to.as_deref().filter(|value| !value.is_empty()) {
                body["root_id"] = serde_json::Value::String(reply_to.to_string());
            }

            let resp = self
                .client
                .post(&posts_url)
                .bearer_auth(self.access_token.as_str())
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| {
                    AdapterError::Channel(
                        self.redact_error(format!("Mattermost API request failed: {}", e)),
                    )
                })?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().unwrap_or_default();
                return Err(AdapterError::Channel(format!(
                    "Mattermost API HTTP {}: {}",
                    status,
                    self.redact_error(text)
                )));
            }

            let json: serde_json::Value = resp.json().map_err(|e| {
                AdapterError::Channel(format!("Mattermost response parse error: {}", e))
            })?;
            if let Some(error) = json
                .get("message")
                .or_else(|| json.get("error"))
                .and_then(|value| value.as_str())
                .filter(|_| json.get("id").and_then(|value| value.as_str()).is_none())
            {
                return Err(AdapterError::Channel(format!(
                    "Mattermost API error: {}",
                    self.redact_error(error)
                )));
            }
            if let Some(post_id) = json.get("id").and_then(|value| value.as_str()) {
                post_ids.push(post_id.to_string());
            }
        }

        Ok(MattermostDeliveryReport {
            channel_id: msg.thread_id.clone(),
            post_id: post_ids.into_iter().next(),
            chunk_count: chunks.len(),
            character_count: msg.text.chars().count(),
        })
    }
}

fn chunk_mattermost_message(text: &str, max_len: usize) -> Vec<String> {
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

impl ChannelAdapter for MattermostAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Mattermost
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("mattermost inbound buffer lock poisoned".into()))?;
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn spawn_mattermost_mock<F>(
        expected_requests: usize,
        mut handler: F,
    ) -> (String, thread::JoinHandle<()>)
    where
        F: FnMut(String) -> String + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(expected_requests) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = handler(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{}", addr), handle)
    }

    #[test]
    fn mattermost_channel_type() {
        let adapter = MattermostAdapter::new("mattermost-token");
        assert_eq!(adapter.channel_type(), ChannelType::Mattermost);
    }

    #[test]
    fn mattermost_posts_url_formats_api_v4_path() {
        let adapter =
            MattermostAdapter::new("mattermost-token").with_api_base_url("http://127.0.0.1:9919/");
        assert_eq!(
            adapter.posts_url().unwrap(),
            "http://127.0.0.1:9919/api/v4/posts"
        );
    }

    #[test]
    fn mattermost_receive_drains_buffer() {
        let adapter = MattermostAdapter::new("mattermost-token").with_api_base_url("http://mm");
        adapter.push_inbound(InboundMessage {
            channel_id: "mattermost".into(),
            thread_id: "research-channel".into(),
            message_id: "post-1".into(),
            sender_id: "user-1".into(),
            text: "hello".into(),
            timestamp: "1710000000".into(),
            metadata: serde_json::json!({}),
        });
        let messages = adapter.receive().unwrap();
        assert_eq!(messages.len(), 1);
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn mattermost_send_with_report_posts_to_api_v4() {
        let (base_url, server) = spawn_mattermost_mock(1, |request| {
            assert!(request.starts_with("POST /api/v4/posts "));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer mattermost-token"));
            assert!(request.contains("\"channel_id\":\"research-channel\""));
            serde_json::json!({
                "id": "mattermost-post-1",
                "channel_id": "research-channel",
            })
            .to_string()
        });
        let adapter = MattermostAdapter::new("mattermost-token").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "mattermost".into(),
            thread_id: "research-channel".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({"source": "webhook"}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&msg).unwrap();
        assert_eq!(report.channel_id, "research-channel");
        assert_eq!(report.post_id.as_deref(), Some("mattermost-post-1"));
        assert_eq!(report.chunk_count, 1);
        server.join().unwrap();
    }

    #[test]
    fn mattermost_api_errors_redact_access_token() {
        let token = "mattermost-secret-token";
        let err = MattermostAdapter::new(token).redact_error(format!("token={token}"));
        assert!(!err.contains(token));
        assert!(err.contains("[REDACTED]"));
    }
}
