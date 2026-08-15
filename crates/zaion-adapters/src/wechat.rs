//! WeCom (企业微信) platform adapter implementation.
//!
//! Integrates with WeCom Bot API:
//!   - Webhook-based message receiving
//!   - Send messages via WeCom API (text + markdown)
//!   - Access token auto-refresh with expiry buffer
//!
//! Security:
//!   H27 — single `reqwest::blocking::Client` reused for all API calls.
//!   Corp secret wrapped in `Zeroizing` for memory safety.

use crate::channel::{ChannelAdapter, ChannelType, InboundMessage, OutboundMessage};
use crate::AdapterError;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const WECOM_API_BASE: &str = "https://qyapi.weixin.qq.com/cgi-bin";
const TOKEN_REFRESH_BUFFER: Duration = Duration::from_secs(5 * 60);
const TOKEN_TTL_SECS: u64 = 7200;

pub struct WeChatAdapter {
    corp_id: String,
    corp_secret: Zeroizing<String>,
    agent_id: String,
    client: reqwest::blocking::Client,
    api_base_url: String,
    token_cache: Mutex<Option<(String, Instant)>>,
    /// Buffer for messages received via webhook callback.
    inbound_buffer: Mutex<Vec<InboundMessage>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeComDeliveryReport {
    pub chat_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

impl WeChatAdapter {
    pub fn new(
        corp_id: impl Into<String>,
        corp_secret: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest::blocking::Client builder must succeed");

        Self {
            corp_id: corp_id.into(),
            corp_secret: Zeroizing::new(corp_secret.into()),
            agent_id: agent_id.into(),
            client,
            api_base_url: WECOM_API_BASE.to_string(),
            token_cache: Mutex::new(None),
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

    fn api_url_for(&self, endpoint: &str) -> String {
        format!("{}/{}", self.api_base_url, endpoint)
    }

    fn redact_error(&self, text: impl Into<String>, extra_secrets: &[&str]) -> String {
        let mut values = vec![self.corp_secret.as_str()];
        values.extend_from_slice(extra_secrets);
        redact_sensitive_values(text.into(), &values)
    }

    fn validate_credentials(&self) -> Result<(), AdapterError> {
        if self.corp_id.is_empty() || self.corp_secret.is_empty() {
            return Err(AdapterError::Channel(
                "WeCom credentials not configured".into(),
            ));
        }
        Ok(())
    }

    /// Returns a valid access token, refreshing when near expiry.
    fn ensure_token(&self) -> Result<String, AdapterError> {
        let mut guard = self
            .token_cache
            .lock()
            .map_err(|_| AdapterError::Channel("wechat token cache lock poisoned".into()))?;

        if let Some((ref token, expires_at)) = *guard {
            if Instant::now() + TOKEN_REFRESH_BUFFER < expires_at {
                return Ok(token.clone());
            }
        }

        let (token, ttl) = self.fetch_token()?;
        *guard = Some((token.clone(), Instant::now() + ttl));
        Ok(token)
    }

    fn fetch_token(&self) -> Result<(String, Duration), AdapterError> {
        let url = format!(
            "{}?corpid={}&corpsecret={}",
            self.api_url_for("gettoken"),
            self.corp_id,
            self.corp_secret.as_str()
        );

        let resp = self.client.get(&url).send().map_err(|e| {
            AdapterError::Channel(
                self.redact_error(format!("WeCom token request failed: {}", e), &[]),
            )
        })?;

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| AdapterError::Channel(format!("WeCom token parse error: {}", e)))?;

        let errcode = json["errcode"].as_i64().unwrap_or(-1);
        if errcode != 0 {
            let errmsg = json["errmsg"].as_str().unwrap_or("unknown");
            return Err(AdapterError::Channel(format!(
                "WeCom token error {}: {}",
                errcode,
                self.redact_error(errmsg, &[])
            )));
        }

        let token = json["access_token"]
            .as_str()
            .ok_or_else(|| AdapterError::Channel("missing access_token".into()))?
            .to_string();
        let ttl = json["expires_in"].as_u64().unwrap_or(TOKEN_TTL_SECS);
        Ok((token, Duration::from_secs(ttl)))
    }

    /// Push a message into the inbound buffer (called by webhook handler).
    pub fn push_inbound(&self, msg: InboundMessage) {
        if let Ok(mut buf) = self.inbound_buffer.lock() {
            buf.push(msg);
        }
    }

    pub fn send_with_report(
        &self,
        msg: &OutboundMessage,
    ) -> Result<WeComDeliveryReport, AdapterError> {
        self.validate_credentials()?;
        let token = self.ensure_token()?;

        let is_markdown = msg.parse_mode.as_deref() == Some("markdown");

        let body = if is_markdown {
            serde_json::json!({
                "touser": msg.thread_id,
                "msgtype": "markdown",
                "agentid": self.agent_id,
                "markdown": {
                    "content": msg.text,
                }
            })
        } else {
            serde_json::json!({
                "touser": msg.thread_id,
                "msgtype": "text",
                "agentid": self.agent_id,
                "text": {
                    "content": msg.text,
                }
            })
        };

        let url = format!(
            "{}?access_token={}",
            self.api_url_for("message/send"),
            token
        );
        let resp = self.client.post(&url).json(&body).send().map_err(|e| {
            AdapterError::Channel(self.redact_error(format!("WeCom send failed: {}", e), &[&token]))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(AdapterError::Channel(format!(
                "WeCom HTTP {}: {}",
                status,
                self.redact_error(text, &[&token])
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| AdapterError::Channel(format!("WeCom response parse error: {}", e)))?;

        let errcode = json["errcode"].as_i64().unwrap_or(-1);
        if errcode != 0 {
            let errmsg = json["errmsg"].as_str().unwrap_or("unknown");
            return Err(AdapterError::Channel(format!(
                "WeCom send error {}: {}",
                errcode,
                self.redact_error(errmsg, &[&token])
            )));
        }

        let message_id = json
            .get("msgid")
            .or_else(|| json.get("message_id"))
            .or_else(|| json.get("messageId"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        Ok(WeComDeliveryReport {
            chat_id: msg.thread_id.clone(),
            message_id,
            character_count: msg.text.chars().count(),
        })
    }
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

impl ChannelAdapter for WeChatAdapter {
    fn channel_type(&self) -> ChannelType {
        ChannelType::WeChat
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, AdapterError> {
        self.validate_credentials()?;
        let mut buf = self
            .inbound_buffer
            .lock()
            .map_err(|_| AdapterError::Channel("wechat inbound buffer lock poisoned".into()))?;
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

    fn spawn_wecom_mock<F>(
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
                let mut buf = [0u8; 2048];
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
    fn wechat_adapter_creation() {
        let adapter = WeChatAdapter::new("corp123", "secret456", "1000002");
        assert_eq!(adapter.corp_id, "corp123");
        assert_eq!(adapter.corp_secret.as_str(), "secret456");
        assert_eq!(adapter.agent_id, "1000002");
    }

    #[test]
    fn wechat_validate_rejects_empty_credentials() {
        let adapter = WeChatAdapter::new("", "", "agent");
        let err = adapter.validate_credentials().unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn wechat_validate_accepts_valid_credentials() {
        let adapter = WeChatAdapter::new("corp", "secret", "agent");
        assert!(adapter.validate_credentials().is_ok());
    }

    #[test]
    fn wechat_channel_type() {
        let adapter = WeChatAdapter::new("c", "s", "a");
        assert_eq!(adapter.channel_type(), ChannelType::WeChat);
    }

    #[test]
    fn wechat_receive_empty_buffer() {
        let adapter = WeChatAdapter::new("corp", "secret", "agent");
        let msgs = adapter.receive().unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn wechat_push_and_receive() {
        let adapter = WeChatAdapter::new("corp", "secret", "agent");
        adapter.push_inbound(InboundMessage {
            channel_id: "wechat".into(),
            thread_id: "user001".into(),
            message_id: "msg1".into(),
            sender_id: "user001".into(),
            text: "你好".into(),
            timestamp: "1650000000".into(),
            metadata: serde_json::json!({}),
        });
        let msgs = adapter.receive().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "你好");
        // Buffer drained
        assert!(adapter.receive().unwrap().is_empty());
    }

    #[test]
    fn wechat_send_rejects_empty_credentials() {
        let adapter = WeChatAdapter::new("", "", "agent");
        let msg = OutboundMessage {
            channel_id: "wechat".into(),
            thread_id: "user001".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };
        let err = adapter.send(&msg).unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn wechat_api_url_format() {
        let adapter = WeChatAdapter::new("corp", "secret", "agent");
        assert_eq!(
            adapter.api_url_for("message/send"),
            "https://qyapi.weixin.qq.com/cgi-bin/message/send"
        );
    }

    #[test]
    fn wechat_api_base_override_formats_urls() {
        let adapter = WeChatAdapter::new("corp", "secret", "agent")
            .with_api_base_url("http://127.0.0.1:9914/");
        assert_eq!(
            adapter.api_url_for("message/send"),
            "http://127.0.0.1:9914/message/send"
        );
    }

    #[test]
    fn wechat_corp_secret_wrapped_in_zeroizing() {
        let adapter = WeChatAdapter::new("c", "s", "a");
        let _: &Zeroizing<String> = &adapter.corp_secret;
    }

    #[test]
    fn wechat_token_cache_starts_empty() {
        let adapter = WeChatAdapter::new("c", "s", "a");
        let guard = adapter.token_cache.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn wecom_token_errors_redact_corp_secret() {
        let secret = "corp-secret-live";
        let (base_url, server) = spawn_wecom_mock(1, move |_request| {
            serde_json::json!({
                "errcode": 40001,
                "errmsg": format!("invalid corpsecret={secret}")
            })
            .to_string()
        });
        let adapter = WeChatAdapter::new("corp-id", secret, "agent").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "wecom".into(),
            thread_id: "user001".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: Some("markdown".into()),
        };

        let err = adapter.send_with_report(&msg).unwrap_err().to_string();
        assert!(!err.contains(secret), "error leaked corp secret: {err}");
        assert!(err.contains("[REDACTED]"), "error was not redacted: {err}");
        server.join().unwrap();
    }

    #[test]
    fn wecom_send_errors_redact_access_token_and_corp_secret() {
        let secret = "corp-secret-live";
        let token = "wecom-access-token-live";
        let (base_url, server) = spawn_wecom_mock(2, move |request| {
            if request.contains("/gettoken") {
                serde_json::json!({
                    "errcode": 0,
                    "access_token": token,
                    "expires_in": 7200
                })
                .to_string()
            } else {
                serde_json::json!({
                    "errcode": 40003,
                    "errmsg": format!("echoed access_token={token} corpsecret={secret}")
                })
                .to_string()
            }
        });
        let adapter = WeChatAdapter::new("corp-id", secret, "agent").with_api_base_url(base_url);
        let msg = OutboundMessage {
            channel_id: "wecom".into(),
            thread_id: "user001".into(),
            text: "test".into(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: Some("markdown".into()),
        };

        let err = adapter.send_with_report(&msg).unwrap_err().to_string();
        assert!(!err.contains(secret), "error leaked corp secret: {err}");
        assert!(!err.contains(token), "error leaked access token: {err}");
        assert!(err.contains("[REDACTED]"), "error was not redacted: {err}");
        server.join().unwrap();
    }
}
