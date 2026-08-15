//! Slack platform adapter implementation.
//!
//! Integrates with Slack Bot API:
//!   - OAuth Bot token authentication
//!   - Receive messages via Events API (webhook-style polling placeholder)
//!   - Send messages via chat.postMessage
//!   - Markdown (mrkdwn) formatting support
//!
//! Security:
//!   H27 — single `reqwest::blocking::Client` reused for all API calls.
//!   Bot token wrapped in `Zeroizing` for memory safety.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use std::sync::Mutex;
use std::time::Duration;
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const SLACK_API_BASE: &str = "https://slack.com/api";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackDeliveryReport {
    pub channel_id: String,
    pub message_ts: Option<String>,
    pub character_count: usize,
}

pub struct SlackAdapter {
    bot_token: Zeroizing<String>,
    _channel_id: String,
    api_base_url: String,
    client: reqwest::blocking::Client,
    /// Buffer for messages received via Events API webhook.
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

impl SlackAdapter {
    pub fn new(bot_token: impl Into<String>, channel_id: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed with default config");

        Self {
            bot_token: Zeroizing::new(bot_token.into()),
            _channel_id: channel_id.into(),
            api_base_url: SLACK_API_BASE.to_string(),
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

    fn api_url(&self, method: &str) -> String {
        format!("{}/{}", self.api_base_url, method)
    }

    /// Validate that the bot token is non-empty.
    fn validate_token(&self) -> Result<(), AdapterError> {
        if self.bot_token.is_empty() {
            return Err(AdapterError::Channel(
                "Slack bot_token not configured".into(),
            ));
        }
        Ok(())
    }

    /// Push a message into the inbound buffer (called by webhook handler).
    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    /// Convert Slack mrkdwn to the message text.
    /// Slack uses its own mrkdwn format: *bold*, _italic_, ~strike~, `code`.
    pub fn to_mrkdwn(text: &str) -> String {
        text.to_string()
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<SlackDeliveryReport, AdapterError> {
        self.validate_token()?;

        let text = if msg.parse_mode.as_deref() == Some("mrkdwn") {
            Self::to_mrkdwn(&msg.text)
        } else {
            msg.text.clone()
        };

        let body = serde_json::json!({
            "channel": msg.thread_id,
            "text": text,
            "thread_ts": msg.reply_to,
        });

        let resp = self
            .client
            .post(self.api_url("chat.postMessage"))
            .header(
                "Authorization",
                format!("Bearer {}", self.bot_token.as_str()),
            )
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .map_err(|e| AdapterError::Channel(format!("Slack API request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "Slack API HTTP {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| AdapterError::Channel(format!("Slack response parse error: {}", e)))?;

        if !json["ok"].as_bool().unwrap_or(false) {
            let err = json["error"].as_str().unwrap_or("unknown");
            return Err(AdapterError::Channel(format!("Slack API error: {}", err)));
        }

        let message_ts = json
            .get("ts")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Ok(SlackDeliveryReport {
            channel_id: msg.thread_id.clone(),
            message_ts,
            character_count: text.chars().count(),
        })
    }
}

impl ChannelAdapter for SlackAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Slack
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_token()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("slack inbound buffer lock poisoned".into()))?;
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
    fn slack_adapter_creation() {
        let adapter = SlackAdapter::new("xoxb-test-token", "C12345");
        assert_eq!(adapter.bot_token.as_str(), "xoxb-test-token");
        assert_eq!(adapter._channel_id, "C12345");
    }

    #[test]
    fn slack_validate_token_rejects_empty() {
        let adapter = SlackAdapter::new("", "C12345");
        let err = adapter.validate_token().unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn slack_validate_token_accepts_valid() {
        let adapter = SlackAdapter::new("xoxb-valid", "C12345");
        assert!(adapter.validate_token().is_ok());
    }

    #[test]
    fn slack_channel_type() {
        let adapter = SlackAdapter::new("token", "ch");
        assert_eq!(adapter.channel_type(), ChannelType::Slack);
    }

    #[test]
    fn slack_receive_empty_buffer() {
        let adapter = SlackAdapter::new("xoxb-token", "C12345");
        let msgs = adapter.receive().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn slack_push_and_receive() {
        let adapter = SlackAdapter::new("xoxb-token", "C12345");
        adapter.push_inbound(InboundMessage {
            channel_id: "slack".into(),
            thread_id: "C12345".into(),
            message_id: "msg1".into(),
            sender_id: "U001".into(),
            text: "hello".into(),
            timestamp: "1234567890.000100".into(),
            metadata: serde_json::json!({}),
        });
        let msgs = adapter.receive().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "hello");
        // Buffer should be drained
        let msgs2 = adapter.receive().unwrap();
        assert!(msgs2.is_empty());
    }

    #[test]
    fn slack_send_rejects_empty_token() {
        let adapter = SlackAdapter::new("", "C12345");
        let msg = OutboundMessage {
            channel_id: "slack".into(),
            thread_id: "C12345".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };
        let err = adapter.send(&msg).unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn slack_mrkdwn_passthrough() {
        let input = "*bold* _italic_ ~strike~ `code`";
        let output = SlackAdapter::to_mrkdwn(input);
        assert_eq!(output, input);
    }

    #[test]
    fn slack_api_url_format() {
        assert_eq!(
            SlackAdapter::new("token", "ch").api_url("chat.postMessage"),
            "https://slack.com/api/chat.postMessage"
        );
    }

    #[test]
    fn slack_bot_token_wrapped_in_zeroizing() {
        let adapter = SlackAdapter::new("secret", "ch");
        let _: &Zeroizing<String> = &adapter.bot_token;
    }
}
